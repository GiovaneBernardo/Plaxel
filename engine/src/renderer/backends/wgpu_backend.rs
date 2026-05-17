use crate::Arc;
use crate::Window;
use crate::assets;
use crate::assets::manager::AssetType;
use crate::assets::manager::Handle;
use crate::assets::material::PipelineDescriptor;
use crate::engine_info;
use crate::model::MeshAsset;
use crate::model::VertexLayout;
use crate::renderer::BindGroupHandle;
use crate::renderer::BufferDescriptor;
use crate::renderer::GraphResources;
use crate::renderer::OutputTexture;
use crate::renderer::TextureDescriptor;
use crate::renderer::TextureSize;
pub use crate::renderer::pool::*;
use crate::renderer::{
    self, BindGroupEntry, BindingType, BufferUsages, ShaderStages, TextureDimension, TextureFormat,
    TextureUsages,
};
use crate::texture;
use offset_allocator::Allocation;
use wgpu::IndexFormat;
use wgpu::util::DeviceExt;

use super::{
    BufferHandle, PipelineHandle, RenderGraph, RenderNode, RenderPassHandle, RendererAPI,
    TextureHandle,
};
use std::collections::HashMap;

pub use crate::renderer::backends::*;
use wgpu;

// --- From impls: engine types -> wgpu types ---

impl From<TextureFormat> for wgpu::TextureFormat {
    fn from(fmt: TextureFormat) -> wgpu::TextureFormat {
        match fmt {
            TextureFormat::None => panic!("Cannot convert TextureFormat::None to wgpu"),
            TextureFormat::Depth32Float => wgpu::TextureFormat::Depth32Float,
            TextureFormat::Depth24PlusStencil8 => wgpu::TextureFormat::Depth24PlusStencil8,
            TextureFormat::Depth24Plus => wgpu::TextureFormat::Depth24Plus,
            TextureFormat::Depth16Unorm => wgpu::TextureFormat::Depth16Unorm,
            TextureFormat::Depth32FloatStencil8 => wgpu::TextureFormat::Depth32FloatStencil8,
            TextureFormat::Depth32Stencil8 => wgpu::TextureFormat::Depth32FloatStencil8,
            TextureFormat::Stencil8 => wgpu::TextureFormat::Stencil8,
            TextureFormat::Rgba8Unorm => wgpu::TextureFormat::Rgba8Unorm,
            TextureFormat::Rgba8UnormSrgb => wgpu::TextureFormat::Rgba8UnormSrgb,
            TextureFormat::Rgba16Float => wgpu::TextureFormat::Rgba16Float,
            TextureFormat::Rgba32Float => wgpu::TextureFormat::Rgba32Float,
            TextureFormat::Rgba8Snorm => wgpu::TextureFormat::Rgba8Snorm,
            TextureFormat::Rgba16Snorm => wgpu::TextureFormat::Rgba16Snorm,
            TextureFormat::Rgba8Uint => wgpu::TextureFormat::Rgba8Uint,
            TextureFormat::Rgba8Sint => wgpu::TextureFormat::Rgba8Sint,
            TextureFormat::Rgba16Uint => wgpu::TextureFormat::Rgba16Uint,
            TextureFormat::Rgba16Sint => wgpu::TextureFormat::Rgba16Sint,
            TextureFormat::Rgba32Uint => wgpu::TextureFormat::Rgba32Uint,
            TextureFormat::Rgba32Sint => wgpu::TextureFormat::Rgba32Sint,
            TextureFormat::Rg32Float => wgpu::TextureFormat::Rg32Float,
            TextureFormat::Rg32Uint => wgpu::TextureFormat::Rg32Uint,
            TextureFormat::Rg32Sint => wgpu::TextureFormat::Rg32Sint,
            TextureFormat::Rg16Float => wgpu::TextureFormat::Rg16Float,
            TextureFormat::Rg16Uint => wgpu::TextureFormat::Rg16Uint,
            TextureFormat::Rg16Sint => wgpu::TextureFormat::Rg16Sint,
            TextureFormat::Rg8Unorm => wgpu::TextureFormat::Rg8Unorm,
            TextureFormat::Rg8Snorm => wgpu::TextureFormat::Rg8Snorm,
            TextureFormat::Rg8Uint => wgpu::TextureFormat::Rg8Uint,
            TextureFormat::Rg8Sint => wgpu::TextureFormat::Rg8Sint,
            TextureFormat::R32Float => wgpu::TextureFormat::R32Float,
            TextureFormat::R32Uint => wgpu::TextureFormat::R32Uint,
            TextureFormat::R32Sint => wgpu::TextureFormat::R32Sint,
            TextureFormat::R16Float => wgpu::TextureFormat::R16Float,
            TextureFormat::R16Uint => wgpu::TextureFormat::R16Uint,
            TextureFormat::R16Sint => wgpu::TextureFormat::R16Sint,
            TextureFormat::R8Unorm => wgpu::TextureFormat::R8Unorm,
            TextureFormat::R8Snorm => wgpu::TextureFormat::R8Snorm,
            TextureFormat::R8Uint => wgpu::TextureFormat::R8Uint,
            TextureFormat::R8Sint => wgpu::TextureFormat::R8Sint,
            TextureFormat::Bgra8Unorm => wgpu::TextureFormat::Bgra8Unorm,
            TextureFormat::Rgba8Srgb => wgpu::TextureFormat::Rgba8UnormSrgb,
            TextureFormat::Bgra8UnormSrgb => wgpu::TextureFormat::Bgra8UnormSrgb,
            TextureFormat::Rgb10a2Unorm => wgpu::TextureFormat::Rgb10a2Unorm,
            TextureFormat::Rgb10a2Uint => wgpu::TextureFormat::Rgb10a2Uint,
            TextureFormat::Rg11b10Float => wgpu::TextureFormat::Rg11b10Ufloat,
            TextureFormat::Rgb9e5Ufloat => wgpu::TextureFormat::Rgb9e5Ufloat,
        }
    }
}

