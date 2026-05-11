use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::process::Output;
use std::{fs, option};

use uuid::Uuid;

use crate::Arc;
use crate::InstanceRaw;
use crate::Window;
use crate::assets;
use crate::assets::manager::Handle;
use crate::assets::material::{Material, PipelineDescriptor};
pub use crate::core::camera;
use crate::core::components::core::TransformComponent;
use crate::core::components::physics::RapierRigidBodyHandle;
use crate::core::components::renderer::MeshRendererComponent;
use crate::ecs::commands::Commands;
use crate::ecs::query::Query;
use crate::ecs::world::World;
use crate::engine_info;
use crate::model;
use crate::model::MeshAsset;
use crate::model::VertexLayout;
pub use crate::renderer::backends::*;
use crate::renderer::model::Vertex;
pub use crate::renderer::render_nodes::*;
use crate::renderer::wgpu_backend::WgpuBackend;
use crate::{State, texture};
use wgpu;
use wgpu::util::DeviceExt;

#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq)]
pub struct PipelineHandle(pub u32);
#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq)]
pub struct BufferHandle(pub u32);
#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq)]
pub struct TextureHandle(pub u32);
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
    pub nodes: Vec<(i8, Box<dyn RenderNode>)>,
    pub compiled: bool,
}

pub struct PassResources {
    pub input_textures: Vec<Texture>,
    pub output_textures: Vec<Texture>,
    pub input_buffers: Vec<Buffer>,
    pub output_buffers: Vec<Buffer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureSize {
    FullRes,
    HalfRes,
    QuarterRes,
    Custom { width: u32, height: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureDimension {
    D2,
    D3,
    D2Array,
    Cube,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    pub label: &'static str,
    pub format: TextureFormat,
    pub size: TextureSize,
    pub dimension: TextureDimension,
    pub usage: TextureUsages,
    pub mip_levels: u32,
    pub sample_count: u32,
}

pub struct BufferDescriptor {
    pub label: &'static str,
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
        multisampled: bool,
    },
    Sampler,
}

#[derive(Debug, Clone, Copy)]
pub enum BindGroupEntry {
    Buffer(BufferHandle),
    Texture(TextureHandle),
}

pub struct BindGroupLayoutEntry {
    pub binding: u32,
    pub visibility: ShaderStages,
    pub entry_type: BindingType,
}

pub struct BindGroupLayoutDescriptor {
    pub label: String,
    pub entries: Vec<BindGroupLayoutEntry>,
}

pub struct BindGroupDescriptor {
    pub label: String,
    pub layout: BindGroupLayoutHandle,
    pub entries: Vec<(u32, BindGroupEntry)>,
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
    pub input_textures: &'static [&'static str],
    pub output_textures: &'static [OutputTexture],
    pub input_buffers: &'static [&'static str],
    pub output_buffers: &'static [OutputBuffer],
}

pub enum OutputTexture {
    Create(TextureSlot),   // I create this resource (has format, size, etc.)
    WriteTo(&'static str), // I write to an existing resource (name only)
}

pub enum OutputBuffer {
    Create(BufferSlot),    // I create this resource (has format, size, etc.)
    WriteTo(&'static str), // I write to an existing resource (name only)
}

pub trait RenderNode {
    fn describe(&self) -> RenderNodeDescriptor;
    fn compile(&mut self, ctx: &mut NodeCompileContext);
    fn prepare(&mut self, resources: &mut RenderResources, api: &mut dyn RendererAPI);
    fn run(&mut self, ctx: &mut dyn RenderContext);
    fn should_render_to_swapchain(&self) -> bool;
    fn needs_depth(&self) -> bool {
        true
    }
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

#[derive(Clone)]
pub struct RenderData {
    pub mesh: Handle<MeshAsset>,
    pub material: Material,
    pub transform_index: u32, // index into a GPU-side transform buffer
    pub sort_key: u64,        // for draw call sorting/batching
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
    pub color_format: TextureFormat, // from pass
    pub depth_format: TextureFormat, // from pass
}

impl PipelineKey {
    pub fn from_material_and_pass(
        material: &Material,
        _render_node: &dyn RenderNode,
    ) -> PipelineKey {
        let desc = &material.pipeline_descriptor;
        PipelineKey {
            shader: desc.shader.clone(),
            blend_mode: desc.blend_mode,
            cull_mode: desc.cull_mode,
            topology: desc.topology,
            front_face: desc.front_face,
            polygon_mode: desc.polygon_mode,
            depth_state: desc.depth_state,
            multisample_count: desc.multisample.count,
            vertex_layouts: desc.vertex_layouts.clone(),
            color_format: TextureFormat::None,
            depth_format: TextureFormat::None,
        }
    }
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
    pub async fn new(window: Arc<Window>) -> anyhow::Result<Self> {
        Ok(Self {
            renderer_api: Box::new(WgpuBackend::new(window).await?),
            render_resources: RenderResources::new(),
            render_graph: RenderGraph {
                nodes: Vec::new(),
                compiled: false,
            },
            pipelines: Vec::new(),
            textures: Vec::new(),
        })
    }

