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
use crate::assets::material::Material;
pub use crate::core::camera;
use crate::engine_info;
use crate::model;
use crate::model::MeshAsset;
use crate::model::VertexLayout;
pub use crate::renderer::backends::*;
use crate::renderer::model::Vertex;
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
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

pub struct RenderData {
    pub mesh: Handle<MeshAsset>,
    pub material: Material,
    pub transform_index: u32, // index into a GPU-side transform buffer
    pub sort_key: u64,        // for draw call sorting/batching
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BlendMode {
    Blend,
    NoBlend,
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CullMode {
    None,
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
    pub shader: String,              // from material
    pub blend_state: BlendMode,      // from material
    pub cull_mode: CullMode,         // from material
    pub vertex_layout: VertexLayout, // from mesh/material
    pub color_format: TextureFormat, // from pass
    pub depth_format: TextureFormat, // from pass
    pub sample_count: u32,           // from pass
}

impl PipelineKey {
    pub fn from_material_and_pass(
        material: &Material,
        render_node: &dyn RenderNode,
    ) -> PipelineKey {
        PipelineKey {
            shader: material.pipeline_descriptor.shader.clone(),
            blend_state: material.blend_state,
            cull_mode: material.cull_mode,
            vertex_layout: material.vertex_layout.clone(),
            color_format: TextureFormat::None,
            depth_format: TextureFormat::None,
            sample_count: 1,
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
        self.render_graph = RenderGraph::default_render_graph();
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

    pub fn compile(
        &self,
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
    ) -> wgpu::RenderPipeline {
        let width = 1920;
        let height = 1080;
        let camera = camera::Camera {
            position: (0.0, 1.0, 2.0).into(),
            yaw: -90.0,
            pitch: 0.0,
            front: (0.0, 0.0, -1.0).into(),
            up: cgmath::Vector3::unit_y(),
            right: cgmath::Vector3::unit_x(),
            world_up: cgmath::Vector3::unit_y(),
            eye: (0.0, 1.0, 2.0).into(),
            // have it look at the origin
            target: (0.0, 0.0, 0.0).into(),
            // which way is "up"
            aspect: width as f32 / height as f32,
            fovy: 65.0,
            znear: 0.1,
            zfar: 15000.0,
        };

        let mut camera_uniform = camera::CameraUniform::new();
        camera_uniform.update_view_proj(&camera);

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: Some("camera_bind_group_layout"),
            });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
            label: Some("camera_bind_group"),
        });

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        // This should match the filterable field of the
                        // corresponding Texture entry above.
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
                label: Some("texture_bind_group_layout"),
            });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[&texture_bind_group_layout, &camera_bind_group_layout],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                buffers: &[model::ModelVertex::desc(), InstanceRaw::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Bgra8UnormSrgb,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });
        render_pipeline
    }
}

pub struct GeometryPassNode {
    render_data: Vec<RenderData>,
    camera_buffer: Option<BufferHandle>,
    camera_bind_group: Option<BindGroupHandle>,
    pub camera_bind_group_layout: Option<BindGroupLayoutHandle>,
    pass_inputs_group: Option<BindGroupHandle>,
}

pub struct CameraData {
    pub uniform: camera::CameraUniform,
}

impl RenderNode for GeometryPassNode {
    fn should_render_to_swapchain(&self) -> bool {
        true
    }

    fn describe(&self) -> RenderNodeDescriptor {
        //RenderNodeDescriptor {
        //    input_textures: &[],
        //    output_textures: &[],
        //    input_buffers: &[],
        //    output_buffers: &[],
        //}

        RenderNodeDescriptor {
            input_textures: &[],
            output_textures: &[OutputTexture::Create(TextureSlot {
                name: "color",
                texture_descriptor: TextureDescriptor {
                    label: "color",
                    size: TextureSize::FullRes,
                    format: TextureFormat::Bgra8UnormSrgb,
                    dimension: TextureDimension::D2,
                    usage: TextureUsages::RENDER_ATTACHMENT,
                    mip_levels: 1,
                    sample_count: 1,
                },
            })],
            input_buffers: &[],
            output_buffers: &[],
        }
    }

    fn compile(&mut self, ctx: &mut NodeCompileContext) {
        let buffer = ctx.create_buffer(&BufferDescriptor {
            label: "camera_uniform",
            size: size_of::<camera::CameraUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let layout = ctx
            .api
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: "camera_layout".to_string(),
                entries: vec![BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::Vertex,
                    entry_type: BindingType::UniformBuffer,
                }],
            });

        let bind_group = ctx.api.create_bind_group(&BindGroupDescriptor {
            label: "camera_bind_group".to_string(),
            layout,
            entries: vec![(0, BindGroupEntry::Buffer(buffer))],
        });

        self.camera_buffer = Some(buffer);
        self.camera_bind_group = Some(bind_group);
        self.camera_bind_group_layout = Some(layout);
    }

    fn prepare(&mut self, resources: &mut RenderResources, api: &mut dyn RendererAPI) {
        if let (Some(buffer), Some(camera_data)) =
            (self.camera_buffer, resources.get::<CameraData>())
        {
            api.write_buffer(buffer, bytemuck::cast_slice(&[camera_data.uniform]));
        }
    }

    fn run(&mut self, ctx: &mut dyn RenderContext) {
        ctx.bind_bind_group(0, self.camera_bind_group.unwrap());
        //ctx.bind_bind_group(1, self.pass_inputs_group);
        for render_data in &mut self.render_data {
            let pipeline = ctx
                .get_pipeline(render_data.material.pipeline_descriptor.uuid)
                .unwrap();
            ctx.bind_pipeline(pipeline);
            let vertex_buffer = ctx.get_mesh_vertex_buffer(&render_data.mesh);
            ctx.bind_vertex_buffer(vertex_buffer);
            let index_buffer = ctx.get_mesh_index_buffer(&render_data.mesh);
            ctx.bind_index_buffer(index_buffer);
            let index_count = ctx.get_mesh_index_count(&render_data.mesh);
            ctx.draw_indexed(index_count, 1);
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl GeometryPassNode {
    pub fn add_render_data(&mut self, new_render_data: RenderData) {
        self.render_data.push(new_render_data);
    }
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
    pub fn default_render_graph() -> Self {
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
}

impl dyn RenderNode {}