impl From<TextureDimension> for wgpu::TextureDimension {
    fn from(dim: TextureDimension) -> wgpu::TextureDimension {
        match dim {
            TextureDimension::D2 => wgpu::TextureDimension::D2,
            TextureDimension::D3 => wgpu::TextureDimension::D3,
            TextureDimension::D2Array => wgpu::TextureDimension::D2,
            TextureDimension::Cube => wgpu::TextureDimension::D2,
        }
    }
}

impl From<BufferUsages> for wgpu::BufferUsages {
    fn from(usage: BufferUsages) -> wgpu::BufferUsages {
        let mut result = wgpu::BufferUsages::empty();
        if usage.contains(BufferUsages::MAP_READ) {
            result |= wgpu::BufferUsages::MAP_READ;
        }
        if usage.contains(BufferUsages::MAP_WRITE) {
            result |= wgpu::BufferUsages::MAP_WRITE;
        }
        if usage.contains(BufferUsages::COPY_SRC) {
            result |= wgpu::BufferUsages::COPY_SRC;
        }
        if usage.contains(BufferUsages::COPY_DST) {
            result |= wgpu::BufferUsages::COPY_DST;
        }
        if usage.contains(BufferUsages::INDEX) {
            result |= wgpu::BufferUsages::INDEX;
        }
        if usage.contains(BufferUsages::VERTEX) {
            result |= wgpu::BufferUsages::VERTEX;
        }
        if usage.contains(BufferUsages::UNIFORM) {
            result |= wgpu::BufferUsages::UNIFORM;
        }
        if usage.contains(BufferUsages::STORAGE) {
            result |= wgpu::BufferUsages::STORAGE;
        }
        if usage.contains(BufferUsages::INDIRECT) {
            result |= wgpu::BufferUsages::INDIRECT;
        }
        if usage.contains(BufferUsages::QUERY_RESOLVE) {
            result |= wgpu::BufferUsages::QUERY_RESOLVE;
        }
        result
    }
}

impl From<TextureUsages> for wgpu::TextureUsages {
    fn from(usage: TextureUsages) -> wgpu::TextureUsages {
        let mut result = wgpu::TextureUsages::empty();
        if usage.contains(TextureUsages::COPY_SRC) {
            result |= wgpu::TextureUsages::COPY_SRC;
        }
        if usage.contains(TextureUsages::COPY_DST) {
            result |= wgpu::TextureUsages::COPY_DST;
        }
        if usage.contains(TextureUsages::TEXTURE_BINDING) {
            result |= wgpu::TextureUsages::TEXTURE_BINDING;
        }
        if usage.contains(TextureUsages::STORAGE_BINDING) {
            result |= wgpu::TextureUsages::STORAGE_BINDING;
        }
        if usage.contains(TextureUsages::RENDER_ATTACHMENT) {
            result |= wgpu::TextureUsages::RENDER_ATTACHMENT;
        }
        result
    }
}

impl From<ShaderStages> for wgpu::ShaderStages {
    fn from(stages: ShaderStages) -> wgpu::ShaderStages {
        match stages {
            ShaderStages::Vertex => wgpu::ShaderStages::VERTEX,
            ShaderStages::Fragment => wgpu::ShaderStages::FRAGMENT,
            ShaderStages::Both => wgpu::ShaderStages::VERTEX_FRAGMENT,
            ShaderStages::Compute => wgpu::ShaderStages::COMPUTE,
        }
    }
}

