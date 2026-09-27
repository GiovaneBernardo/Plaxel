use engine::prelude::*;
use std::{any::Any, collections::HashMap};

use engine::{
    assets::material::Material,
    ecs::{commands::Commands, entity::Entity, query::Query, resource::Res, system::GlobalsMut},
    math::Mat4,
    model::Vertex,
    reflect::RuntimeCounter,
    renderer::{
        BindGroupDescriptor, BindGroupEntry, BindGroupHandle, BindGroupLayoutHandle,
        BufferDescriptor, BufferHandle, BufferUsages, GpuMeshBinding, GpuMeshHandle, MeshUpload,
        MeshUploadError, PipelineHandle, ProducerPrepareContext, RenderContext, RenderPassContext,
        RenderProducer, RenderProducerId, RenderResources, RenderRoute, RendererAPI,
        TextureDescriptor, TextureDimension, TextureFormat, TextureSize, TextureUsages,
        material_passes,
    },
};
use game_types::{
    octree::NodeKey,
    planet::{GpuPlanetTerrainMaterial, PlanetVertex},
};

use crossbeam_channel::{Receiver, Sender};

use crate::GpuPlanetFrame;

pub const CLOUDS_PRODUCER: RenderProducerId = RenderProducerId::new("game.clouds_producer");

pub struct CloudsProducer {
    routes: Vec<RenderRoute>,
    commands: Receiver<PlanetGenerationCommand>,
    events: Sender<PlanetOceanEvent>,
    commands_processed: RuntimeCounter,
    events_emitted: RuntimeCounter,
    material: Material,
    pipelines: PlanetPipelines,
    ocean_layout: BindGroupLayoutHandle,
    forward_batches: Vec<PreparedOceanBatch>,
    shadow_batches: Vec<PreparedOceanBatch>,
    batches_dirty: bool,
}

struct PlanetPipelines {
    forward: PipelineHandle,
    shadow: PipelineHandle,
}

struct ChunkGpuState {
    mesh: GpuMeshHandle,
    node_origin_planet: [i32; 3],
}

struct IndirectBuffer {
    buffer: BufferHandle,
    capacity: u32,
}

