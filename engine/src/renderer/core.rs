use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::prelude::*;
use crate::renderer::{DefaultMeshes, GpuMaterialData};
use bytemuck::{Pod, Zeroable};

use crate::Window;
use crate::assets::manager::Handle;
use crate::assets::material::Material;
pub use crate::core::camera;
use crate::ecs::world::World;
use crate::model::MeshAsset;
pub use crate::renderer::backends::*;
pub use crate::renderer::render_nodes::*;
use crate::renderer::wgpu_backend::WgpuBackend;
use crate::texture;
use wgpu;

#[derive(Debug, Copy, Clone)]
pub struct MeshDrawRange {
    pub first_index: u32,
    pub index_count: u32,
    pub base_vertex: i32,
}

pub struct Renderer {
    pub renderer_api: Box<dyn RendererAPI>,
    pub render_resources: RenderResources,
    pub render_graph: RenderGraph,
    pub pipelines: Vec<wgpu::RenderPipeline>,
    pub textures: Vec<texture::Texture>,
    pub render_database: RenderDatabase,
    pub producer_registry: RenderProducerRegistry,
    pub view_registry: RenderViewRegistry,
}

pub struct Texture {
    pub name: String,
    pub wgpu_texture: wgpu::Texture,
}

pub struct Buffer {
    pub name: String,
    pub wgpu_buffer: wgpu::Buffer,
}

pub struct RenderGraph {
    pub nodes: Vec<(GraphPassId, Box<dyn RenderNode>)>,
    pub resources: GraphResources,
    pub compiled: bool,
    pub(crate) disabled_nodes: HashSet<GraphPassId>,
}

/// A temporarily detached graph node together with its execution position.
/// Returning this token preserves graph order; `GraphPassId` identifies a pass
/// but deliberately carries no ordering semantics.
pub struct TakenRenderNode {
    pub id: GraphPassId,
    pub position: usize,
    pub node: Box<dyn RenderNode>,
}

impl std::ops::Deref for TakenRenderNode {
    type Target = dyn RenderNode;

    fn deref(&self) -> &Self::Target {
        self.node.as_ref()
    }
}

impl std::ops::DerefMut for TakenRenderNode {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.node.as_mut()
    }
}

pub struct PassResources {
    pub input_textures: Vec<Texture>,
    pub output_textures: Vec<Texture>,
    pub input_buffers: Vec<Buffer>,
    pub output_buffers: Vec<Buffer>,
}

pub trait RenderNode {
    /// Human-readable profiler label supplied automatically by the concrete pass type.
    fn profile_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    fn describe_pass(&self) -> RenderNodeDescriptor;
    fn compile(&mut self, ctx: &mut NodeCompileContext);
    fn prepare(&mut self, resources: &mut RenderResources, api: &mut dyn RendererAPI);
    fn run(&mut self, ctx: &mut dyn RenderContext, render_resources: &RenderResources);
    fn should_render_to_swapchain(&self) -> bool;
    fn needs_depth(&self) -> bool {
        true
    }
    fn reflect_mut(&mut self) -> Option<&mut dyn crate::reflect::PartialReflect> {
        None
    }
    fn resize(
        &mut self,
        ctx: &mut NodeCompileContext,
        graph_resources: &GraphResources,
        _width: u32,
        _height: u32,
    ) {
        let descriptor = self.describe_pass();
        for output_texture in descriptor.output_textures {
            match output_texture {
                OutputTexture::Create(create) => {
                    if graph_resources.textures.contains_key(create.name) {
                        ctx.api.resize_texture(
                            graph_resources.texture(create.name).unwrap(),
                            &create.texture_descriptor,
                        );
                    }
                }
                _ => {}
            }
        }
    }
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

#[derive(Clone)]
pub struct RenderData {
    pub mesh: Handle<MeshAsset>,
    pub material: Material,
    pub pipeline: PipelineHandle,
    pub transform_index: u32, // index into a GPU-side transform buffer
    pub sort_key: u64,        // for draw call sorting/batching

    pub extra_bind_groups: Vec<(u32, BindGroupHandle)>,
}

pub struct FrameBindings {
    pub camera_buffer: BufferHandle,
    pub camera_layout: BindGroupLayoutHandle,
    pub camera_bind_group: BindGroupHandle,

    pub textures_layout: BindGroupLayoutHandle,
    pub materials_bind_group: BindGroupHandle,
    pub materials_ssbo_buffer: BufferHandle,
}

pub struct RenderResources {
    map: HashMap<(TypeId, &'static str), Box<dyn Any + Send + Sync>>,
}

impl RenderResources {
    pub fn insert<T: Send + Sync + 'static>(&mut self, resource: T) {
        self.insert_labeled::<T>("", resource);
    }