impl From<&BindingType> for wgpu::BindingType {
    fn from(ty: &BindingType) -> wgpu::BindingType {
        match ty {
            BindingType::UniformBuffer => wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            BindingType::StorageBuffer { read_only } => wgpu::BindingType::Buffer {
                ty: if *read_only {
                    wgpu::BufferBindingType::Storage { read_only: true }
                } else {
                    wgpu::BufferBindingType::Storage { read_only: false }
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            BindingType::Texture {
                dimension,
                multisampled,
            } => wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: match dimension {
                    TextureDimension::D2 => wgpu::TextureViewDimension::D2,
                    TextureDimension::D3 => wgpu::TextureViewDimension::D3,
                    TextureDimension::D2Array => wgpu::TextureViewDimension::D2Array,
                    TextureDimension::Cube => wgpu::TextureViewDimension::Cube,
                },
                multisampled: *multisampled,
            },
            BindingType::Sampler => wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        }
    }
}

use crate::model::{AttributeFormat, StepMode};
use crate::renderer::{
    BlendMode, CompareFunction, CullMode, DepthState, FrontFace, MultisampleState, PolygonMode,
    Topology,
};

impl From<Topology> for wgpu::PrimitiveTopology {
    fn from(t: Topology) -> wgpu::PrimitiveTopology {
        match t {
            Topology::TriangleList => wgpu::PrimitiveTopology::TriangleList,
            Topology::TriangleStrip => wgpu::PrimitiveTopology::TriangleStrip,
            Topology::LineList => wgpu::PrimitiveTopology::LineList,
            Topology::LineStrip => wgpu::PrimitiveTopology::LineStrip,
            Topology::PointList => wgpu::PrimitiveTopology::PointList,
        }
    }
}

impl From<FrontFace> for wgpu::FrontFace {
    fn from(f: FrontFace) -> wgpu::FrontFace {
        match f {
            FrontFace::Ccw => wgpu::FrontFace::Ccw,
            FrontFace::Cw => wgpu::FrontFace::Cw,
        }
    }
}

impl From<CullMode> for Option<wgpu::Face> {
    fn from(c: CullMode) -> Option<wgpu::Face> {
        match c {
            CullMode::None => None,
            CullMode::Front => Some(wgpu::Face::Front),
            CullMode::Back => Some(wgpu::Face::Back),
        }
    }
}

impl From<PolygonMode> for wgpu::PolygonMode {
    fn from(p: PolygonMode) -> wgpu::PolygonMode {
        match p {
            PolygonMode::Fill => wgpu::PolygonMode::Fill,
            PolygonMode::Line => wgpu::PolygonMode::Line,
            PolygonMode::Point => wgpu::PolygonMode::Point,
        }
    }
}

impl From<CompareFunction> for wgpu::CompareFunction {
    fn from(c: CompareFunction) -> wgpu::CompareFunction {
        match c {
            CompareFunction::Never => wgpu::CompareFunction::Never,
            CompareFunction::Less => wgpu::CompareFunction::Less,
            CompareFunction::Equal => wgpu::CompareFunction::Equal,
            CompareFunction::LessEqual => wgpu::CompareFunction::LessEqual,
            CompareFunction::Greater => wgpu::CompareFunction::Greater,
            CompareFunction::NotEqual => wgpu::CompareFunction::NotEqual,
            CompareFunction::GreaterEqual => wgpu::CompareFunction::GreaterEqual,
            CompareFunction::Always => wgpu::CompareFunction::Always,
        }
    }
}

fn blend_mode_to_wgpu(mode: BlendMode) -> Option<wgpu::BlendState> {
    match mode {
        BlendMode::None => None,
        BlendMode::Replace => Some(wgpu::BlendState::REPLACE),
        BlendMode::Alpha => Some(wgpu::BlendState::ALPHA_BLENDING),
        BlendMode::Additive => Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        }),
    }
}

impl From<AttributeFormat> for wgpu::VertexFormat {
    fn from(fmt: AttributeFormat) -> wgpu::VertexFormat {
        match fmt {
            AttributeFormat::Float32 => wgpu::VertexFormat::Float32,
            AttributeFormat::Float32x2 => wgpu::VertexFormat::Float32x2,
            AttributeFormat::Float32x3 => wgpu::VertexFormat::Float32x3,
            AttributeFormat::Float32x4 => wgpu::VertexFormat::Float32x4,
            AttributeFormat::Uint32 => wgpu::VertexFormat::Uint32,
            AttributeFormat::Uint8x4 => wgpu::VertexFormat::Uint8x4,
            AttributeFormat::Snorm8x4 => wgpu::VertexFormat::Snorm8x4,
            AttributeFormat::Unorm8x4 => wgpu::VertexFormat::Unorm8x4,
        }
    }
}

impl From<StepMode> for wgpu::VertexStepMode {
    fn from(mode: StepMode) -> wgpu::VertexStepMode {
        match mode {
            StepMode::Vertex => wgpu::VertexStepMode::Vertex,
            StepMode::Instance => wgpu::VertexStepMode::Instance,
        }
    }
}

pub struct GpuMesh {
    pub pool: VertexPoolId,
    pub vertex_alloc: Allocation,
    pub index_page: u32,
    pub index_alloc: Allocation,
    pub index_count: u32,
    pub first_index: u32,
    pub base_vertex: i32,
}

pub struct WgpuBackend {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,
    surface: wgpu::Surface<'static>,
    is_surface_configured: bool,
    depth_texture: texture::Texture,
    pipelines: HashMap<PipelineHandle, wgpu::RenderPipeline>,
    pipelines_by_uuid: HashMap<Uuid, PipelineHandle>,
    buffers: HashMap<BufferHandle, wgpu::Buffer>,
    bind_groups: HashMap<BindGroupHandle, wgpu::BindGroup>,
    bind_group_layouts: HashMap<BindGroupLayoutHandle, wgpu::BindGroupLayout>,
    textures: HashMap<TextureHandle, wgpu::Texture>,
    texture_views: HashMap<TextureHandle, wgpu::TextureView>,
    pool_manager: PoolManager,
    gpu_meshes: HashMap<Handle<MeshAsset>, GpuMesh>,
    shaders_hot_reload_data: HashMap<String, Vec<(PipelineDescriptor, PipelineHandle)>>,
}

pub struct WgpuRenderContext<'a> {
    pub backend: &'a mut WgpuBackend,
    pub pass: wgpu::RenderPass<'a>,
}