    pub fn init(&mut self) {
        self.renderer_api.compile();
        self.render_graph = RenderGraph::default_render_graph(self.renderer_api.as_mut());
        self.render_graph
            .compile(&mut self.render_resources, self.renderer_api.as_mut());
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.renderer_api.resize(width, height);
    }

    pub fn prepare(&mut self) {
        for (_, node) in &mut self.render_graph.nodes {
            node.prepare(&mut self.render_resources, self.renderer_api.as_mut());
        }
    }

    pub fn render(&mut self) -> anyhow::Result<()> {
        self.prepare();
        self.renderer_api.render(&mut self.render_graph)
    }
}

pub struct CameraData {
    pub uniform: camera::CameraUniform,
}
pub struct GraphResources {
    textures: HashMap<&'static str, TextureHandle>,
    buffers: HashMap<&'static str, BufferHandle>,
}

impl GraphResources {
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
            .map(|output| {
                let name = match output {
                    OutputTexture::Create(slot) => slot.name,
                    OutputTexture::WriteTo(slot_name) => slot_name,
                };
                (name, self.textures[name])
            })
            .collect()
    }
}

impl RenderGraph {
    pub fn default_render_graph(renderer_api: &mut dyn RendererAPI) -> Self {
        let mut graph = RenderGraph {
            nodes: Vec::new(),
            compiled: false,
        };

        let geometry_pass_node = GeometryPassNode {
            render_data: Vec::new(),
            camera_buffer: None,
            camera_bind_group: None,
            camera_bind_group_layout: None,
            pass_inputs_group: None,
        };
        graph.nodes.push((0, Box::new(geometry_pass_node)));

        let meshe = MeshAsset {
            name: "eae".to_string(),
            uuid: Uuid::new_v4(),
            vertices: Vec::new(),
            indices: Vec::new(),
            vertex_layout: VertexLayout {
                stride: std::mem::size_of::<[f32; 3]>() as u64,
                step_mode: model::StepMode::Vertex,
                attributes: Vec::new(),
            },
        };
        let positions = vec![
            cgmath::Point3::new(-0.5, -0.5, -0.5),
            cgmath::Point3::new(0.5, -0.5, -0.5),
            cgmath::Point3::new(0.5, 0.5, -0.5),
            cgmath::Point3::new(-0.5, 0.5, -0.5),
            cgmath::Point3::new(-0.5, -0.5, 0.5),
            cgmath::Point3::new(0.5, -0.5, 0.5),
            cgmath::Point3::new(0.5, 0.5, 0.5),
            cgmath::Point3::new(-0.5, 0.5, 0.5),
        ];

        let indices: Vec<u32> = vec![
            4, 5, 6, 4, 6, 7, // front  (+z)
            1, 0, 3, 1, 3, 2, // back   (-z)
            5, 1, 2, 5, 2, 6, // right  (+x)
            0, 4, 7, 0, 7, 3, // left   (-x)
            3, 7, 6, 3, 6, 2, // top    (+y)
            0, 1, 5, 0, 5, 4, // bottom (-y)
        ];
        //meshe.indices = indices;

        let positions_raw: Vec<[f32; 3]> = positions.iter().map(|p| [p.x, p.y, p.z]).collect();

        let vertex_bytes: Vec<u8> = bytemuck::cast_slice(&positions_raw).to_vec();
        //let index_bytes: Vec<u8> = bytemuck::cast_slice(&indices).to_vec();

        let mesh = MeshAsset {
            name: "Cube".to_string(),
            uuid: Uuid::new_v4(),
            vertices: vertex_bytes.clone(),
            indices: bytemuck::cast_slice(&indices).to_vec(),
            vertex_layout: VertexLayout {
                stride: std::mem::size_of::<[f32; 3]>() as u64,
                step_mode: model::StepMode::Vertex,
                attributes: Vec::new(),
            },
        };

        let cube_mesh = renderer_api.upload_mesh(&mesh);

        let sphere_latitudes = 12u32;
        let sphere_longitudes = 24u32;
        let mut sphere_positions: Vec<[f32; 3]> = Vec::new();
        let mut sphere_indices: Vec<u32> = Vec::new();

        for lat in 0..=sphere_latitudes {
            let theta = std::f32::consts::PI * lat as f32 / sphere_latitudes as f32;
            let y = theta.cos() * 0.5;
            let ring_radius = theta.sin() * 0.5;

            for lon in 0..=sphere_longitudes {
                let phi = std::f32::consts::TAU * lon as f32 / sphere_longitudes as f32;
                sphere_positions.push([ring_radius * phi.cos(), y, ring_radius * phi.sin()]);
            }
        }

        for lat in 0..sphere_latitudes {
            for lon in 0..sphere_longitudes {
                let row = sphere_longitudes + 1;
                let i0 = lat * row + lon;
                let i1 = i0 + 1;
                let i2 = i0 + row;
                let i3 = i2 + 1;

                sphere_indices.extend_from_slice(&[i0, i2, i1]);
                sphere_indices.extend_from_slice(&[i1, i2, i3]);
            }
        }

        let sphere_vertex_bytes: Vec<u8> = bytemuck::cast_slice(&sphere_positions).to_vec();
        let sphere_asset = MeshAsset {
            name: "DebugSphere".to_string(),
            uuid: Uuid::new_v4(),
            vertices: sphere_vertex_bytes,
            indices: bytemuck::cast_slice(&sphere_indices).to_vec(),
            vertex_layout: VertexLayout {
                stride: std::mem::size_of::<[f32; 3]>() as u64,
                step_mode: model::StepMode::Vertex,
                attributes: Vec::new(),
            },
        };
        let sphere_mesh = renderer_api.upload_mesh(&sphere_asset);

        // Wire cube mesh (same vertices, line-list indices for 12 edges)
        let wire_indices: Vec<u32> = vec![
            0, 1, 1, 2, 2, 3, 3, 0, // back face edges
            4, 5, 5, 6, 6, 7, 7, 4, // front face edges
            0, 4, 1, 5, 2, 6, 3, 7, // connecting edges
        ];
        let wire_mesh = MeshAsset {
            name: "WireCube".to_string(),
            uuid: Uuid::new_v4(),
            vertices: vertex_bytes,
            indices: bytemuck::cast_slice(&wire_indices).to_vec(),
            vertex_layout: VertexLayout {
                stride: std::mem::size_of::<[f32; 3]>() as u64,
                step_mode: model::StepMode::Vertex,
                attributes: Vec::new(),
            },
        };
        let wire_cube_mesh = renderer_api.upload_mesh(&wire_mesh);

        let vertex_layout = VertexLayout {
            stride: std::mem::size_of::<[f32; 3]>() as u64,
            step_mode: model::StepMode::Vertex,
            attributes: vec![model::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: model::AttributeFormat::Float32x3,
            }],
        };

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

