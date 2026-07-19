use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};

use crate::renderer::ids::{GraphPassId, MaterialPassId, RenderViewId};
use crate::renderer::{
    DefaultMeshes, PipelineOverride, RenderDatabase, RenderFlags, RenderObjectWriter,
    RenderProducerRegistry, RenderRoute, RenderView, RenderViewKind, RenderViewRegistry,
    RenderViewSelector, StandardDraw, StandardMeshProducer, graph_passes, material_passes, phases,
};
use bytemuck::{Pod, Zeroable};

use crate::Arc;
use crate::Window;
use crate::assets::manager::Handle;
use crate::assets::material::Material;
pub use crate::core::camera;
use crate::ecs::world::World;
use crate::model;
use crate::model::MeshAsset;
use crate::model::Vertex;
use crate::model::VertexLayout;
pub use crate::renderer::backends::*;
pub use crate::renderer::render_nodes::*;
use crate::renderer::wgpu_backend::WgpuBackend;
use crate::texture;
use wgpu;

pub use engine_inspector_derive::Inspector;

#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq)]
pub struct PipelineHandle(pub u32);
#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq)]
pub struct BufferHandle(pub u32);
#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq)]
pub struct TextureHandle(pub u32);
#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq)]
pub struct SamplerHandle(pub u32);
#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq)]
pub struct BindGroupHandle(pub u32);
#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq)]
pub struct BindGroupLayoutHandle(pub u32);
#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq)]
pub struct RenderPassHandle(pub u32);

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
    id: GraphPassId,
    position: usize,
    node: Box<dyn RenderNode>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TextureSize {
    FullRes,
    HalfRes,
    QuarterRes,
    Custom { width: u32, height: u32 },
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum TextureDimension {
    #[default]
    D2,
    D3,
    D2Array,
    Cube,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum AddressMode {
    #[default]
    ClampToEdge = 0,
    Repeat = 1,
    MirrorRepeat = 2,
    ClampToBorder = 3,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum FilterMode {
    Nearest = 0,
    #[default]
    Linear = 1,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum SamplerBorderColor {
    #[default]
    TransparentBlack,
    OpaqueBlack,
    OpaqueWhite,
    Zero,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BufferUsages(u32);

impl BufferUsages {
    pub const MAP_READ: Self = Self(1 << 0);
    pub const MAP_WRITE: Self = Self(1 << 1);
    pub const COPY_SRC: Self = Self(1 << 2);
    pub const COPY_DST: Self = Self(1 << 3);
    pub const INDEX: Self = Self(1 << 4);
    pub const VERTEX: Self = Self(1 << 5);
    pub const UNIFORM: Self = Self(1 << 6);
    pub const STORAGE: Self = Self(1 << 7);
    pub const INDIRECT: Self = Self(1 << 8);
    pub const QUERY_RESOLVE: Self = Self(1 << 9);

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    pub const fn bits(self) -> u32 {
        self.0
    }
}

impl std::ops::BitOr for BufferUsages {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for BufferUsages {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureUsages(u32);

impl TextureUsages {
    pub const COPY_SRC: Self = Self(1 << 0);
    pub const COPY_DST: Self = Self(1 << 1);
    pub const TEXTURE_BINDING: Self = Self(1 << 2);
    pub const STORAGE_BINDING: Self = Self(1 << 3);
    pub const RENDER_ATTACHMENT: Self = Self(1 << 4);

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}

impl std::ops::BitOr for TextureUsages {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for TextureUsages {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

pub struct TextureDescriptor {
    pub label: String,
    pub format: TextureFormat,
    pub size: TextureSize,
    pub dimension: TextureDimension,
    pub usage: TextureUsages,
    pub mip_levels: u32,
    pub sample_count: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SamplerDescriptor {
    pub label: String,
    pub address_mode_u: AddressMode,
    pub address_mode_v: AddressMode,
    pub address_mode_w: AddressMode,
    pub mag_filter: FilterMode,
    pub min_filter: FilterMode,
    pub mipmap_filter: FilterMode,
    pub lod_min_clamp: f32,
    pub lod_max_clamp: f32,
    pub compare: Option<CompareFunction>,
    pub anisotropy_clamp: u16,
    pub border_color: Option<SamplerBorderColor>,
}

pub struct BufferDescriptor {
    pub label: String,
    pub size: u64,
    pub usage: BufferUsages,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderStages {
    Vertex,
    Fragment,
    Both,
    Compute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingType {
    UniformBuffer,
    StorageBuffer {
        read_only: bool,
    },
    Texture {
        dimension: TextureDimension,
        sample_type: TextureSampleType,
        multisampled: bool,
    },
    Sampler,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureSampleType {
    FloatFilterable,
    FloatUnfilterable,
    Depth,
    Uint,
    Sint,
}

#[derive(Debug, Clone, Copy)]
pub enum BindGroupEntry<'a> {
    Buffer(BufferHandle),
    Texture(TextureHandle),
    Sampler(SamplerHandle),
    TextureArray(&'a [&'a TextureHandle]),
}

pub struct BindGroupLayoutEntry {
    pub binding: u32,
    pub visibility: ShaderStages,
    pub entry_type: BindingType,
    pub count: Option<u32>,
}

pub struct BindGroupLayoutDescriptor {
    pub label: String,
    pub entries: Vec<BindGroupLayoutEntry>,
}

pub struct BindGroupDescriptor<'a> {
    pub label: String,
    pub layout: BindGroupLayoutHandle,
    pub entries: Vec<(u32, BindGroupEntry<'a>)>,
}

pub struct TextureSlot {
    pub name: &'static str,
    pub texture_descriptor: TextureDescriptor,
}

pub struct BufferSlot {
    pub name: &'static str,
    pub buffer_descriptor: BufferDescriptor,
}

pub struct RenderNodeDescriptor {
    pub name: &'static str,
    pub color_attachments: Vec<ColorAttachmentDescriptor>,
    pub depth_attachment: Option<DepthAttachmentDescriptor>,
    pub input_textures: Vec<&'static str>,
    pub output_textures: Vec<OutputTexture>,
    pub input_buffers: Vec<&'static str>,
    pub output_buffers: Vec<OutputBuffer>,
}

pub enum OutputTexture {
    Create(TextureSlot),   // I create this resource (has format, size, etc.)
    WriteTo(&'static str), // I write to an existing resource (name only)
}

pub enum OutputBuffer {
    Create(BufferSlot),    // I create this resource (has format, size, etc.)
    WriteTo(&'static str), // I write to an existing resource (name only)
}

#[derive(Debug, Clone, Copy)]
pub struct ColorAttachmentDescriptor {
    pub name: &'static str,
    pub load_op: AttachmentLoadOp,
    pub store: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct DepthAttachmentDescriptor {
    pub name: &'static str,
    pub load_op: AttachmentLoadOp,
    pub store: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum AttachmentLoadOp {
    Load,
    ClearColor([f64; 4]),
    ClearDepth(f32),
}

pub trait Inspector {
    fn inspect(&mut self, visitor: &mut dyn InspectorVisitor);
}

pub trait InspectorVisitor {
    fn field_f32(&mut self, name: &'static str, value: &mut f32);
    fn field_i32(&mut self, name: &'static str, value: &mut i32);
    fn field_u32(&mut self, name: &'static str, value: &mut u32);
    fn field_bool(&mut self, name: &'static str, value: &mut bool);
    fn field_f32_array(&mut self, name: &'static str, value: &mut [f32]);
}

pub trait RenderNode {
    fn describe_pass(&self) -> RenderNodeDescriptor;
    fn compile(&mut self, ctx: &mut NodeCompileContext);
    fn prepare(&mut self, resources: &mut RenderResources, api: &mut dyn RendererAPI);
    fn run(&mut self, ctx: &mut dyn RenderContext, render_resources: &RenderResources);
    fn should_render_to_swapchain(&self) -> bool;
    fn needs_depth(&self) -> bool {
        true
    }
    fn inspect(&mut self, _visitor: &mut dyn InspectorVisitor) -> bool {
        false
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

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BlendMode {
    None,
    Alpha,
    Additive,
    Replace,
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CullMode {
    None,
    Front,
    Back,
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Topology {
    TriangleList,
    TriangleStrip,
    LineList,
    LineStrip,
    PointList,
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FrontFace {
    Ccw,
    Cw,
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PolygonMode {
    Fill,
    Line,
    Point,
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CompareFunction {
    Never,
    Less,
    Equal,
    LessEqual,
    Greater,
    NotEqual,
    GreaterEqual,
    Always,
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DepthState {
    pub write_enabled: bool,
    pub compare: CompareFunction,
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MultisampleState {
    pub count: u32,
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TextureFormat {
    None,
    Depth32Float,
    Depth24PlusStencil8,
    Depth24Plus,
    Depth16Unorm,
    Depth32FloatStencil8,
    Depth32Stencil8,
    Stencil8,
    Rgba8Unorm,
    Rgba8UnormSrgb,
    Rgba16Float,
    Rgba32Float,
    Rgba8Snorm,
    Rgba16Snorm,
    Rgba8Uint,
    Rgba8Sint,
    Rgba16Uint,
    Rgba16Sint,
    Rgba32Uint,
    Rgba32Sint,
    Rg32Float,
    Rg32Uint,
    Rg32Sint,
    Rg16Float,
    Rg16Uint,
    Rg16Sint,
    Rg8Unorm,
    Rg8Snorm,
    Rg8Uint,
    Rg8Sint,
    R32Float,
    R32Uint,
    R32Sint,
    R16Float,
    R16Uint,
    R16Sint,
    R8Unorm,
    R8Snorm,
    R8Uint,
    R8Sint,
    Bgra8Unorm,
    Rgba8Srgb,
    Bgra8UnormSrgb,
    Rgb10a2Unorm,
    Rgb10a2Uint,
    Rg11b10Float,
    Rgb9e5Ufloat,
    Bc1RgbaUnorm,
    Bc1RgbaUnormSrgb,
    Bc2RgbaUnorm,
    Bc2RgbaUnormSrgb,
    Bc3RgbaUnorm,
    Bc3RgbaUnormSrgb,
    Bc4RUnorm,
    Bc4RSnorm,
    Bc5RgUnorm,
    Bc5RgSnorm,
    Bc6hRgbUfloat,
    Bc6hRgbFloat,
    Bc7RgbaUnorm,
    Bc7RgbaUnormSrgb,
    Etc2Rgb8Unorm,
    Etc2Rgb8UnormSrgb,
    Etc2Rgb8A1Unorm,
    Etc2Rgb8A1UnormSrgb,
    Etc2Rgba8Unorm,
    Etc2Rgba8UnormSrgb,
    EacR11Unorm,
    EacR11Snorm,
    EacRg11Unorm,
    EacRg11Snorm,
}

impl TextureFormat {
    pub fn is_depth(&self) -> bool {
        match self {
            TextureFormat::Depth16Unorm
            | TextureFormat::Depth24Plus
            | TextureFormat::Depth24PlusStencil8
            | TextureFormat::Depth32Float
            | TextureFormat::Depth32FloatStencil8
            | TextureFormat::Depth32Stencil8
            | TextureFormat::Stencil8 => true,
            _ => false,
        }
    }
}

#[derive(Hash, Eq, PartialEq)]
pub struct PipelineKey {
    pub shader: String,
    pub blend_mode: BlendMode,
    pub cull_mode: CullMode,
    pub topology: Topology,
    pub front_face: FrontFace,
    pub polygon_mode: PolygonMode,
    pub depth_state: Option<DepthState>,
    pub multisample_count: u32,
    pub vertex_layouts: Vec<VertexLayout>,
    pub color_formats: Vec<TextureFormat>,   // from pass
    pub depth_format: Option<TextureFormat>, // from pass
    pub material_pass: MaterialPassId,
    pub vertex_entry: String,
    pub fragment_entry: Option<String>,
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct PipelineTargetInfo {
    pub color_formats: Vec<TextureFormat>,
    pub depth_format: Option<TextureFormat>,
    pub sample_count: u32,
}

impl PipelineKey {
    pub fn from_material_and_pass(
        material: &Material,
        material_pass: MaterialPassId,
        _render_node: &dyn RenderNode,
    ) -> PipelineKey {
        let shader_pass = material.require_pass(material_pass);
        let desc = &shader_pass.pipeline;
        PipelineKey {
            material_pass,
            shader: desc.shader.clone(),
            vertex_entry: shader_pass.vertex_entry.clone(),
            fragment_entry: shader_pass.fragment_entry.clone(),
            blend_mode: desc.blend_mode,
            cull_mode: desc.cull_mode,
            topology: desc.topology,
            front_face: desc.front_face,
            polygon_mode: desc.polygon_mode,
            depth_state: desc.depth_state,
            multisample_count: desc.multisample.count,
            vertex_layouts: desc.vertex_layouts.clone(),
            color_formats: Vec::new(),
            depth_format: None,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct GpuMaterialData {
    pub diffuse_texture_index: u32,
    pub normal_texture_index: u32,
    pub roughness_texture_index: u32,
    pub flags: u32,
    pub base_color: [f32; 4],
}

impl Default for GpuMaterialData {
    fn default() -> Self {
        Self {
            diffuse_texture_index: 0,
            normal_texture_index: 0,
            roughness_texture_index: 0,
            flags: 0,
            base_color: [1.0, 1.0, 1.0, 1.0],
        }
    }
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
            self.renderer_api
                .write_buffer(buffer, bytemuck::bytes_of(&uniform));
        }
        self.producer_registry.prepare(
            &self.view_registry,
            &mut self.render_resources,
            self.renderer_api.as_mut(),
        );
        let disabled_nodes = self.render_graph.disabled_nodes.clone();
        for (index, node) in &mut self.render_graph.nodes {
            if disabled_nodes.contains(index) {
                continue;
            }
            let _profile_scope =
                crate::profiling::Scope::new_owned(format!("render_node.prepare.{index}"));
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

    pub fn sync_render_database(
        &mut self,
        world: &mut World,
        assets: &crate::assets::manager::AssetManager,
    ) {
        self.render_database.sync_ecs(world, assets);
        let dirty_ranges = self.render_database.take_dirty_ranges();
        let revision = self.render_database.structural_revision();
        let needs_rebuild = self
            .producer_registry
            .get_mut::<StandardMeshProducer>(crate::renderer::producers::STANDARD_MESHES)
            .is_some_and(|producer| producer.database_revision != revision);

        if needs_rebuild {
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
            for id in ids {
                let Some(object) = self.render_database.get(id) else {
                    continue;
                };
                let mut pipelines = Vec::with_capacity(2);
                let forward_pass = material_passes::FORWARD_OPAQUE;
                if object.flags.contains(RenderFlags::VISIBLE_MAIN)
                    && object.material.supports_pass(forward_pass)
                {
                    let pipeline = object.pipeline_override(forward_pass).unwrap_or_else(|| {
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
            let transforms = (0..self.render_database.slot_count())
                .map(|index| self.render_database.gpu_transform_at(index))
                .collect();
            self.producer_registry
                .get_mut::<StandardMeshProducer>(crate::renderer::producers::STANDARD_MESHES)
                .expect("standard mesh producer must stay registered")
                .replace(draws, transforms, revision);
        } else if !dirty_ranges.is_empty() {
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
pub struct GraphResources {
    textures: HashMap<&'static str, TextureHandle>,
    buffers: HashMap<&'static str, BufferHandle>,
}

impl GraphResources {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
            buffers: HashMap::new(),
        }
    }
    pub fn resolve_inputs(
        &self,
        desc: &RenderNodeDescriptor,
    ) -> HashMap<&'static str, TextureHandle> {
        desc.input_textures
            .iter()
            .map(|&name| (name, self.textures[name]))
            .collect()
    }

    pub fn resolve_outputs(
        &self,
        desc: &RenderNodeDescriptor,
    ) -> HashMap<&'static str, TextureHandle> {
        desc.output_textures
            .iter()
            .filter_map(|output| {
                let name = match output {
                    OutputTexture::Create(slot) => slot.name,
                    OutputTexture::WriteTo(slot_name) => slot_name,
                };

                self.textures.get(name).map(|texture| (name, *texture))
            })
            .collect()
    }

    pub fn texture(&self, name: &str) -> Option<&TextureHandle> {
        self.textures.get(name)
    }

    pub fn texture_mut(&mut self, name: &str) -> Option<&mut TextureHandle> {
        self.textures.get_mut(name)
    }

    pub fn buffer(&self, name: &str) -> Option<&BufferHandle> {
        self.buffers.get(name)
    }

    pub fn buffer_mut(&mut self, name: &str) -> Option<&mut BufferHandle> {
        self.buffers.get_mut(name)
    }
}

impl RenderGraph {
    pub fn default_render_graph(default_meshes: DefaultMeshes) -> Self {
        let mut graph = RenderGraph {
            nodes: Vec::new(),
            resources: GraphResources::new(),
            compiled: false,
            disabled_nodes: HashSet::new(),
        };

        graph
            .nodes
            .push((graph_passes::SHADOWS, Box::new(ShadowPassNode::new())));

        let geometry_pass_node = GeometryPassNode {
            camera_bind_group: None,
            camera_bind_group_layout: None,
            pass_inputs_group: None,
        };
        graph.nodes.push((
            crate::renderer::ids::graph_passes::GEOMETRY,
            Box::new(geometry_pass_node),
        ));

        graph.nodes.push((
            crate::renderer::ids::graph_passes::ATMOSPHERE,
            Box::new(AtmospherePassNode::new()),
        ));

        RenderGraph::default_debug_nodes(&mut graph, default_meshes);

        graph
    }

    pub fn default_debug_nodes(graph: &mut RenderGraph, default_meshes: DefaultMeshes) {
        let vertex_layout = model::ModelVertex::layout();

        let instance_layout = VertexLayout {
            stride: std::mem::size_of::<[[f32; 4]; 5]>() as u64,
            step_mode: model::StepMode::Instance,
            attributes: vec![
                model::VertexAttribute {
                    offset: 0,
                    shader_location: 5,
                    format: model::AttributeFormat::Float32x4,
                },
                model::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 4]>() as u64,
                    shader_location: 6,
                    format: model::AttributeFormat::Float32x4,
                },
                model::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 8]>() as u64,
                    shader_location: 7,
                    format: model::AttributeFormat::Float32x4,
                },
                model::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 12]>() as u64,
                    shader_location: 8,
                    format: model::AttributeFormat::Float32x4,
                },
                model::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 16]>() as u64,
                    shader_location: 9,
                    format: model::AttributeFormat::Float32x4,
                },
            ],
        };

        let sphere_material = Material::new("shaders/debug.wgsl".to_string())
            .with_vertex_layouts(vec![vertex_layout.clone(), instance_layout.clone()]);
        let cube_material = sphere_material.clone();
        let wire_cube_material = Material::new("shaders/debug.wgsl".to_string())
            .with_vertex_layouts(vec![vertex_layout.clone(), instance_layout.clone()])
            .with_topology(Topology::LineList);

        let debug_pass_node = DebugPassNode {
            camera_buffer: None,
            camera_bind_group: None,
            camera_bind_group_layout: None,
            pass_inputs_group: None,
            cubes: Vec::new(),
            wire_cubes: Vec::new(),
            sphere_positions: Vec::new(),
            sphere_mesh: default_meshes.sphere,
            sphere_material,
            cube_mesh: default_meshes.cube,
            cube_material,
            wire_cube_mesh: default_meshes.wire_cube,
            wire_cube_material,
            sphere_instance_buffer: None,
            cube_instance_buffer: None,
            wire_cube_instance_buffer: None,
            sphere_instance_capacity: 0,
            cube_instance_capacity: 0,
            wire_cube_instance_capacity: 0,
            sphere_instance_count: 0,
            cube_instance_count: 0,
            wire_cube_instance_count: 0,
        };

        graph.nodes.push((
            crate::renderer::ids::graph_passes::DEBUG,
            Box::new(debug_pass_node),
        ));
    }

    fn allocate_graph_resources(
        nodes: &Vec<(GraphPassId, Box<dyn RenderNode>)>,
        api: &mut dyn RendererAPI,
    ) -> GraphResources {
        let mut textures = HashMap::new();
        let mut buffers = HashMap::new();

        for (_, node) in nodes {
            for slot in node.describe_pass().output_textures {
                match slot {
                    OutputTexture::Create(slot) => {
                        textures.insert(slot.name, api.create_texture(&slot.texture_descriptor));
                    }
                    _ => {}
                }
            }

            for slot in node.describe_pass().output_buffers {
                match slot {
                    OutputBuffer::Create(slot) => {
                        buffers.insert(slot.name, api.create_buffer(&slot.buffer_descriptor));
                    }
                    _ => {}
                }
            }
        }

        GraphResources { textures, buffers }
    }

    pub fn compile(&mut self, render_resources: &mut RenderResources, api: &mut dyn RendererAPI) {
        self.resources = RenderGraph::allocate_graph_resources(&self.nodes, api); // textures for all declared outputs

        for (_, node) in &mut self.nodes {
            let desc = node.describe_pass();
            let target_info = api.target_info_for_pass(&desc, &self.resources);
            let mut ctx = NodeCompileContext {
                api,
                render_resources,
                resolved_inputs: self.resources.resolve_inputs(&desc),
                resolved_outputs: self.resources.resolve_outputs(&desc),
                target_info,
            };
            node.compile(&mut ctx);
        }
        self.compiled = true;
    }

    pub fn resize(
        &mut self,
        api: &mut dyn RendererAPI,
        render_resources: &mut RenderResources,
        width: u32,
        height: u32,
    ) {
        for (_, node) in &mut self.nodes {
            let desc = node.describe_pass();
            let target_info = api.target_info_for_pass(&desc, &self.resources);
            let mut ctx = NodeCompileContext {
                api,
                render_resources,
                resolved_inputs: self.resources.resolve_inputs(&desc),
                resolved_outputs: self.resources.resolve_outputs(&desc),
                target_info,
            };

            node.resize(&mut ctx, &self.resources, width, height);
        }
    }

    pub fn get_node_mut<T: 'static>(&mut self, index: GraphPassId) -> Option<&mut T> {
        for (node_index, node) in &mut self.nodes {
            if *node_index == index {
                return node.as_any_mut().downcast_mut::<T>();
            }
        }
        None
    }

    /// Remove a node from the graph while remembering its execution position.
    /// Use `return_node` to put it back after you're done.
    /// This is useful to avoid borrow conflicts when the node needs
    /// mutable access to State while being part of State.
    pub fn take_node(&mut self, index: GraphPassId) -> Option<TakenRenderNode> {
        if let Some(pos) = self.nodes.iter().position(|(i, _)| *i == index) {
            let (id, node) = self.nodes.remove(pos);
            Some(TakenRenderNode {
                id,
                position: pos,
                node,
            })
        } else {
            None
        }
    }

    /// Return a previously taken node to its original execution position.
    pub fn return_node(&mut self, taken: TakenRenderNode) {
        let position = taken.position.min(self.nodes.len());
        self.nodes.insert(position, (taken.id, taken.node));
    }

    pub fn is_node_enabled(&self, index: GraphPassId) -> bool {
        !self.disabled_nodes.contains(&index)
    }

    pub fn set_node_enabled(&mut self, index: GraphPassId, enabled: bool) {
        if enabled {
            self.disabled_nodes.remove(&index);
        } else {
            self.disabled_nodes.insert(index);
        }
    }
}

impl dyn RenderNode {}