impl<'a> RenderContext for WgpuRenderContext<'a> {
    fn api(&mut self) -> &mut dyn RendererAPI {
        self.backend
    }

    fn bind_pipeline(&mut self, handle: PipelineHandle) {
        let pipeline = &self.backend.pipelines[&handle];
        self.pass.set_pipeline(pipeline);
    }

    fn draw(&mut self, vertices: u32, instances: u32) {
        self.pass.draw(0..vertices, 0..instances);
    }

    fn draw_indexed(
        &mut self,
        first_index: u32,
        index_count: u32,
        base_vertex: i32,
        instances: u32,
    ) {
        self.pass.draw_indexed(
            first_index..first_index + index_count,
            base_vertex,
            0..instances,
        );
    }

    fn bind_vertex_buffer(&mut self, slot: u32, buffer: BufferHandle) {
        self.pass
            .set_vertex_buffer(slot, self.backend.get_buffer(buffer).unwrap().slice(..));
    }

    fn bind_index_buffer(&mut self, buffer: BufferHandle) {
        self.pass.set_index_buffer(
            self.backend.get_buffer(buffer).unwrap().slice(..),
            IndexFormat::Uint32,
        );
    }

    fn bind_bind_group(&mut self, index: u32, bind_group_handle: BindGroupHandle) {
        let bind_group = self.backend.get_bind_group(bind_group_handle).unwrap();
        self.pass.set_bind_group(index, bind_group, &[]);
    }

    fn with_raw_pass(&mut self, f: &mut dyn FnMut(&mut wgpu::RenderPass<'static>)) {
        // SAFETY: The pass is valid for the duration of this call. The 'static
        // lifetime is required by egui_wgpu's API (forget_lifetime pattern).
        // The closure must not store the reference.
        let pass: &mut wgpu::RenderPass<'static> = unsafe { std::mem::transmute(&mut self.pass) };
        f(pass);
    }
}

impl RendererAPI for WgpuBackend {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn compile(&mut self) {}

    fn resize(&mut self, width: u32, height: u32) {
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
        self.depth_texture = texture::Texture::create_depth_texture(
            &self.device,
            &self.surface_config,
            "depth_texture",
        );
    }

    fn resize_texture(&mut self, texture_handle: &TextureHandle, descriptor: &TextureDescriptor) {
        let width = self.window.inner_size().width;
        let height = self.window.inner_size().height;
        let (tex_width, tex_height) = match descriptor.size {
            TextureSize::FullRes => (width, height),
            TextureSize::HalfRes => (width / 2, height / 2),
            TextureSize::QuarterRes => (width / 4, height / 4),
            TextureSize::Custom { width, height } => (width, height),
        };

        let depth_or_array_layers = match descriptor.dimension {
            TextureDimension::Cube => 6,
            _ => 1,
        };

        let wgpu_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(descriptor.label),
            size: wgpu::Extent3d {
                width: tex_width,
                height: tex_height,
                depth_or_array_layers,
            },
            mip_level_count: descriptor.mip_levels,
            sample_count: descriptor.sample_count,
            dimension: descriptor.dimension.into(),
            format: descriptor.format.into(),
            usage: descriptor.usage.into(),
            view_formats: &[],
        });