        let mut sphere_material = Material::new("shaders/debug.wgsl".to_string())
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
            sphere_mesh,
            sphere_material,
            cube_mesh,
            cube_material,
            wire_cube_mesh,
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

        graph.nodes.push((1, Box::new(debug_pass_node)));

        graph
    }

    fn allocate_graph_resources(
        nodes: &Vec<(i8, Box<dyn RenderNode>)>,
        api: &mut dyn RendererAPI,
    ) -> GraphResources {
        let mut textures = HashMap::new();
        let mut buffers = HashMap::new();

        for (_, node) in nodes {
            for slot in node.describe().output_textures {
                match slot {
                    OutputTexture::Create(slot) => {
                        textures.insert(slot.name, api.create_texture(&slot.texture_descriptor));
                    }
                    _ => {}
                }
            }

            for slot in node.describe().output_buffers {
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
        let allocated = RenderGraph::allocate_graph_resources(&self.nodes, api); // textures for all declared outputs

        for (_, node) in &mut self.nodes {
            let desc = node.describe();
            let mut ctx = NodeCompileContext {
                api,
                render_resources,
                resolved_inputs: allocated.resolve_inputs(&desc),
                resolved_outputs: allocated.resolve_outputs(&desc),
            };
            node.compile(&mut ctx);
        }
        self.compiled = true;
    }

    pub fn get_node_mut<T: 'static>(&mut self, index: i8) -> Option<&mut T> {
        for (node_index, node) in &mut self.nodes {
            if *node_index == index {
                return node.as_any_mut().downcast_mut::<T>();
            }
        }
        None
    }

    /// Remove a node from the graph, returning it as an owned Box.
    /// Use `return_node` to put it back after you're done.
    /// This is useful to avoid borrow conflicts when the node needs
    /// mutable access to State while being part of State.
    pub fn take_node(&mut self, index: i8) -> Option<Box<dyn RenderNode>> {
        if let Some(pos) = self.nodes.iter().position(|(i, _)| *i == index) {
            Some(self.nodes.remove(pos).1)
        } else {
            None
        }
    }

    /// Return a previously taken node back into the graph.
    pub fn return_node(&mut self, index: i8, node: Box<dyn RenderNode>) {
        self.nodes.push((index, node));
        self.nodes.sort_by_key(|(i, _)| *i);
    }
}

impl dyn RenderNode {}

pub struct GeometryRenderQueue {
    pub items: Vec<RenderData>,
}

impl GeometryRenderQueue {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }
}

pub fn get_render_data_system(world: &mut World, commands: &mut Commands) {
    let render_data = {
        let mut items = Vec::new();

        let mut query = Query::<(&MeshRendererComponent, &TransformComponent)>::new(world);
        query.for_each(|_entity, (mesh_renderer, transform)| {
            items.push(RenderData {
                mesh: mesh_renderer.mesh,
                material: mesh_renderer.material.clone(),
                transform_index: 0,
                sort_key: 0,
            });
        });

        items
    };

    let Some(mut queue) = world.get_resource_mut::<GeometryRenderQueue>() else {
        return;
    };

    queue.items.clear();
    queue.items.extend(render_data);
}