#[derive(Clone, Copy)]
struct PreparedOceanBatch {
    bind_group: BindGroupHandle,
    vertex_buffer: BufferHandle,
    index_buffer: BufferHandle,
    indirect_offset: u64,
    draw_count: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DrawIndexedIndirectArgs {
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuPlanetChunk {
    node_origin_planet: [i32; 3],
    level: i32,
}

fn clouds_producer_init(mut globals: GlobalsMut, commands: &mut Commands) {
    engine::profile_scope!("ocean.render.init");

    let (command_sender, command_receiver) = crossbeam_channel::unbounded();
    let (event_sender, event_receiver) = crossbeam_channel::unbounded();
    let commands_sent = RuntimeCounter::default();
    let commands_processed = RuntimeCounter::default();
    let events_emitted = RuntimeCounter::default();

    let producer = CloudsProducer::create(
        &mut globals.renderer,
        command_receiver,
        event_sender,
        commands_processed.clone(),
        events_emitted.clone(),
    );

    commands.insert_resource(PlanetOceanRenderQueue {
        sender: command_sender,
        commands_sent,
        commands_processed,
    });
    commands.insert_resource(PlanetOceanEvents {
        receiver: event_receiver,
        events_emitted,
    });

    globals
        .renderer
        .register_producer(producer)
        .expect("planet ocean producer must only be registered once");
}

fn clouds_producer_update(
    queue: Option<Res<PlanetOceanRenderQueue>>,
    camera: Option<Res<crate::GameCamera>>,
    mut planets: Query<(&game_types::planet::Planet,)>,
) {
    engine::profile_scope!("ocean.render.queue_frames");
    let Some(queue) = queue else {
        return;
    };
    let Some(camera) = camera else {
        return;
    };
    let camera_position = camera.world_position;
    let view_projection_rotation = engine::camera::OPENGL_TO_WGPU_MATRIX
        * camera.camera.build_projection_matrix()
        * Mat4::from_quat(camera.camera.orientation.inverse());
    drop(camera);

    let mut frames = Vec::new();
    planets.for_each(|entity, (planet,)| {
        frames.push((
            entity,
            GpuPlanetFrame::new(view_projection_rotation, camera_position, planet.position),
        ));
    });

    for (planet, frame) in frames {
        queue
            .send(PlanetGenerationCommand::EnsurePlanet { planet, frame })
            .expect("planet ocean producer command channel must remain connected");
    }
}

impl CloudsProducer {
    fn emit_event(&self, event: PlanetOceanEvent) {
        if self.events.send(event).is_ok() {
            self.events_emitted.increment();
        }
    }

    fn chunk_index_layout() -> VertexLayout {
        VertexLayout {
            stride: std::mem::size_of::<u32>() as u64,
            step_mode: StepMode::Instance,
            attributes: vec![VertexAttribute {
                offset: 0,
                shader_location: 4,
                format: AttributeFormat::Uint32,
            }],
        }
    }

    fn create(
        renderer: &mut engine::renderer::Renderer,
        commands: Receiver<PlanetGenerationCommand>,
        events: Sender<PlanetOceanEvent>,
        commands_processed: RuntimeCounter,
        events_emitted: RuntimeCounter,
    ) -> Self {
        use engine::{assets::material::Material, model::Vertex, renderer::*};
        use game_types::planet::PlanetVertex;

        let mut material = Material::new("shaders/volumetric_clouds.wgsl".into())
            .with_vertex_layouts(vec![
                PlanetVertex::layout(),
                CloudsProducer::chunk_index_layout(),
            ])
            .with_blend(BlendMode::Alpha)
            .with_cull(CullMode::None);

        let camera_layout = renderer
            .render_graph
            .get_node_mut::<GeometryPassNode>(graph_passes::GEOMETRY)
            .and_then(|node| node.camera_bind_group_layout)
            .expect("geometry pass must be compiled before ocean initialization");

        let frame = renderer
            .render_resources
            .get_labeled::<FrameBindings>("frame_bindings")
            .expect("frame bindings must exist before ocean initialization");
        let textures_layout = frame.textures_layout;

        let shadow = *renderer
            .render_resources
            .get_labeled::<ShadowBindings>("shadow_bindings")
            .expect("shadow bindings must exist before ocean initialization");

        let ocean_layout =
            renderer
                .renderer_api
                .create_bind_group_layout(&BindGroupLayoutDescriptor {
                    label: "planet_ocean_layout".into(),
                    entries: vec![
                        BindGroupLayoutEntry {
                            binding: 0,
                            visibility: ShaderStages::Fragment,
                            entry_type: BindingType::StorageBuffer { read_only: true },
                            count: None,
                        },
                        BindGroupLayoutEntry {
                            binding: 1,
                            visibility: ShaderStages::Vertex,
                            entry_type: BindingType::UniformBuffer,
                            count: None,
                        },
                        BindGroupLayoutEntry {
                            binding: 2,
                            visibility: ShaderStages::Vertex,
                            entry_type: BindingType::StorageBuffer { read_only: true },
                            count: None,
                        },
                    ],
                });

        let palette = CloudsProducer::create_ocean_palette(renderer);
        let material_palette = renderer.renderer_api.create_buffer(&BufferDescriptor {
            label: "planet_ocean_palette".into(),
            size: std::mem::size_of_val(&palette) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        });
        renderer
            .renderer_api
            .write_buffer(material_palette, bytemuck::cast_slice(&palette));

        let geometry_target = renderer.renderer_api.target_info_for_pass(
            &GeometryPassNode::pass_descriptor(),
            &renderer.render_graph.resources,
        );
        let forward = renderer.renderer_api.create_pipeline(
            &material,
            material_passes::FORWARD_OPAQUE,
            &[
                camera_layout,
                textures_layout,
                ocean_layout,
                shadow.sampling_layout,
            ],
            &geometry_target,
        );

        let shadow_target = renderer.renderer_api.target_info_for_pass(
            &ShadowPassNode::pass_descriptor(),
            &renderer.render_graph.resources,
        );
        let shadow_pipeline = renderer.renderer_api.create_pipeline(
            &material,
            material_passes::SHADOW,
            &[shadow.view_layout, textures_layout, ocean_layout],
            &shadow_target,
        );

        //let indirect_capacity = MAX_OCEAN_DRAWS;
        //let indirect_buffer = renderer.renderer_api.create_buffer(&BufferDescriptor {
        //    label: "planet_ocean_indirect".into(),
        //    size: indirect_capacity as u64 * std::mem::size_of::<DrawIndexedIndirectArgs>() as u64,
        //    usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST,
        //});
        //let chunk_indices_buffer = renderer.renderer_api.create_buffer(&BufferDescriptor {
        //    label: "planet_ocean_chunk_indices".into(),
        //    size: u64::from(MAX_OCEAN_CHUNKS_PER_PLANET) * std::mem::size_of::<u32>() as u64,
        //    usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        //});
        //let chunk_indices: Vec<u32> = (0..MAX_OCEAN_CHUNKS_PER_PLANET).collect();
        //renderer
        //    .renderer_api
        //    .write_buffer(chunk_indices_buffer, bytemuck::cast_slice(&chunk_indices));

        Self {
            routes: vec![
                RenderRoute {
                    graph_pass: graph_passes::GEOMETRY,
                    material_pass: material_passes::FORWARD_OPAQUE,
                    phase: phases::OPAQUE,
                    views: RenderViewSelector::Main,
                },
                RenderRoute {
                    graph_pass: graph_passes::SHADOWS,
                    material_pass: material_passes::SHADOW,
                    phase: phases::OPAQUE,
                    views: RenderViewSelector::ShadowCascades,
                },
            ],
            commands,
            events,
            commands_processed,
            events_emitted,
            material: material,
            pipelines: PlanetPipelines {
                forward,
                shadow: shadow_pipeline,
            },
            ocean_layout,
            forward_batches: Vec::new(),
            shadow_batches: Vec::new(),
            batches_dirty: false,
        }
    }

    fn create_ocean_palette(
        renderer: &mut engine::renderer::Renderer,
    ) -> [GpuPlanetTerrainMaterial; 1] {
        CloudsProducer::load_ocean_diffuse_texture(
            renderer,
            "blue_plaster_wall_2k/textures/blue_plaster_wall_diff_2k.jpg",
            "ocean_water_diffuse",
            500,
        );

        CloudsProducer::load_ocean_normal_texture(
            renderer,
            "Ice002_2K-JPG_NormalDX.jpg",
            "terrain_water_normal",
            501,
        );

        // PlanetVertex material IDs address this palette directly. The order is
        // defined by game_types::ocean::ocean_materials.
        let ocean_materials = [GpuPlanetTerrainMaterial {
            diffuse_texture_index: 500,
            normal_texture_index: 501,
            displacement_texture_index: 0,
            roughness_texture_index: 0,
            texture_scale: 1.0,
            displacement_scale: 0.0,
            roughness_factor: 0.9,
            flags: 0,
        }];

        ocean_materials
    }

    fn load_ocean_diffuse_texture(
        renderer: &mut engine::renderer::Renderer,
        relative_path: &str,
        label: &str,
        texture_index: u32,
    ) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../res/terrain_textures")
            .join(relative_path);
        renderer.renderer_api.load_texture_to_index(
            &path.to_string_lossy().into_owned(),
            &TextureDescriptor {
                label: label.to_string(),
                format: TextureFormat::Rgba8Srgb,
                size: TextureSize::Custom {
                    width: 1,
                    height: 1,
                },
                dimension: TextureDimension::D2,
                usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
                mip_levels: 6,
                sample_count: 1,
            },
            Some(texture_index),
        );
    }

    fn load_ocean_normal_texture(
        renderer: &mut engine::renderer::Renderer,
        relative_path: &str,
        label: &str,
        texture_index: u32,
    ) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../res/terrain_textures")
            .join(relative_path);
        renderer.renderer_api.load_texture_to_index(
            &path.to_string_lossy().into_owned(),
            &TextureDescriptor {
                label: label.to_string(),
                format: TextureFormat::Rgba8Unorm,
                size: TextureSize::Custom {
                    width: 1,
                    height: 1,
                },
                dimension: TextureDimension::D2,
                usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
                mip_levels: 6,
                sample_count: 1,
            },
            Some(texture_index),
        );
    }