        let view = wgpu_texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.textures.insert(*texture_handle, wgpu_texture);
        self.texture_views.insert(*texture_handle, view);
    }

    fn compile_pipeline(&mut self, node: &dyn RenderNode) -> PipelineHandle {
        PipelineHandle(0)
    }

    fn submit(&mut self, graph: &RenderGraph) {}

    fn render(&mut self, render_graph: &mut RenderGraph) -> anyhow::Result<()> {
        //match state.render(&mut self.on_render) {
        //    Ok(_) => {}
        //    // Reconfigure the surface if it's lost or outdated
        //    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
        //        let size = state.window.inner_size();
        //        state.resize(size.width, size.height);
        //    }
        //    Err(e) => {
        //        log::error!("Unable to render {}", e);
        //    }
        //}

        if !render_graph.compiled {
            engine_info!("Render graph not compiled");
            //render_graph.compile(self.render_resources, self);
        }

        let surface = &self.surface;

        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        for (i, (_, node)) in render_graph.nodes.iter_mut().enumerate() {
            self.render_node(
                node.as_mut(),
                &render_graph.resources,
                &mut encoder,
                &view,
                i == 0,
            );
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }

    fn reload_shader(&mut self, shader_path: &str) {
        //self.create_pipeline(material, bind_group_layouts);
    }

    // Load assets
    fn load_material(&mut self, header: &crate::assets::manager::AssetHeader) -> Material {
        engine_info!("Loading material: {:?}", header);

        let pipeline_descriptor = PipelineDescriptor::new("shaders/cube.wgsl".to_string());
        let pipeline_uuid = pipeline_descriptor.uuid;
        Material::default()
        //Material {
        //    uuid: Uuid::new_v4(),
        //    pipeline_descriptor,
        //    pipeline_uuid,
        //}
    }

    fn create_pipeline(
        &mut self,
        material: &Material,
        bind_group_layouts: &[BindGroupLayoutHandle],
    ) {
        engine_info!("Shader name: {:?}", material.pipeline_descriptor.shader);
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Shader"),
                source: wgpu::ShaderSource::Wgsl(
                    pollster::block_on(assets::resources::load_string(
                        &material.pipeline_descriptor.shader,
                    ))
                    .unwrap()
                    .into(),
                ),
            });

        let wgpu_layouts: Vec<&wgpu::BindGroupLayout> = bind_group_layouts
            .iter()
            .map(|h| self.get_bind_group_layout(*h).unwrap())
            .collect();

        let render_pipeline_layout =
            self.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Render Pipeline Layout"),
                    bind_group_layouts: &wgpu_layouts,
                    push_constant_ranges: &[],
                });

        engine_info!("Depth Format: {:?}", texture::Texture::DEPTH_FORMAT);

        let desc = &material.pipeline_descriptor;

        // Build vertex buffer layouts from material's layout descriptors
        let wgpu_attributes: Vec<Vec<wgpu::VertexAttribute>> = desc
            .vertex_layouts
            .iter()
            .map(|layout| {
                layout
                    .attributes
                    .iter()
                    .map(|attr| wgpu::VertexAttribute {
                        offset: attr.offset,
                        shader_location: attr.shader_location,
                        format: attr.format.into(),
                    })
                    .collect()
            })
            .collect();

        let vertex_buffer_layouts: Vec<wgpu::VertexBufferLayout> = desc
            .vertex_layouts
            .iter()
            .enumerate()
            .map(|(i, layout)| wgpu::VertexBufferLayout {
                array_stride: layout.stride,
                step_mode: layout.step_mode.into(),
                attributes: &wgpu_attributes[i],
            })
            .collect();

        let strip_index_format = match desc.topology {
            Topology::TriangleStrip | Topology::LineStrip => Some(wgpu::IndexFormat::Uint32),
            _ => None,
        };

        let depth_stencil = desc.depth_state.map(|ds| wgpu::DepthStencilState {
            format: texture::Texture::DEPTH_FORMAT,
            depth_write_enabled: ds.write_enabled,
            depth_compare: ds.compare.into(),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        });

        let render_pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&material.pipeline_descriptor.shader),
                layout: Some(&render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &vertex_buffer_layouts,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: self.surface_config.format,
                        blend: blend_mode_to_wgpu(desc.blend_mode),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: desc.topology.into(),
                    strip_index_format,
                    front_face: desc.front_face.into(),
                    cull_mode: desc.cull_mode.into(),
                    polygon_mode: desc.polygon_mode.into(),
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil,
                multisample: wgpu::MultisampleState {
                    count: desc.multisample.count,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
                cache: None,
            });

        self.add_render_pipeline(render_pipeline, &material.pipeline_descriptor);
    }

    fn create_render_data(
        &mut self,
        vertex_bytes: &Vec<u8>, // How to turn a Vec of vertices into bytes: bytemuck::cast_slice(&positions_raw).to_vec();
        indices: &Vec<u32>,
        material: Material,
        pipeline_handle: &PipelineHandle,
    ) -> RenderData {
        let mesh = MeshAsset {
            name: "Cube".to_string(),
            uuid: Uuid::new_v4(),
            vertices: vertex_bytes.clone(),
            indices: bytemuck::cast_slice(&indices).to_vec(),
            vertex_layout: material.pipeline_descriptor.vertex_layouts[0].clone(),
            //vertex_layout: VertexLayout {
            //    stride: std::mem::size_of::<[f32; 3]>() as u64,
            //    step_mode: crate::model::StepMode::Vertex,
            //    attributes: Vec::new(),
            //},
        };

        RenderData {
            mesh: self.load_mesh_with_data(&mesh),
            material,
            transform_index: 0,
            sort_key: 0,
        }
    }

    fn write_buffer(&mut self, buffer: BufferHandle, data: &[u8]) {
        let wgpu_buffer = self.get_buffer(buffer).unwrap();
        self.queue.write_buffer(wgpu_buffer, 0, data);
    }

    fn read_texture_bytes(&mut self, texture: &TextureHandle, x: f32, y: f32, out: &mut [u8]) {
        let wgpu_texture = self.get_texture(*texture).unwrap();
        let width = wgpu_texture.width();
        let height = wgpu_texture.height();

        let bytes_per_pixel = WgpuBackend::bytes_per_pixel(wgpu_texture.format()).unwrap();
        let unpadded_bytes_per_row = width * bytes_per_pixel;
        let padded_bytes_per_row =
            unpadded_bytes_per_row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);

        let buffer_size = padded_bytes_per_row as u64 * height as u64;

        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Output Buffer"),
            size: (wgpu_texture.width() * wgpu_texture.height() * 4) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Reader Encoder"),
            });

        let texture_size = wgpu::Extent3d {
            width: width,
            height: height,
            depth_or_array_layers: 0,
        };
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &wgpu_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &output_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row), // Must be multiple of 256
                    rows_per_image: Some(height),
                },
            },
            texture_size,
        );

        self.queue.submit(std::iter::once(encoder.finish()));

        //self.device.r
    }

    // Get using Uuids
    fn get_pipeline(&mut self, uuid: Uuid) -> Option<PipelineHandle> {
        self.pipelines_by_uuid.get(&uuid).cloned()
    }

    fn get_mesh_vertex_buffer(&mut self, mesh: &Handle<MeshAsset>) -> BufferHandle {
        let gm = self.gpu_meshes.get(mesh).unwrap();
        self.pool_manager.vertex_buffer(gm.pool)
    }

    fn get_mesh_index_buffer(&mut self, mesh: &Handle<MeshAsset>) -> BufferHandle {
        let gm = self.gpu_meshes.get(mesh).unwrap();
        self.pool_manager.index_buffer(gm.index_page)
    }
    fn get_mesh_index_count(&mut self, mesh: &Handle<MeshAsset>) -> u32 {
        self.gpu_meshes.get(mesh).unwrap().index_count
    }

    fn get_mesh_draw_range(&mut self, mesh: &Handle<MeshAsset>) -> MeshDrawRange {
        let gm = self.gpu_meshes.get(mesh).unwrap();
        MeshDrawRange {
            first_index: gm.first_index,
            index_count: gm.index_count,
            base_vertex: gm.base_vertex,
        }
    }

    fn get_mesh_instance_count(&mut self, mesh: &Handle<MeshAsset>) -> u32 {
        self.gpu_meshes.get(mesh).unwrap().index_count
    }
    fn get_mesh_instance_buffer(&mut self, _mesh: &Handle<MeshAsset>) -> BufferHandle {
        BufferHandle(0)
    }

    fn set_texture(&mut self, texture: &texture::Texture) {
        // ?????????????? I don't even know what set_texture is used for
        self.depth_texture = texture.clone();
    }

    fn upload_mesh(&mut self, mesh: &MeshAsset) -> Handle<MeshAsset> {
        self.load_mesh_with_data(mesh)
    }

    fn create_texture(&mut self, descriptor: &TextureDescriptor) -> TextureHandle {
        let size = self.window.inner_size();
        let (tex_width, tex_height) = match descriptor.size {
            TextureSize::FullRes => (size.width, size.height),
            TextureSize::HalfRes => (size.width / 2, size.height / 2),
            TextureSize::QuarterRes => (size.width / 4, size.height / 4),
            TextureSize::Custom { width, height } => (width, height),
        };

        let depth_or_array_layers = match descriptor.dimension {
            TextureDimension::Cube => 6,
            _ => 1,
        };

        let wgpu_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(descriptor.label),
            size: wgpu::Extent3d {
                width: tex_width,
                height: tex_height,
                depth_or_array_layers,
            },
            mip_level_count: descriptor.mip_levels,
            sample_count: descriptor.sample_count,
            dimension: descriptor.dimension.into(),
            format: descriptor.format.into(),
            usage: descriptor.usage.into(),
            view_formats: &[],
        });

        self.add_texture(wgpu_texture)
    }

    fn create_buffer(&mut self, descriptor: &BufferDescriptor) -> BufferHandle {
        let wgpu_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(descriptor.label),
            size: descriptor.size,
            usage: descriptor.usage.into(),
            mapped_at_creation: false,
        });

        self.add_buffer(wgpu_buffer)
    }

    fn create_bind_group_layout(
        &mut self,
        descriptor: &BindGroupLayoutDescriptor,
    ) -> BindGroupLayoutHandle {
        let wgpu_entries: Vec<wgpu::BindGroupLayoutEntry> = descriptor
            .entries
            .iter()
            .map(|entry| wgpu::BindGroupLayoutEntry {
                binding: entry.binding,
                visibility: entry.visibility.into(),
                ty: (&entry.entry_type).into(),
                count: None,
            })
            .collect();

        let wgpu_bind_group_layout =
            self.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some(&descriptor.label),
                    entries: &wgpu_entries,
                });

        self.add_bind_group_layout(wgpu_bind_group_layout)
    }

    fn create_bind_group(&mut self, descriptor: &BindGroupDescriptor) -> BindGroupHandle {
        // Collect resource references first since we need to borrow self immutably
        let layout = self.get_bind_group_layout(descriptor.layout).unwrap();

        let wgpu_entries: Vec<wgpu::BindGroupEntry> = descriptor
            .entries
            .iter()
            .map(|(binding, entry)| {
                let resource = match entry {
                    BindGroupEntry::Buffer(handle) => {
                        self.get_buffer(*handle).unwrap().as_entire_binding()
                    }
                    BindGroupEntry::Texture(handle) => {
                        wgpu::BindingResource::TextureView(self.get_texture_view(*handle).unwrap())
                    }
                };
                wgpu::BindGroupEntry {
                    binding: *binding,
                    resource,
                }
            })
            .collect();

        let wgpu_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout,
            label: Some(&descriptor.label),
            entries: &wgpu_entries,
        });

        self.add_bind_group(wgpu_bind_group)
    }
}