    pub fn insert_labeled<T: Send + Sync + 'static>(&mut self, label: &'static str, resource: T) {
        self.map
            .insert((TypeId::of::<T>(), label), Box::new(resource));
    }

    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.get_labeled::<T>("")
    }

    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.get_labeled_mut::<T>("")
    }

    pub fn get_labeled<T: 'static>(&self, label: &'static str) -> Option<&T> {
        self.map.get(&(TypeId::of::<T>(), label))?.downcast_ref()
    }

    pub fn get_labeled_mut<T: 'static>(&mut self, label: &'static str) -> Option<&mut T> {
        self.map
            .get_mut(&(TypeId::of::<T>(), label))?
            .downcast_mut()
    }

    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }
}

impl Renderer {
    pub fn objects(&mut self) -> RenderObjectWriter<'_> {
        RenderObjectWriter::new(&mut self.render_database)
    }

    pub async fn new(window: Arc<Window>) -> anyhow::Result<Self> {
        let mut producer_registry = RenderProducerRegistry::default();

        let mut standard_mesh_producer = StandardMeshProducer::new(RenderRoute {
            graph_pass: graph_passes::GEOMETRY,
            material_pass: material_passes::FORWARD_OPAQUE,
            phase: phases::OPAQUE,
            views: RenderViewSelector::Main,
        });
        standard_mesh_producer.add_route(RenderRoute {
            graph_pass: graph_passes::SHADOWS,
            material_pass: material_passes::SHADOW,
            phase: phases::OPAQUE,
            views: RenderViewSelector::ShadowCascades,
        });

        producer_registry
            .register(standard_mesh_producer)
            .expect("standard mesh producer ID must be unique");

        Ok(Self {
            renderer_api: Box::new(WgpuBackend::new(window).await?),
            render_resources: RenderResources::new(),
            render_graph: RenderGraph {
                nodes: Vec::new(),
                resources: GraphResources::new(),
                compiled: false,
                disabled_nodes: HashSet::new(),
            },
            pipelines: Vec::new(),
            textures: Vec::new(),
            render_database: RenderDatabase::new(),
            producer_registry,
            view_registry: RenderViewRegistry::default(),
        })
    }

    pub fn init(&mut self) {
        self.renderer_api.compile();
        self.init_frame_bindings();

        let default_meshes = DefaultMeshes::upload(self.renderer_api.as_mut());
        self.render_resources.insert(default_meshes);
        self.render_graph = RenderGraph::default_render_graph(default_meshes);
        self.render_graph
            .compile(&mut self.render_resources, self.renderer_api.as_mut());
        let shadow = *self
            .render_resources
            .get_labeled::<ShadowBindings>("shadow_bindings")
            .expect("the default shadow pass must compile its bindings");
        self.view_registry.set_views(
            graph_passes::SHADOWS,
            vec![RenderView {
                id: RenderViewId::new("engine.shadow_cascade.0"),
                kind: RenderViewKind::ShadowCascade { cascade: 0 },
                view_bind_group: Some(shadow.view_bind_group),
            }],
        );
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.renderer_api.resize(width, height);
    }

    /// Returns GPU handles for the renderer's shared unit primitives.
    pub fn default_meshes(&self) -> &DefaultMeshes {
        self.render_resources
            .get::<DefaultMeshes>()
            .expect("default meshes are unavailable before Renderer::init")
    }

    pub fn prepare(&mut self) {
        crate::profile_scope!("renderer.prepare");
        let camera_upload = self
            .render_resources
            .get_labeled::<FrameBindings>("frame_bindings")
            .and_then(|frame| {
                self.render_resources
                    .get::<CameraData>()
                    .map(|camera| (frame.camera_buffer, camera.uniform))
            });
        if let Some((buffer, uniform)) = camera_upload {
            crate::profile_scope!("renderer.prepare.camera");
            self.renderer_api
                .write_buffer(buffer, bytemuck::bytes_of(&uniform));
        }
        {
            crate::profile_scope!("renderer.prepare.producers");
            self.producer_registry.prepare(
                &self.view_registry,
                &mut self.render_resources,
                self.renderer_api.as_mut(),
            );
        }
        let disabled_nodes = self.render_graph.disabled_nodes.clone();
        for (index, node) in &mut self.render_graph.nodes {
            if disabled_nodes.contains(index) {
                continue;
            }
            crate::profile_dynamic_scope!(
                "render.pass.prepare",
                format!("render.pass.prepare.{}", node.profile_name())
            );
            node.prepare(&mut self.render_resources, self.renderer_api.as_mut());
        }
    }

    pub fn render(&mut self) -> anyhow::Result<()> {
        crate::profile_scope!("renderer.render");
        self.prepare();
        self.renderer_api.render(
            &mut self.render_graph,
            &mut self.render_resources,
            &self.producer_registry,
            &self.view_registry,
        )
    }

    pub fn producers(&mut self) -> &mut RenderProducerRegistry {
        &mut self.producer_registry
    }

    pub fn register_producer(
        &mut self,
        producer: impl crate::renderer::RenderProducer + 'static,
    ) -> Result<(), String> {
        self.producer_registry.register(producer)
    }

    pub fn producer_mut<T: 'static>(
        &mut self,
        id: crate::renderer::RenderProducerId,
    ) -> Option<&mut T> {
        self.producer_registry.get_mut(id)
    }

    pub fn views(&mut self) -> &mut RenderViewRegistry {
        &mut self.view_registry
    }

    pub fn sync_render_database(&mut self, world: &mut World) {
        {
            crate::profile_scope!("render_database.sync_ecs");
            self.render_database.sync_ecs(world);
        }
        let dirty_ranges = {
            crate::profile_scope!("render_database.take_dirty_ranges");
            self.render_database.take_dirty_ranges()
        };
        let revision = self.render_database.structural_revision();
        let needs_rebuild = self
            .producer_registry
            .get_mut::<StandardMeshProducer>(crate::renderer::producers::STANDARD_MESHES)
            .is_some_and(|producer| producer.database_revision != revision);

        if needs_rebuild {
            crate::profile_scope!("render_database.rebuild");
            let ids = self
                .render_database
                .phase_objects(crate::renderer::phases::OPAQUE)
                .to_vec();
            let (camera_layout, textures_layout) = self
                .render_resources
                .get_labeled::<FrameBindings>("frame_bindings")
                .map(|frame| (frame.camera_layout, frame.textures_layout))
                .expect("frame bindings must exist before retained rendering is synchronized");
            let geometry_target = self.renderer_api.target_info_for_pass(
                &GeometryPassNode::pass_descriptor(),
                &self.render_graph.resources,
            );
            let shadow_target = self.renderer_api.target_info_for_pass(
                &ShadowPassNode::pass_descriptor(),
                &self.render_graph.resources,
            );
            let shadow_layout = self
                .render_resources
                .get_labeled::<ShadowBindings>("shadow_bindings")
                .expect("shadow bindings must exist while preparing retained draws")
                .view_layout;
            let mut draws = Vec::with_capacity(ids.len());
            {
                crate::profile_scope!("render_database.rebuild_draws");
                for id in ids {
                    let Some(object) = self.render_database.get(id) else {
                        continue;
                    };
                    let mut pipelines = Vec::with_capacity(2);
                    let forward_pass = material_passes::FORWARD_OPAQUE;
                    if object.flags.contains(RenderFlags::VISIBLE_MAIN)
                        && object.material.supports_pass(forward_pass)
                    {
                        let pipeline =
                            object.pipeline_override(forward_pass).unwrap_or_else(|| {
                                self.renderer_api.create_pipeline(
                                    &object.material,
                                    forward_pass,
                                    &[camera_layout, textures_layout],
                                    &geometry_target,
                                )
                            });
                        pipelines.push(PipelineOverride {
                            material_pass: forward_pass,
                            pipeline,
                        });
                    }

                    let shadow_pass = material_passes::SHADOW;
                    if object.flags.contains(RenderFlags::CASTS_SHADOWS)
                        && object.material.supports_pass(shadow_pass)
                    {
                        let pipeline = object.pipeline_override(shadow_pass).unwrap_or_else(|| {
                            self.renderer_api.create_pipeline(
                                &object.material,
                                shadow_pass,
                                &[shadow_layout, textures_layout],
                                &shadow_target,
                            )
                        });
                        pipelines.push(PipelineOverride {
                            material_pass: shadow_pass,
                            pipeline,
                        });
                    }

                    if pipelines.is_empty() {
                        continue;
                    }
                    draws.push(StandardDraw {
                        mesh: object.mesh,
                        pipelines,
                        transform_index: id.index() as u32,
                        extra_bind_groups: object.extra_bind_groups.clone(),
                    });
                }
            }
            let transforms = {
                crate::profile_scope!("render_database.rebuild_transforms");
                (0..self.render_database.slot_count())
                    .map(|index| self.render_database.gpu_transform_at(index))
                    .collect()
            };
            crate::profile_scope!("render_database.replace_producer_data");
            self.producer_registry
                .get_mut::<StandardMeshProducer>(crate::renderer::producers::STANDARD_MESHES)
                .expect("standard mesh producer must stay registered")
                .replace(draws, transforms, revision);
        } else if !dirty_ranges.is_empty() {
            crate::profile_scope!("render_database.update_transforms");
            let updates = dirty_ranges
                .into_iter()
                .map(|range| {
                    let values = range
                        .clone()
                        .map(|index| self.render_database.gpu_transform_at(index))
                        .collect();
                    (range, values)
                })
                .collect::<Vec<_>>();
            self.producer_registry
                .get_mut::<StandardMeshProducer>(crate::renderer::producers::STANDARD_MESHES)
                .expect("standard mesh producer must stay registered")
                .update_transforms(updates);
        }
    }

    pub fn init_frame_bindings(&mut self) {
        // Camera (group 0)
        let camera_buffer = self.renderer_api.create_buffer(&BufferDescriptor {
            label: "camera_uniform".to_string(),
            size: size_of::<camera::CameraUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST | BufferUsages::VERTEX,
        });

        let camera_layout =
            self.renderer_api
                .create_bind_group_layout(&BindGroupLayoutDescriptor {
                    label: "camera_layout".to_string(),
                    entries: vec![BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::Both,
                        entry_type: BindingType::UniformBuffer,
                        count: None,
                    }],
                });

        let camera_bind_group = self.renderer_api.create_bind_group(&BindGroupDescriptor {
            label: "camera_bind_group".to_string(),
            layout: camera_layout,
            entries: vec![(0, BindGroupEntry::Buffer(camera_buffer))],
        });

        // Global Materials (group 1)
        let textures_layout =
            self.renderer_api
                .create_bind_group_layout(&BindGroupLayoutDescriptor {
                    label: "textures_layout".to_string(),
                    entries: vec![
                        BindGroupLayoutEntry {
                            binding: 0,
                            visibility: ShaderStages::Fragment,
                            entry_type: BindingType::Texture {
                                dimension: TextureDimension::D2,
                                sample_type: TextureSampleType::FloatFilterable,
                                multisampled: false,
                            },
                            count: Some(512),
                        },
                        BindGroupLayoutEntry {
                            binding: 1,
                            visibility: ShaderStages::Fragment,
                            entry_type: BindingType::Sampler,
                            count: None,
                        },
                        BindGroupLayoutEntry {
                            binding: 2,
                            visibility: ShaderStages::Fragment,
                            entry_type: BindingType::StorageBuffer { read_only: true },
                            count: None,
                        },
                    ],
                });

        let white = self.renderer_api.get_white_texture();

        let textures = vec![white; 512];
        let texture_refs: Vec<&TextureHandle> = textures.iter().collect();

        let materials_ssbo_buffer = self.renderer_api.create_buffer(&BufferDescriptor {
            label: "materials_ssbo".to_string(),
            size: 1024 * size_of::<GpuMaterialData>() as u64,
            usage: BufferUsages::COPY_SRC | BufferUsages::COPY_DST | BufferUsages::STORAGE,
        });

        let materials_bind_group = self.renderer_api.create_bind_group(&BindGroupDescriptor {
            label: "materials_bind_group".to_string(),
            layout: textures_layout,
            entries: vec![
                (0, BindGroupEntry::TextureArray(&texture_refs)),
                (
                    1,
                    BindGroupEntry::Sampler(self.renderer_api.get_default_sampler()),
                ),
                (2, BindGroupEntry::Buffer(materials_ssbo_buffer)),
            ],
        });

        self.render_resources.insert_labeled(
            "frame_bindings",
            FrameBindings {
                camera_buffer,
                camera_layout,
                camera_bind_group,

                textures_layout,
                materials_bind_group,

                materials_ssbo_buffer,
            },
        );
        self.view_registry.set_views(
            crate::renderer::graph_passes::GEOMETRY,
            vec![RenderView {
                id: crate::renderer::views::MAIN,
                kind: RenderViewKind::Main,
                view_bind_group: Some(camera_bind_group),
            }],
        );
    }
}

pub struct CameraData {
    pub uniform: camera::CameraUniform,
    pub inverse_projection: [[f32; 4]; 4],
    pub inverse_view: [[f32; 4]; 4],
}

impl CameraData {
    pub fn from_camera(camera: &camera::Camera, uniform: camera::CameraUniform) -> Self {
        let inverse_projection =
            (camera::OPENGL_TO_WGPU_MATRIX * camera.build_projection_matrix()).inverse();
        let inverse_view = camera.build_view_matrix().inverse();

        Self {
            uniform,
            inverse_projection: inverse_projection.to_cols_array_2d(),
            inverse_view: inverse_view.to_cols_array_2d(),
        }
    }
}
