use engine::prelude::*;
use std::{any::Any, collections::HashMap};

use engine::{
    assets::material::Material,
    ecs::{commands::Commands, entity::Entity, query::Query, system::SystemContext},
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

pub const PLANET_OCEAN_PRODUCER: RenderProducerId =
    RenderProducerId::new("game.planet_ocean_producer");
const MAX_OCEAN_CHUNKS_PER_PLANET: u32 = 65_536;
const MAX_OCEAN_DRAWS: u32 = 65_536;

pub struct PlanetOceanProducer {
    routes: Vec<RenderRoute>,
    commands: Receiver<PlanetGenerationCommand>,
    events: Sender<PlanetOceanEvent>,
    commands_processed: RuntimeCounter,
    events_emitted: RuntimeCounter,
    _material: Material,
    pipelines: PlanetPipelines,
    ocean_layout: BindGroupLayoutHandle,
    material_palette: BufferHandle,
    chunk_indices_buffer: BufferHandle,
    planets: HashMap<Entity, PlanetGpuState>,
    indirect: IndirectBuffer,
    forward_batches: Vec<PreparedOceanBatch>,
    shadow_batches: Vec<PreparedOceanBatch>,
    batches_dirty: bool,
}

struct PlanetPipelines {
    forward: PipelineHandle,
    shadow: PipelineHandle,
}

struct PlanetGpuState {
    frame_buffer: BufferHandle,
    chunks_buffer: BufferHandle,
    bind_group: BindGroupHandle,
    ocean_chunks: HashMap<NodeKey, ChunkGpuState>,
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

fn planet_ocean_producer_init(ctx: &mut SystemContext, _commands: &mut Commands) {
    engine::profile_scope!("ocean.render.init");
    if ctx
        .globals
        .renderer
        .producer_mut::<PlanetOceanProducer>(PLANET_OCEAN_PRODUCER)
        .is_some()
    {
        return;
    }

    let (command_sender, command_receiver) = crossbeam_channel::unbounded();
    let (event_sender, event_receiver) = crossbeam_channel::unbounded();
    let commands_sent = RuntimeCounter::default();
    let commands_processed = RuntimeCounter::default();
    let events_emitted = RuntimeCounter::default();

    let producer = PlanetOceanProducer::create(
        &mut ctx.globals.renderer,
        command_receiver,
        event_sender,
        commands_processed.clone(),
        events_emitted.clone(),
    );

    ctx.world.insert_resource(PlanetOceanRenderQueue {
        sender: command_sender,
        commands_sent,
        commands_processed,
    });
    ctx.world.insert_resource(PlanetOceanEvents {
        receiver: event_receiver,
        events_emitted,
    });

    ctx.globals
        .renderer
        .register_producer(producer)
        .expect("planet ocean producer must only be registered once");
}

fn planet_ocean_producer_update(ctx: &mut SystemContext, _commands: &mut Commands) {
    engine::profile_scope!("ocean.render.queue_frames");
    let Some(queue) = ctx.world.get_resource::<PlanetOceanRenderQueue>() else {
        return;
    };
    let queue = queue.clone();
    let Some(camera) = ctx.world.get_resource::<crate::GameCamera>() else {
        return;
    };
    let camera_position = camera.world_position;
    let view_projection_rotation = engine::camera::OPENGL_TO_WGPU_MATRIX
        * camera.camera.build_projection_matrix()
        * Mat4::from_quat(camera.camera.orientation.inverse());
    drop(camera);

    let mut frames = Vec::new();
    let mut planets = Query::<(&game_types::planet::Planet,)>::new(ctx.world);
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

impl PlanetOceanProducer {
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

        let mut material = Material::new("shaders/planet_ocean.wgsl".into())
            .with_vertex_layouts(vec![
                PlanetVertex::layout(),
                PlanetOceanProducer::chunk_index_layout(),
            ])
            .with_blend(BlendMode::Alpha)
            .with_cull(CullMode::None);

        material.configure_pass(material_passes::SHADOW, |pass| {
            pass.pipeline.shader = "shaders/shadow_depth.wgsl".into();
            pass.vertex_entry = "vs_shadow".into();
            pass.fragment_entry = None;
            pass.pipeline.cull_mode = CullMode::None;
            pass.pipeline.depth_state = Some(DepthState {
                write_enabled: true,
                compare: CompareFunction::Less,
            });
        });

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

        let palette = PlanetOceanProducer::create_ocean_palette(renderer);
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

        let indirect_capacity = MAX_OCEAN_DRAWS;
        let indirect_buffer = renderer.renderer_api.create_buffer(&BufferDescriptor {
            label: "planet_ocean_indirect".into(),
            size: indirect_capacity as u64 * std::mem::size_of::<DrawIndexedIndirectArgs>() as u64,
            usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST,
        });
        let chunk_indices_buffer = renderer.renderer_api.create_buffer(&BufferDescriptor {
            label: "planet_ocean_chunk_indices".into(),
            size: u64::from(MAX_OCEAN_CHUNKS_PER_PLANET) * std::mem::size_of::<u32>() as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        });
        let chunk_indices: Vec<u32> = (0..MAX_OCEAN_CHUNKS_PER_PLANET).collect();
        renderer
            .renderer_api
            .write_buffer(chunk_indices_buffer, bytemuck::cast_slice(&chunk_indices));

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
            _material: material,
            pipelines: PlanetPipelines {
                forward,
                shadow: shadow_pipeline,
            },
            ocean_layout,
            material_palette,
            chunk_indices_buffer,
            planets: HashMap::new(),
            indirect: IndirectBuffer {
                buffer: indirect_buffer,
                capacity: indirect_capacity,
            },
            forward_batches: Vec::new(),
            shadow_batches: Vec::new(),
            batches_dirty: false,
        }
    }

    fn create_ocean_palette(
        renderer: &mut engine::renderer::Renderer,
    ) -> [GpuPlanetTerrainMaterial; 1] {
        PlanetOceanProducer::load_ocean_diffuse_texture(
            renderer,
            "blue_plaster_wall_2k/textures/blue_plaster_wall_diff_2k.jpg",
            "ocean_water_diffuse",
            500,
        );

        PlanetOceanProducer::load_ocean_normal_texture(
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

    fn ensure_planet(&mut self, api: &mut dyn RendererAPI, planet: Entity, frame: GpuPlanetFrame) {
        if let Some(state) = self.planets.get(&planet) {
            api.write_buffer(state.frame_buffer, bytemuck::bytes_of(&frame));
            return;
        }

        let frame_buffer = api.create_buffer(&BufferDescriptor {
            label: format!("planet_ocean_frame_{planet:?}"),
            size: std::mem::size_of::<GpuPlanetFrame>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });
        api.write_buffer(frame_buffer, bytemuck::bytes_of(&frame));

        let chunks_buffer = api.create_buffer(&BufferDescriptor {
            label: format!("planet_ocean_chunks_{planet:?}"),
            size: u64::from(MAX_OCEAN_CHUNKS_PER_PLANET)
                * std::mem::size_of::<GpuPlanetChunk>() as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        });

        let bind_group = api.create_bind_group(&BindGroupDescriptor {
            label: format!("planet_ocean_bindings_{planet:?}"),
            layout: self.ocean_layout,
            entries: vec![
                (0, BindGroupEntry::Buffer(self.material_palette)),
                (1, BindGroupEntry::Buffer(frame_buffer)),
                (2, BindGroupEntry::Buffer(chunks_buffer)),
            ],
        });

        self.planets.insert(
            planet,
            PlanetGpuState {
                frame_buffer,
                chunks_buffer,
                bind_group,
                ocean_chunks: HashMap::new(),
            },
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

    fn replace_chunks(
        &mut self,
        api: &mut dyn RendererAPI,
        planet: Entity,
        replacement_id: Option<u64>,
        remove_all: bool,
        remove: Vec<NodeKey>,
        insert: Vec<PendingChunkMesh>,
    ) {
        engine::profile_scope!("ocean.render.replace_chunks");
        if !self.planets.contains_key(&planet) {
            self.emit_event(PlanetOceanEvent::ReplacementFailed {
                planet,
                reason: "planet was not initialized".into(),
            });
            return;
        }

        let uploaded = {
            engine::profile_scope!("ocean.render.upload_meshes");
            let mut uploaded = Vec::with_capacity(insert.len());
            for chunk in &insert {
                if chunk.indices.is_empty() {
                    continue;
                }
                match PlanetOceanProducer::upload_chunk(api, chunk) {
                    Ok(mesh) => uploaded.push((chunk.key, chunk.node_origin_planet, mesh)),
                    Err(error) => {
                        for (_, _, mesh) in uploaded {
                            api.remove_mesh(mesh);
                        }
                        self.emit_event(PlanetOceanEvent::ReplacementFailed {
                            planet,
                            reason: error.to_string(),
                        });
                        return;
                    }
                }
            }
            uploaded
        };

        let state = self.planets.get_mut(&planet).unwrap();
        if remove_all {
            for old in state.ocean_chunks.drain().map(|(_, chunk)| chunk) {
                api.remove_mesh(old.mesh);
            }
        } else {
            let mut keys = remove;
            keys.extend(insert.iter().map(|chunk| chunk.key));
            keys.sort_unstable();
            keys.dedup();

            for key in keys {
                if let Some(old) = state.ocean_chunks.remove(&key) {
                    api.remove_mesh(old.mesh);
                }
            }
        }
        for (key, node_origin_planet, mesh) in uploaded {
            state.ocean_chunks.insert(
                key,
                ChunkGpuState {
                    mesh,
                    node_origin_planet,
                },
            );
        }

        self.batches_dirty = true;
        let rendered_keys = state.ocean_chunks.keys().copied().collect();
        self.emit_event(PlanetOceanEvent::ReplacementApplied {
            planet,
            replacement_id,
            rendered_keys,
        });
    }

    fn remove_planet(&mut self, api: &mut dyn RendererAPI, planet: Entity) {
        let Some(state) = self.planets.remove(&planet) else {
            return;
        };
        for chunk in state.ocean_chunks.into_values() {
            api.remove_mesh(chunk.mesh);
        }
        self.batches_dirty = true;
    }

    fn indirect_args(binding: GpuMeshBinding, chunk_index: u32) -> DrawIndexedIndirectArgs {
        DrawIndexedIndirectArgs {
            index_count: binding.draw_range.index_count,
            instance_count: 1,
            first_index: binding.draw_range.first_index,
            base_vertex: binding.draw_range.base_vertex,
            first_instance: chunk_index,
        }
    }

    fn rebuild_batches(&mut self, api: &mut dyn RendererAPI) {
        engine::profile_scope!("ocean.render.rebuild_batches");
        let mut grouped = HashMap::<OceanBatchKey, Vec<DrawIndexedIndirectArgs>>::new();

        for (&planet, state) in &self.planets {
            assert!(
                state.ocean_chunks.len() <= MAX_OCEAN_CHUNKS_PER_PLANET as usize,
                "planet {planet:?} exceeds the ocean chunk metadata capacity"
            );
            let mut chunks: Vec<_> = state.ocean_chunks.iter().collect();
            chunks.sort_unstable_by_key(|(key, _)| **key);
            let mut gpu_chunks = Vec::with_capacity(chunks.len());

            for (key, chunk) in chunks {
                let Some(binding) = api.get_gpu_mesh_binding(chunk.mesh) else {
                    continue;
                };
                let chunk_index = gpu_chunks.len() as u32;
                gpu_chunks.push(GpuPlanetChunk {
                    node_origin_planet: chunk.node_origin_planet,
                    level: i32::from(key.level),
                });
                grouped
                    .entry(OceanBatchKey {
                        planet,
                        vertex_buffer: binding.vertex_buffer,
                        index_buffer: binding.index_buffer,
                    })
                    .or_default()
                    .push(PlanetOceanProducer::indirect_args(binding, chunk_index));
            }

            if !gpu_chunks.is_empty() {
                api.write_buffer(state.chunks_buffer, bytemuck::cast_slice(&gpu_chunks));
            }
        }

        let command_count: usize = grouped.values().map(Vec::len).sum();
        assert!(
            command_count <= self.indirect.capacity as usize,
            "ocean draw count exceeds the indirect buffer capacity"
        );

        let stride = std::mem::size_of::<DrawIndexedIndirectArgs>() as u64;
        let mut commands = Vec::with_capacity(command_count);
        let mut batches = Vec::with_capacity(grouped.len());

        for (key, draws) in grouped {
            let first_command = commands.len() as u64;
            let draw_count = draws.len() as u32;
            commands.extend(draws);

            let state = &self.planets[&key.planet];
            batches.push(PreparedOceanBatch {
                bind_group: state.bind_group,
                vertex_buffer: key.vertex_buffer,
                index_buffer: key.index_buffer,
                indirect_offset: first_command * stride,
                draw_count,
            });
        }

        if !commands.is_empty() {
            api.write_buffer(self.indirect.buffer, bytemuck::cast_slice(&commands));
        }
        self.forward_batches = batches.clone();
        self.shadow_batches = batches;
    }
}

impl RenderProducer for PlanetOceanProducer {
    fn id(&self) -> RenderProducerId {
        PLANET_OCEAN_PRODUCER
    }

    fn routes(&self) -> &[RenderRoute] {
        &self.routes
    }

    fn prepare_frame(&mut self, ctx: &mut ProducerPrepareContext<'_>) {
        engine::profile_scope!("ocean.render.prepare_frame");
        let commands: Vec<_> = self.commands.try_iter().collect();
        self.commands_processed.add(commands.len());

        for command in &commands {
            match *command {
                PlanetGenerationCommand::EnsurePlanet { planet, frame } => {
                    engine::profile_scope!("ocean.render.prepare_frame.ensure_planet");
                    self.ensure_planet(ctx.api, planet, frame);
                }
                PlanetGenerationCommand::UpdatePlanetFrame { planet, frame } => {
                    if let Some(state) = self.planets.get(&planet) {
                        engine::profile_scope!("ocean.render.prepare_frame.write_buffer");
                        ctx.api
                            .write_buffer(state.frame_buffer, bytemuck::bytes_of(&frame));
                    }
                }
                PlanetGenerationCommand::ReplaceChunks { .. }
                | PlanetGenerationCommand::ReplaceOceanChunks { .. }
                | PlanetGenerationCommand::RemovePlanet { .. } => {}
            }
        }

        for command in commands {
            match command {
                PlanetGenerationCommand::EnsurePlanet { .. }
                | PlanetGenerationCommand::ReplaceChunks { .. }
                | PlanetGenerationCommand::UpdatePlanetFrame { .. } => {}
                // Ocean
                PlanetGenerationCommand::ReplaceOceanChunks {
                    planet,
                    replacement_id,
                    remove_all,
                    remove,
                    insert,
                } => {
                    engine::profile_scope!("ocean.render.prepare_frame.replace_chunks");
                    self.replace_chunks(ctx.api, planet, replacement_id, remove_all, remove, insert)
                }
                PlanetGenerationCommand::RemovePlanet { planet } => {
                    engine::profile_scope!("ocean.render.prepare_frame.remove_planet");
                    self.remove_planet(ctx.api, planet);
                }
            }
        }

        if self.batches_dirty {
            engine::profile_scope!("ocean.render.prepare_frame.rebuild_batches");
            self.rebuild_batches(ctx.api);
            self.batches_dirty = false;
        }
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

        ctx.bind_pipeline(pipeline);

        for batch in batches {
            ctx.bind_bind_group(2, batch.bind_group);
            ctx.bind_vertex_buffer(0, batch.vertex_buffer);
            ctx.bind_vertex_buffer(1, self.chunk_indices_buffer);
            ctx.bind_index_buffer(batch.index_buffer);
            ctx.multi_draw_indexed_indirect(
                self.indirect.buffer,
                batch.indirect_offset,
                batch.draw_count,
            );
        }
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

pub struct PlanetOceanProducerPlugin;
impl Plugin for PlanetOceanProducerPlugin {
    fn build(&self, app: &mut engine::App) {
        app.add_named_legacy_system(
            CoreSchedule::Startup,
            "game.ocean_producer_init",
            planet_ocean_producer_init,
        )
        .add_named_legacy_system(
            CoreSchedule::RenderExtract,
            "game.ocean_producer_update",
            planet_ocean_producer_update,
        );
    }
}