impl WgpuBackend {
    fn load_mesh_with_data(&mut self, mesh: &MeshAsset) -> Handle<MeshAsset> {
        let handle: Handle<MeshAsset> = Handle {
            uuid: mesh.uuid,
            asset_type: AssetType::Mesh,
            _marker: std::marker::PhantomData,
        };

        let stride = mesh.vertex_layout.stride as u32;
        assert!(stride > 0, "vertex layout stride must be > 0");
        assert_eq!(
            mesh.vertices.len() as u32 % stride,
            0,
            "vertex bytes not a multiple of stride"
        );
        let vertex_count = mesh.vertices.len() as u32 / stride;
        let index_count = mesh.indices.len() as u32;

        let layout_idx = self.pool_manager.get_or_create_layout(&mesh.vertex_layout);

        // Allocate vertices. Split-borrow `device`/`buffers` so the closure
        // doesn't conflict with the `&mut self.pool_manager` call.
        let (v_page, v_alloc) = {
            let device = &self.device;
            let buffers = &mut self.buffers;
            let mut make_vb = |cap: u32| -> BufferHandle {
                let buf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("VertexPoolPage"),
                    size: cap as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let h = BufferHandle(buffers.len() as u32);
                buffers.insert(h, buf);
                h
            };
            self.pool_manager
                .alloc_vertices(layout_idx, vertex_count, stride, &mut make_vb)
        };

        let (i_page, i_alloc) = {
            let device = &self.device;
            let buffers = &mut self.buffers;
            let mut make_ib = |cap: u32| -> BufferHandle {
                let buf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("IndexPoolPage"),
                    size: cap as u64,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let h = BufferHandle(buffers.len() as u32);
                buffers.insert(h, buf);
                h
            };
            self.pool_manager.alloc_indices(index_count, &mut make_ib)
        };

        // Byte offsets come from allocator units × element size.
        let vertex_byte_offset = v_alloc.offset as u64 * stride as u64;
        let index_byte_offset = i_alloc.offset as u64 * 4;

        let pool_id = VertexPoolId {
            layout_index: layout_idx,
            page_index: v_page,
        };
        let v_buffer_handle = self.pool_manager.vertex_buffer(pool_id);
        let i_buffer_handle = self.pool_manager.index_buffer(i_page);

        let vb = self.buffers.get(&v_buffer_handle).unwrap();
        self.queue
            .write_buffer(vb, vertex_byte_offset, &mesh.vertices);

        let index_bytes: &[u8] = bytemuck::cast_slice(&mesh.indices);
        let ib = self.buffers.get(&i_buffer_handle).unwrap();
        self.queue.write_buffer(ib, index_byte_offset, index_bytes);

        let gpu_mesh = GpuMesh {
            pool: pool_id,
            vertex_alloc: v_alloc,
            index_page: i_page,
            index_alloc: i_alloc,
            index_count,
            first_index: i_alloc.offset,
            base_vertex: v_alloc.offset as i32,
        };
        self.gpu_meshes.insert(handle, gpu_mesh);

        handle
    }