    fn upload_chunk(
        api: &mut dyn RendererAPI,
        chunk: &PendingChunkMesh,
    ) -> Result<GpuMeshHandle, MeshUploadError> {
        api.upload_mesh(MeshUpload {
            label: "planet_ocean_chunk",
            vertices: bytemuck::cast_slice(&chunk.vertices),
            indices: &chunk.indices,
            vertex_layout: &PlanetVertex::layout(),
        })
    }
}

impl RenderProducer for CloudsProducer {
    fn id(&self) -> RenderProducerId {
        CLOUDS_PRODUCER
    }

    fn routes(&self) -> &[RenderRoute] {
        &self.routes
    }

    fn prepare_frame(&mut self, ctx: &mut ProducerPrepareContext<'_>) {
        engine::profile_scope!("ocean.render.prepare_frame");
        let commands: Vec<_> = self.commands.try_iter().collect();
        self.commands_processed.add(commands.len());
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn record(
        &self,
        ctx: &mut dyn RenderContext,
        _resources: &RenderResources,
        pass: &RenderPassContext<'_>,
    ) {
        let (pipeline, batches) = match pass.route.material_pass {
            material_passes::FORWARD_OPAQUE => (self.pipelines.forward, &self.forward_batches),
            material_passes::SHADOW => (self.pipelines.shadow, &self.shadow_batches),
            _ => return,
        };

        //ctx.bind_pipeline(pipeline);
        //
        //for batch in batches {
        //    ctx.bind_bind_group(2, batch.bind_group);
        //    ctx.bind_vertex_buffer(0, batch.vertex_buffer);
        //    ctx.bind_vertex_buffer(1, self.chunk_indices_buffer);
        //    ctx.bind_index_buffer(batch.index_buffer);
        //    ctx.multi_draw_indexed_indirect(
        //        self.indirect.buffer,
        //        batch.indirect_offset,
        //        batch.draw_count,
        //    );
        //}
    }
}

pub(crate) struct PendingChunkMesh {
    pub key: NodeKey,
    pub node_origin_planet: [i32; 3],
    pub vertices: Vec<PlanetVertex>,
    pub indices: Vec<u32>,
}

pub(crate) enum PlanetGenerationCommand {
    EnsurePlanet {
        planet: Entity,
        frame: GpuPlanetFrame,
    },
    UpdatePlanetFrame {
        planet: Entity,
        frame: GpuPlanetFrame,
    },
    ReplaceChunks {
        planet: Entity,
        replacement_id: Option<u64>,
        remove_all: bool,
        remove: Vec<NodeKey>,
        insert: Vec<PendingChunkMesh>,
    },
    ReplaceOceanChunks {
        planet: Entity,
        replacement_id: Option<u64>,
        remove_all: bool,
        remove: Vec<NodeKey>,
        insert: Vec<PendingChunkMesh>,
    },
    RemovePlanet {
        planet: Entity,
    },
}

#[derive(Clone, plaxel_reflect::Reflect)]
#[reflect(from_reflect = false)]
pub(crate) struct PlanetOceanRenderQueue {
    #[reflect(ignore)]
    sender: Sender<PlanetGenerationCommand>,
    commands_sent: RuntimeCounter,
    commands_processed: RuntimeCounter,
}

impl PlanetOceanRenderQueue {
    pub(crate) fn send(
        &self,
        command: PlanetGenerationCommand,
    ) -> Result<(), crossbeam_channel::SendError<PlanetGenerationCommand>> {
        let result = self.sender.send(command);
        if result.is_ok() {
            self.commands_sent.increment();
        }
        result
    }
}

pub(crate) enum PlanetOceanEvent {
    ReplacementApplied {
        planet: Entity,
        replacement_id: Option<u64>,
        rendered_keys: Vec<NodeKey>,
    },
    ReplacementFailed {
        planet: Entity,
        reason: String,
    },
}

#[derive(plaxel_reflect::Reflect)]
#[reflect(from_reflect = false)]
pub(crate) struct PlanetOceanEvents {
    #[reflect(ignore)]
    receiver: crossbeam_channel::Receiver<PlanetOceanEvent>,
    events_emitted: RuntimeCounter,
}

impl PlanetOceanEvents {
    pub(crate) fn try_iter(&self) -> crossbeam_channel::TryIter<'_, PlanetOceanEvent> {
        self.receiver.try_iter()
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct OceanBatchKey {
    planet: Entity,
    vertex_buffer: BufferHandle,
    index_buffer: BufferHandle,
}

pub struct CloudsProducerPlugin;
impl Plugin for CloudsProducerPlugin {
    fn build(&self, app: &mut engine::App) {
        app.add_system(CoreSchedule::Startup, clouds_producer_init)
            .add_system(CoreSchedule::RenderExtract, clouds_producer_update);
    }
}