    pub fn free_mesh(&mut self, handle: Handle<MeshAsset>) {
        if let Some(mesh) = self.gpu_meshes.remove(&handle) {
            self.pool_manager
                .free_vertices(mesh.pool, mesh.vertex_alloc);
            self.pool_manager
                .free_indices(mesh.index_page, mesh.index_alloc);
        }
    }
}

impl WgpuBackend {
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.surface_config.format
    }
}

impl WgpuBackend {
    pub async fn new(window: Arc<Window>) -> anyhow::Result<Self> {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            #[cfg(not(target_arch = "wasm32"))]
            backends: wgpu::Backends::PRIMARY,
            #[cfg(target_arch = "wasm32")]
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                required_limits: if cfg!(target_arch = "wasm32") {
                    wgpu::Limits::default()
                } else {
                    wgpu::Limits::default()
                },
                memory_hints: Default::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        let depth_texture =
            texture::Texture::create_depth_texture(&device, &config, "depth_texture");

        Ok(Self {
            window,
            device,
            queue,
            surface,
            surface_config: config,
            is_surface_configured: false,
            depth_texture,
            pipelines: HashMap::new(),
            pipelines_by_uuid: HashMap::new(),
            buffers: HashMap::new(),
            bind_groups: HashMap::new(),
            bind_group_layouts: HashMap::new(),
            textures: HashMap::new(),
            texture_views: HashMap::new(),
            pool_manager: PoolManager::new(),
            gpu_meshes: HashMap::new(),
            shaders_hot_reload_data: HashMap::new(),
        })
    }

    fn render_node(
        &mut self,
        node: &mut dyn RenderNode,
        resources: &GraphResources,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        clear: bool,
    ) {
        let depth_load = if clear {
            // Reverse-Z: clear to 0.0 (the "far" value); depth_compare = Greater.
            wgpu::LoadOp::Clear(0.0)
        } else {
            wgpu::LoadOp::Load
        };

        let color_load = if clear {
            wgpu::LoadOp::Clear(wgpu::Color {
                r: 0.1,
                g: 0.2,
                b: 0.3,
                a: 1.0,
            })
        } else {
            wgpu::LoadOp::Load
        };

        let render_node_descriptor = node.describe_pass();
        let mut color_attachments: Vec<Option<wgpu::RenderPassColorAttachment<'_>>> = Vec::new();
        let mut i = 0;
        let mut depth_stencil_attachment: Option<wgpu::RenderPassDepthStencilAttachment<'_>> = None;

        for output_texture in render_node_descriptor.output_textures {
            let texture_name = match output_texture {
                OutputTexture::Create(slot) => slot.name,
                OutputTexture::WriteTo(name) => name,
            };

            if texture_name == "swapchain_image" {
                color_attachments.insert(
                    i,
                    Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: color_load,
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    }),
                );
                i += 1;
                continue;
            }

            let handle = resources.texture(texture_name).unwrap();
            let Some(view) = self.get_texture_view(*handle) else {
                continue;
            };
            let Some(texture) = self.get_texture(*handle) else {
                continue;
            };

            let format = texture.format();

            if WgpuBackend::is_depth(format) {
                depth_stencil_attachment = Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &view,
                    depth_ops: Some(wgpu::Operations {
                        load: depth_load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                });
                continue;
            }

            color_attachments.insert(
                i,
                Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: color_load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                }),
            );
            i += 1;
        }

        if color_attachments.len() == 0 && depth_stencil_attachment.is_none() {
            return;
        }

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(render_node_descriptor.name),
            color_attachments: &color_attachments,
            depth_stencil_attachment: depth_stencil_attachment,
            occlusion_query_set: None,
            timestamp_writes: None,
        });

        let mut ctx = WgpuRenderContext {
            backend: self,
            pass: render_pass,
        };
        node.run(&mut ctx);

        //for render_data in &node.render_data {
        //    render_pass.set_pipeline(render_data.pipeline);
        //    render_pass.set_vertex_buffer(0, render_data.vertex_buffer.slice(..));
        //    render_pass.set_index_buffer(
        //        render_data.index_buffer.slice(..),
        //        wgpu::IndexFormat::Uint32,
        //    );
        //    render_pass.draw_indexed(0..render_data.num_elements, 0, 0..1);
        //}
    }

    fn get_render_pipeline(
        &self,
        pipeline_handle: PipelineHandle,
    ) -> Option<&wgpu::RenderPipeline> {
        self.pipelines.get(&pipeline_handle)
    }

    fn add_render_pipeline(
        &mut self,
        pipeline: wgpu::RenderPipeline,
        pipeline_descriptor: &PipelineDescriptor,
    ) -> PipelineHandle {
        let handle = PipelineHandle(self.pipelines.len() as u32);
        self.pipelines.insert(handle, pipeline);
        self.pipelines_by_uuid
            .insert(pipeline_descriptor.uuid, handle);
        self.shaders_hot_reload_data
            .entry(pipeline_descriptor.shader.clone())
            .or_insert_with(Vec::new)
            .push((pipeline_descriptor.clone(), handle));
        handle
    }

    fn get_buffer(&self, handle: BufferHandle) -> Option<&wgpu::Buffer> {
        self.buffers.get(&handle)
    }

    fn get_bind_group(&self, handle: BindGroupHandle) -> Option<&wgpu::BindGroup> {
        self.bind_groups.get(&handle)
    }

    pub fn add_bind_group(&mut self, bind_group: wgpu::BindGroup) -> BindGroupHandle {
        let handle = BindGroupHandle(self.bind_groups.len() as u32);
        self.bind_groups.insert(handle, bind_group);
        handle
    }

    fn add_buffer(&mut self, buffer: wgpu::Buffer) -> BufferHandle {
        let handle = BufferHandle(self.buffers.len() as u32);
        self.buffers.insert(handle, buffer);
        handle
    }

    fn get_texture(&self, handle: TextureHandle) -> Option<&wgpu::Texture> {
        self.textures.get(&handle)
    }

    pub fn add_texture(&mut self, texture: wgpu::Texture) -> TextureHandle {
        let handle = TextureHandle(self.textures.len() as u32);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.textures.insert(handle, texture);
        self.texture_views.insert(handle, view);
        handle
    }

    fn get_texture_view(&self, handle: TextureHandle) -> Option<&wgpu::TextureView> {
        self.texture_views.get(&handle)
    }

    pub fn add_bind_group_layout(
        &mut self,
        layout: wgpu::BindGroupLayout,
    ) -> BindGroupLayoutHandle {
        let handle = BindGroupLayoutHandle(self.bind_group_layouts.len() as u32);
        self.bind_group_layouts.insert(handle, layout);
        handle
    }

    fn get_bind_group_layout(
        &self,
        handle: BindGroupLayoutHandle,
    ) -> Option<&wgpu::BindGroupLayout> {
        self.bind_group_layouts.get(&handle)
    }

    pub fn bytes_per_pixel(format: wgpu::TextureFormat) -> Option<u32> {
        use wgpu::TextureFormat::*;

        Some(match format {
            R8Unorm | R8Snorm | R8Uint | R8Sint => 1,

            Rg8Unorm | Rg8Snorm | Rg8Uint | Rg8Sint | R16Uint | R16Sint | R16Float => 2,

            Rgba8Unorm | Rgba8UnormSrgb | Bgra8Unorm | Bgra8UnormSrgb | Rgba8Snorm | Rgba8Uint
            | Rgba8Sint | R32Float | R32Uint | R32Sint | Depth32Float => 4,

            Rgba16Float | Rgba16Uint | Rgba16Sint | Rg32Float | Rg32Uint | Rg32Sint => 8,

            Rgba32Float | Rgba32Uint | Rgba32Sint => 16,

            _ => return None,
        })
    }

    pub fn is_depth(format: wgpu::TextureFormat) -> bool {
        use wgpu::TextureFormat::*;
        match format {
            Depth16Unorm | Depth24Plus | Depth24PlusStencil8 | Depth32Float
            | Depth32FloatStencil8 | Stencil8 => true,

            _ => return false,
        }
    }
}
