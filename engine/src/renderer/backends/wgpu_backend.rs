use crate::Arc;
use crate::Window;
use crate::assets;
use crate::assets::manager::AssetType;
use crate::assets::manager::Handle;
use crate::assets::material::MaterialResource;
use crate::assets::material::PipelineDescriptor;
use crate::assets::material::TextureAsset;
use crate::engine_info;
use crate::math::{UVec2, uvec2};
use crate::model::MeshAsset;
use crate::renderer::BindGroupHandle;
use crate::renderer::BufferDescriptor;
use crate::renderer::FrameBindings;
use crate::renderer::GpuMaterialData;
use crate::renderer::GraphResources;
use crate::renderer::PipelineKey;
use crate::renderer::PipelineTargetInfo;
use crate::renderer::SamplerDescriptor;
use crate::renderer::TextureDescriptor;
use crate::renderer::TextureSize;
use crate::renderer::ids::material_passes;
pub use crate::renderer::pool::*;
use crate::renderer::{
    AddressMode, AttachmentLoadOp, BindGroupEntry, BindingType, BufferUsages, FilterMode,
    GraphPassId, RenderNodeDescriptor, SamplerBorderColor, ShaderStages, TextureDimension,
    TextureFormat, TextureSampleType, TextureUsages,
};
use crate::renderer::{gpu::GpuArena, gpu_mesh::GpuMesh};
use crate::texture;
use wgpu::IndexFormat;
use wgpu::PipelineCache;
use wgpu::PipelineCacheDescriptor;

use super::{
    BindGroupLayoutHandle, BufferHandle, PipelineHandle, RenderGraph, RenderNode, TextureHandle,
};
use std::collections::HashMap;
use std::num::NonZeroU32;

pub use crate::renderer::backends::*;
use wgpu;

fn texture_extent_from_descriptor(
    size: winit::dpi::PhysicalSize<u32>,
    descriptor: &TextureDescriptor,
) -> (u32, u32) {
    let (width, height) = match descriptor.size {
        TextureSize::FullRes => (size.width, size.height),
        TextureSize::HalfRes => (size.width / 2, size.height / 2),
        TextureSize::QuarterRes => (size.width / 4, size.height / 4),
        TextureSize::Custom { width, height } => (width, height),
    };

    (width.max(1), height.max(1))
}

fn load_shader_source(shader_path: &str) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        assets::resources::embedded_shader_source(shader_path)
            .unwrap_or_else(|| panic!("shader is not embedded for wasm: {shader_path}"))
            .to_string()
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        pollster::block_on(assets::resources::load_string(shader_path)).unwrap()
    }
}

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
            TextureFormat::Bc1RgbaUnorm => wgpu::TextureFormat::Bc1RgbaUnorm,
            TextureFormat::Bc1RgbaUnormSrgb => wgpu::TextureFormat::Bc1RgbaUnormSrgb,
            TextureFormat::Bc2RgbaUnorm => wgpu::TextureFormat::Bc2RgbaUnorm,
            TextureFormat::Bc2RgbaUnormSrgb => wgpu::TextureFormat::Bc2RgbaUnormSrgb,
            TextureFormat::Bc3RgbaUnorm => wgpu::TextureFormat::Bc3RgbaUnorm,
            TextureFormat::Bc3RgbaUnormSrgb => wgpu::TextureFormat::Bc3RgbaUnormSrgb,
            TextureFormat::Bc4RUnorm => wgpu::TextureFormat::Bc4RUnorm,
            TextureFormat::Bc4RSnorm => wgpu::TextureFormat::Bc4RSnorm,
            TextureFormat::Bc5RgUnorm => wgpu::TextureFormat::Bc5RgUnorm,
            TextureFormat::Bc5RgSnorm => wgpu::TextureFormat::Bc5RgSnorm,
            TextureFormat::Bc6hRgbUfloat => wgpu::TextureFormat::Bc6hRgbUfloat,
            TextureFormat::Bc6hRgbFloat => wgpu::TextureFormat::Bc6hRgbFloat,
            TextureFormat::Bc7RgbaUnorm => wgpu::TextureFormat::Bc7RgbaUnorm,
            TextureFormat::Bc7RgbaUnormSrgb => wgpu::TextureFormat::Bc7RgbaUnormSrgb,
            TextureFormat::Etc2Rgb8Unorm => wgpu::TextureFormat::Etc2Rgb8Unorm,
            TextureFormat::Etc2Rgb8UnormSrgb => wgpu::TextureFormat::Etc2Rgb8UnormSrgb,
            TextureFormat::Etc2Rgb8A1Unorm => wgpu::TextureFormat::Etc2Rgb8A1Unorm,
            TextureFormat::Etc2Rgb8A1UnormSrgb => wgpu::TextureFormat::Etc2Rgb8A1UnormSrgb,
            TextureFormat::Etc2Rgba8Unorm => wgpu::TextureFormat::Etc2Rgba8Unorm,
            TextureFormat::Etc2Rgba8UnormSrgb => wgpu::TextureFormat::Etc2Rgba8UnormSrgb,
            TextureFormat::EacR11Unorm => wgpu::TextureFormat::EacR11Unorm,
            TextureFormat::EacR11Snorm => wgpu::TextureFormat::EacR11Snorm,
            TextureFormat::EacRg11Unorm => wgpu::TextureFormat::EacRg11Unorm,
            TextureFormat::EacRg11Snorm => wgpu::TextureFormat::EacRg11Snorm,
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

impl From<AddressMode> for wgpu::AddressMode {
    fn from(mode: AddressMode) -> wgpu::AddressMode {
        match mode {
            AddressMode::ClampToEdge => wgpu::AddressMode::ClampToEdge,
            AddressMode::Repeat => wgpu::AddressMode::Repeat,
            AddressMode::MirrorRepeat => wgpu::AddressMode::MirrorRepeat,
            AddressMode::ClampToBorder => wgpu::AddressMode::ClampToBorder,
        }
    }
}

impl From<FilterMode> for wgpu::FilterMode {
    fn from(mode: FilterMode) -> wgpu::FilterMode {
        match mode {
            FilterMode::Nearest => wgpu::FilterMode::Nearest,
            FilterMode::Linear => wgpu::FilterMode::Linear,
        }
    }
}

impl From<SamplerBorderColor> for wgpu::SamplerBorderColor {
    fn from(color: SamplerBorderColor) -> wgpu::SamplerBorderColor {
        match color {
            SamplerBorderColor::TransparentBlack => wgpu::SamplerBorderColor::TransparentBlack,
            SamplerBorderColor::OpaqueBlack => wgpu::SamplerBorderColor::OpaqueBlack,
            SamplerBorderColor::OpaqueWhite => wgpu::SamplerBorderColor::OpaqueWhite,
            SamplerBorderColor::Zero => wgpu::SamplerBorderColor::Zero,
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
                sample_type,
                multisampled,
            } => wgpu::BindingType::Texture {
                sample_type: match sample_type {
                    TextureSampleType::FloatFilterable => {
                        wgpu::TextureSampleType::Float { filterable: true }
                    }
                    TextureSampleType::FloatUnfilterable => {
                        wgpu::TextureSampleType::Float { filterable: false }
                    }
                    TextureSampleType::Depth => wgpu::TextureSampleType::Depth,
                    TextureSampleType::Uint => wgpu::TextureSampleType::Uint,
                    TextureSampleType::Sint => wgpu::TextureSampleType::Sint,
                },
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
use crate::renderer::{BlendMode, CompareFunction, CullMode, FrontFace, PolygonMode, Topology};

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

#[derive(Clone)]
struct ShaderHotReloadData {
    pipeline_descriptor: PipelineDescriptor,
    vertex_entry: String,
    fragment_entry: Option<String>,
    bind_group_layouts: Vec<BindGroupLayoutHandle>,
    target_info: PipelineTargetInfo,
    pipeline_handle: PipelineHandle,
}

#[derive(Eq, Hash, PartialEq)]
struct WgpuPipelineKey {
    pipeline: PipelineKey,
    bind_group_layouts: Vec<BindGroupLayoutHandle>,
}

pub struct WgpuBackend {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,
    supported_present_modes: Vec<wgpu::PresentMode>,
    surface: wgpu::Surface<'static>,
    depth_texture: texture::Texture,
    pipelines: HashMap<PipelineHandle, wgpu::RenderPipeline>,
    pipelines_by_uuid: HashMap<Uuid, PipelineHandle>,
    pipelines_by_key: HashMap<WgpuPipelineKey, PipelineHandle>,
    buffers: HashMap<BufferHandle, wgpu::Buffer>,
    bind_groups: HashMap<BindGroupHandle, wgpu::BindGroup>,
    bind_group_layouts: HashMap<BindGroupLayoutHandle, wgpu::BindGroupLayout>,
    textures: HashMap<TextureHandle, wgpu::Texture>,
    texture_views: HashMap<TextureHandle, wgpu::TextureView>,
    textures_by_uuid: HashMap<Uuid, TextureHandle>,
    materials_by_uuid: HashMap<Uuid, u32>,
    samplers: HashMap<SamplerHandle, wgpu::Sampler>,
    pool_manager: PoolManager,
    gpu_meshes: GpuArena<GpuMesh>,
    asset_gpu_meshes: HashMap<Handle<MeshAsset>, GpuMeshHandle>,
    shaders_hot_reload_data: HashMap<String, Vec<ShaderHotReloadData>>,
    white_texture: Option<TextureHandle>,
    default_sampler: Option<SamplerHandle>,
    dirty_global_textures: bool,
    dirty_global_materials: bool,
    uploaded_textures: Vec<TextureHandle>,
    uploaded_textures_last_available_index: u32,
    uploaded_materials: Vec<GpuMaterialData>,
    pipeline_cache: Option<PipelineCache>,
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

    fn draw_indirect(&mut self, buffer: BufferHandle, offset: u64) {
        self.pass
            .draw_indirect(self.backend.get_buffer(buffer).unwrap(), offset);
    }

    fn draw_indexed_indirect(&mut self, buffer: BufferHandle, offset: u64) {
        self.pass
            .draw_indexed_indirect(self.backend.get_buffer(buffer).unwrap(), offset);
    }

    fn multi_draw_indexed_indirect(&mut self, buffer: BufferHandle, offset: u64, count: u32) {
        self.pass.multi_draw_indexed_indirect(
            self.backend.get_buffer(buffer).unwrap(),
            offset,
            count,
        );
    }

    fn bind_vertex_buffer(&mut self, slot: u32, buffer: BufferHandle) {
        self.pass
            .set_vertex_buffer(slot, self.backend.get_buffer(buffer).unwrap().slice(..));
    }

    fn bind_vertex_buffer_range(
        &mut self,
        slot: u32,
        buffer: BufferHandle,
        offset: u64,
        size: u64,
    ) {
        let buffer = self.backend.get_buffer(buffer).unwrap();
        self.pass
            .set_vertex_buffer(slot, buffer.slice(offset..offset + size));
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

    fn compile(&mut self) {
        self.create_white_texture();
        self.default_sampler = Some(self.create_sampler(&SamplerDescriptor {
            label: "default_sampler".to_string(),
            address_mode_u: AddressMode::Repeat,
            address_mode_v: AddressMode::Repeat,
            address_mode_w: AddressMode::Repeat,
            anisotropy_clamp: 16,
            border_color: None,
            compare: None,
            lod_max_clamp: 32.0,
            lod_min_clamp: 0.0,
            mag_filter: FilterMode::default(),
            min_filter: FilterMode::default(),
            mipmap_filter: FilterMode::default(),
        }));

        self.uploaded_textures = vec![self.get_white_texture(); 512];
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.surface_config.width = width.max(1);
        self.surface_config.height = height.max(1);
        if let Err(error) = self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        }) {
            log::warn!("Unable to wait for GPU before surface resize: {error}");
        }
        self.surface.configure(&self.device, &self.surface_config);
        self.depth_texture = texture::Texture::create_depth_texture(
            &self.device,
            &self.surface_config,
            "depth_texture",
        );
    }

    fn toggle_present_mode(&mut self) {
        let next_mode = match self.surface_config.present_mode {
            wgpu::PresentMode::Mailbox => wgpu::PresentMode::Immediate,
            wgpu::PresentMode::Immediate => wgpu::PresentMode::Mailbox,
            _ if self
                .supported_present_modes
                .contains(&wgpu::PresentMode::Mailbox) =>
            {
                wgpu::PresentMode::Mailbox
            }
            _ => wgpu::PresentMode::Immediate,
        };

        if !self.supported_present_modes.contains(&next_mode) {
            log::warn!(
                "Cannot switch presentation mode from {:?} to {:?}: mode is unsupported",
                self.surface_config.present_mode,
                next_mode
            );
            return;
        }

        self.surface_config.present_mode = next_mode;
        self.surface.configure(&self.device, &self.surface_config);
        engine_info!("Presentation mode switched to {next_mode:?}");
    }

    fn resize_texture(&mut self, texture_handle: &TextureHandle, descriptor: &TextureDescriptor) {
        let (tex_width, tex_height) =
            texture_extent_from_descriptor(self.window.inner_size(), descriptor);

        let depth_or_array_layers = match descriptor.dimension {
            TextureDimension::Cube => 6,
            _ => 1,
        };

        let wgpu_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(descriptor.label.as_str()),
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

    fn compile_pipeline(&mut self, _node: &dyn RenderNode) -> PipelineHandle {
        PipelineHandle(0)
    }

    fn submit(&mut self, _graph: &RenderGraph) {}

    fn render(
        &mut self,
        render_graph: &mut RenderGraph,
        render_resources: &mut RenderResources,
        producers: &crate::renderer::RenderProducerRegistry,
        views: &crate::renderer::RenderViewRegistry,
    ) -> anyhow::Result<()> {
        crate::profile_scope!("wgpu.render");
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

        let _surface = &self.surface;

        let output = {
            crate::profile_scope!("wgpu.acquire_surface");
            self.surface.get_current_texture()?
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        // Check if the global textures arrays are dirty to update them
        {
            crate::profile_scope!("wgpu.update_global_resources");
            if self.dirty_global_textures {
                self.update_global_textures(render_resources);
            }

            if self.dirty_global_materials {
                self.update_global_materials(render_resources);
            }
        }

        let disabled_nodes = render_graph.disabled_nodes.clone();
        for (index, node) in render_graph.nodes.iter_mut() {
            if disabled_nodes.contains(index) {
                continue;
            }
            crate::profile_dynamic_scope!(
                "render.pass.run",
                format!("render.pass.run.{}", node.profile_name())
            );
            self.render_node(
                *index,
                node.as_mut(),
                &render_graph.resources,
                render_resources,
                &mut encoder,
                &view,
                producers,
                views,
            );
        }

        {
            crate::profile_scope!("wgpu.submit");
            self.queue.submit(std::iter::once(encoder.finish()));
        }
        crate::profile_counter!("render.nodes", render_graph.nodes.len() as f64);
        {
            crate::profile_scope!("wgpu.present");
            output.present();
        }
        Ok(())
    }

    fn reload_shader(&mut self, shader_path: &str) {
        let Some(reload_data) = self.shaders_hot_reload_data.get(shader_path).cloned() else {
            engine_info!("No pipelines registered for shader reload: {shader_path}");
            return;
        };

        for data in reload_data {
            let pipeline = self.create_render_pipeline(
                &data.pipeline_descriptor,
                &data.vertex_entry,
                data.fragment_entry.as_deref(),
                &data.bind_group_layouts,
                &data.target_info,
            );
            self.pipelines.insert(data.pipeline_handle, pipeline);
        }

        engine_info!(
            "Reloaded shader {:?} for {} pipeline(s)",
            shader_path,
            self.shaders_hot_reload_data
                .get(shader_path)
                .map_or(0, Vec::len)
        );
    }

    fn reload_shaders(&mut self) {
        let shader_paths: Vec<String> = self.shaders_hot_reload_data.keys().cloned().collect();

        for shader_path in shader_paths {
            self.reload_shader(&shader_path);
        }
    }

    // Load assets
    fn create_white_texture(&mut self) {
        let size = 64u32;
        self.white_texture = Some(self.create_texture(&TextureDescriptor {
            label: "white_texture".to_string(),
            format: TextureFormat::Rgba8Unorm,
            size: TextureSize::Custom {
                width: size,
                height: size,
            },
            dimension: TextureDimension::D2,
            usage: TextureUsages::COPY_SRC
                | TextureUsages::COPY_DST
                | TextureUsages::TEXTURE_BINDING,
            mip_levels: 1,
            sample_count: 1,
        }));

        let texture = self.get_texture(self.white_texture.unwrap()).unwrap();
        let white = vec![255u8; (size * size * 4) as usize];

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &white,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * size),
                rows_per_image: Some(size),
            },
            wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
        );
    }
    fn get_white_texture(&self) -> TextureHandle {
        self.white_texture.unwrap()
    }

    fn get_default_sampler(&self) -> SamplerHandle {
        self.default_sampler.unwrap()
    }

    fn load_texture_to_index(
        &mut self,
        path: &String,
        descriptor: &TextureDescriptor,
        index: Option<u32>,
    ) -> TextureHandle {
        // Load JPG from disk
        let img = image::open(path)
            .expect("Failed to load texture")
            .to_rgba8();

        let (width, height) = img.dimensions();

        let _depth_or_array_layers = match descriptor.dimension {
            TextureDimension::Cube => 6,
            _ => 1,
        };

        let depth_or_array_layers = match descriptor.dimension {
            TextureDimension::Cube => 6,
            _ => 1,
        };
        let maximum_mip_levels = u32::BITS - width.max(height).leading_zeros();
        let mip_level_count = descriptor.mip_levels.clamp(1, maximum_mip_levels);
        let wgpu_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(descriptor.label.as_str()),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers,
            },
            mip_level_count,
            sample_count: descriptor.sample_count,
            dimension: descriptor.dimension.into(),
            format: descriptor.format.into(),
            usage: descriptor.usage.into(),
            view_formats: &[],
        });

        for mip_level in 0..mip_level_count {
            let mip_width = (width >> mip_level).max(1);
            let mip_height = (height >> mip_level).max(1);
            let mip = if mip_level == 0 {
                img.clone()
            } else {
                image::imageops::resize(
                    &img,
                    mip_width,
                    mip_height,
                    image::imageops::FilterType::Lanczos3,
                )
            };
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &wgpu_texture,
                    mip_level,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                mip.as_raw(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * mip_width),
                    rows_per_image: Some(mip_height),
                },
                wgpu::Extent3d {
                    width: mip_width,
                    height: mip_height,
                    depth_or_array_layers,
                },
            );
        }

        let handle = self.add_texture(wgpu_texture);
        self.upload_texture(&handle, index);
        handle
    }

    fn load_material(&mut self, header: &crate::assets::manager::AssetHeader) -> Material {
        engine_info!("Loading material: {:?}", header);

        let pipeline_descriptor = PipelineDescriptor::new("shaders/cube.wgsl".to_string());
        let _pipeline_uuid = pipeline_descriptor.uuid;
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
        material_pass: MaterialPassId,
        bind_group_layouts: &[BindGroupLayoutHandle],
        target_info: &PipelineTargetInfo,
    ) -> PipelineHandle {
        let shader_pass = material.require_pass(material_pass);
        let descriptor = &shader_pass.pipeline;

        let key = Self::pipeline_key(
            descriptor,
            material_pass,
            &shader_pass.vertex_entry,
            shader_pass.fragment_entry.as_deref(),
            bind_group_layouts,
            target_info,
        );

        if let Some(handle) = self.pipelines_by_key.get(&key).copied() {
            self.pipelines_by_uuid.insert(descriptor.uuid, handle);
            return handle;
        }

        let pipeline = self.create_render_pipeline(
            descriptor,
            &shader_pass.vertex_entry,
            shader_pass.fragment_entry.as_deref(),
            bind_group_layouts,
            target_info,
        );

        let handle = self.add_render_pipeline(
            pipeline,
            descriptor,
            &shader_pass.vertex_entry,
            shader_pass.fragment_entry.as_deref(),
            bind_group_layouts,
            target_info,
        );

        self.pipelines_by_key.insert(key, handle);
        handle
    }

    fn target_info_for_pass(
        &self,
        descriptor: &RenderNodeDescriptor,
        resources: &GraphResources,
    ) -> PipelineTargetInfo {
        let color_formats = descriptor
            .color_attachments
            .iter()
            .map(|attachment| {
                self.attachment_format(attachment.name, resources)
                    .unwrap_or_else(|| {
                        panic!("Color attachment '{}' has no known format", attachment.name)
                    })
            })
            .collect();

        let depth_format = descriptor.depth_attachment.as_ref().map(|attachment| {
            self.attachment_format(attachment.name, resources)
                .unwrap_or_else(|| {
                    panic!("Depth attachment '{}' has no known format", attachment.name)
                })
        });

        PipelineTargetInfo {
            color_formats,
            depth_format,
            sample_count: 1,
        }
    }

    fn create_render_data(
        &mut self,
        vertex_bytes: &Vec<u8>, // How to turn a Vec of vertices into bytes: bytemuck::cast_slice(&positions_raw);
        indices: &Vec<u32>,
        material: Material,
        pipeline_handle: &PipelineHandle,
    ) -> RenderData {
        let mesh = MeshAsset {
            name: "Cube".to_string(),
            uuid: Uuid::new_v4(),
            vertices: vertex_bytes.clone(),
            indices: bytemuck::cast_slice(&indices).to_vec(),
            material_uuid: None,
            vertex_layout: material
                .require_pass(material_passes::FORWARD_OPAQUE)
                .pipeline
                .vertex_layouts[0]
                .clone(),
            //vertex_layout: VertexLayout {
            //    stride: std::mem::size_of::<[f32; 3]>() as u64,
            //    step_mode: crate::model::StepMode::Vertex,
            //    attributes: Vec::new(),
            //},
        };

        RenderData {
            mesh: self.load_mesh_with_data(&mesh),
            material,
            pipeline: *pipeline_handle,
            transform_index: 0,
            sort_key: 0,
            extra_bind_groups: Vec::new(),
        }
    }

    fn write_buffer(&mut self, buffer: BufferHandle, data: &[u8]) {
        let wgpu_buffer = self.get_buffer(buffer).unwrap();
        self.queue.write_buffer(wgpu_buffer, 0, data);
    }

    fn write_buffer_at(&mut self, buffer: BufferHandle, offset: u64, data: &[u8]) {
        let wgpu_buffer = self.get_buffer(buffer).unwrap();
        self.queue.write_buffer(wgpu_buffer, offset, data);
    }

    fn read_texture_bytes_at(&mut self, texture: &TextureHandle, x: f32, y: f32, out: &mut [u8]) {
        let wgpu_texture = self.get_texture(*texture).unwrap();
        let width = wgpu_texture.width();
        let height = wgpu_texture.height();

        let bytes_per_pixel = WgpuBackend::bytes_per_pixel(wgpu_texture.format()).unwrap();
        let unpadded_bytes_per_row = width * bytes_per_pixel;
        let padded_bytes_per_row =
            unpadded_bytes_per_row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);

        let buffer_size = padded_bytes_per_row as u64 * height as u64;

        // Create buffer used to turn gpu texture into cpu buffer
        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Output Buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        // Queue the copy texture to buffer
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Reader Encoder"),
            });

        let texture_size = wgpu::Extent3d {
            width: width,
            height: height,
            depth_or_array_layers: 1,
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

        // Immediately submit queue and read results to cpu
        self.queue.submit(std::iter::once(encoder.finish()));

        let buffer_slice = output_buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();

        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });

        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .unwrap();
        receiver.recv().unwrap().unwrap();

        let data = buffer_slice.get_mapped_range();

        // Send bytes to out[]
        let px = x as u32;
        let py = y as u32;

        let offset = (py * padded_bytes_per_row + px * bytes_per_pixel) as usize;
        let len = out.len().min(bytes_per_pixel as usize);

        out[..len].copy_from_slice(&data[offset..offset + len]);

        // Drop data
        drop(data);
        output_buffer.unmap();
    }

    fn upload_texture(&mut self, handle: &TextureHandle, index: Option<u32>) {
        if index.is_some() {
            self.uploaded_textures[index.unwrap() as usize] = *handle;
        } else {
            let index = self
                .find_first_empty_uploaded_texture_index(
                    self.uploaded_textures_last_available_index,
                )
                .unwrap_or(self.uploaded_textures.len() as u32);

            if index as usize == self.uploaded_textures.len() {
                self.uploaded_textures.push(*handle);
            } else {
                self.uploaded_textures[index as usize] = *handle;
            }

            self.uploaded_textures_last_available_index = self
                .find_first_empty_uploaded_texture_index(index + 1)
                .unwrap_or(self.uploaded_textures.len() as u32);
        }

        self.dirty_global_textures = true;
    }

    // TODO: UNLOAD TEXTURE FROM uploaded_textures

    fn upload_mesh(&mut self, upload: MeshUpload<'_>) -> Result<GpuMeshHandle, MeshUploadError> {
        if upload.vertices.is_empty() {
            return Err(MeshUploadError::EmptyVertices);
        }
        if upload.indices.is_empty() {
            return Err(MeshUploadError::EmptyIndices);
        }

        let stride_u64 = upload.vertex_layout.stride;
        let stride = usize::try_from(stride_u64)
            .ok()
            .filter(|stride| *stride > 0 && *stride <= u32::MAX as usize)
            .ok_or(MeshUploadError::InvalidVertexStride(stride_u64))?;
        if upload.vertices.len() % stride != 0 {
            return Err(MeshUploadError::MisalignedVertexData {
                bytes: upload.vertices.len(),
                stride,
            });
        }

        let vertex_count_usize = upload.vertices.len() / stride;
        let vertex_count = u32::try_from(vertex_count_usize)
            .map_err(|_| MeshUploadError::TooManyVertices(vertex_count_usize))?;
        let index_count = u32::try_from(upload.indices.len())
            .map_err(|_| MeshUploadError::TooManyIndices(upload.indices.len()))?;
        let stride_u32 = stride as u32;

        let layout_index = self.pool_manager.get_or_create_layout(upload.vertex_layout);
        let (vertex_page, vertex_allocation) = {
            let device = &self.device;
            let buffers = &mut self.buffers;
            let label = format!("{} vertex pool page", upload.label);
            let mut create = |capacity: u32| {
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&label),
                    size: capacity as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let handle = BufferHandle(buffers.len() as u32);
                buffers.insert(handle, buffer);
                handle
            };
            self.pool_manager
                .alloc_vertices(layout_index, vertex_count, stride_u32, &mut create)
        };

        let (index_page, index_allocation) = {
            let device = &self.device;
            let buffers = &mut self.buffers;
            let label = format!("{} index pool page", upload.label);
            let mut create = |capacity: u32| {
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&label),
                    size: capacity as u64,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let handle = BufferHandle(buffers.len() as u32);
                buffers.insert(handle, buffer);
                handle
            };
            self.pool_manager.alloc_indices(index_count, &mut create)
        };

        let pool = VertexPoolId {
            layout_index,
            page_index: vertex_page,
        };
        let vertex_buffer = self.pool_manager.vertex_buffer(pool);
        let index_buffer = self.pool_manager.index_buffer(index_page);
        let vertex_offset = vertex_allocation.offset as u64 * stride_u64;
        let index_offset = index_allocation.offset as u64 * std::mem::size_of::<u32>() as u64;

        self.queue.write_buffer(
            self.buffers.get(&vertex_buffer).unwrap(),
            vertex_offset,
            upload.vertices,
        );
        self.queue.write_buffer(
            self.buffers.get(&index_buffer).unwrap(),
            index_offset,
            bytemuck::cast_slice(upload.indices),
        );

        Ok(self.gpu_meshes.insert(GpuMesh {
            pool,
            vertex_allocation,
            index_page,
            index_allocation,
            draw_range: MeshDrawRange {
                first_index: index_allocation.offset,
                index_count,
                base_vertex: vertex_allocation.offset as i32,
            },
        }))
    }

    fn remove_mesh(&mut self, handle: GpuMeshHandle) -> bool {
        let Some(mesh) = self.gpu_meshes.remove(handle) else {
            return false;
        };
        self.pool_manager
            .free_vertices(mesh.pool, mesh.vertex_allocation);
        self.pool_manager
            .free_indices(mesh.index_page, mesh.index_allocation);
        true
    }

    fn remove_mesh_asset(&mut self, handle: Handle<MeshAsset>) -> bool {
        let Some(gpu_handle) = self.asset_gpu_meshes.remove(&handle) else {
            return false;
        };
        self.remove_mesh(gpu_handle)
    }

    fn get_gpu_mesh_binding(&mut self, handle: GpuMeshHandle) -> Option<GpuMeshBinding> {
        let mesh = self.gpu_meshes.get(handle)?;
        Some(GpuMeshBinding {
            vertex_buffer: self.pool_manager.vertex_buffer(mesh.pool),
            index_buffer: self.pool_manager.index_buffer(mesh.index_page),
            draw_range: mesh.draw_range,
        })
    }

    // Get using Uuids
    fn get_pipeline(&mut self, uuid: Uuid) -> Option<PipelineHandle> {
        self.pipelines_by_uuid.get(&uuid).cloned()
    }

    fn get_mesh_binding(&mut self, mesh: &Handle<MeshAsset>) -> Option<GpuMeshBinding> {
        let handle = self.asset_gpu_meshes.get(mesh).copied()?;
        self.get_gpu_mesh_binding(handle)
    }

    fn set_texture(&mut self, texture: &texture::Texture) {
        // ?????????????? I don't even know what set_texture is used for
        self.depth_texture = texture.clone();
    }

    fn get_texture_size(&self, handle: &TextureHandle) -> UVec2 {
        let Some(texture) = self.get_texture(*handle) else {
            return uvec2(0, 0);
        };

        uvec2(texture.size().width, texture.size().height)
    }

    fn get_surface_size(&self) -> UVec2 {
        uvec2(self.surface_config.width, self.surface_config.height)
    }

    fn upload_mesh_asset(&mut self, mesh: &MeshAsset) -> Handle<MeshAsset> {
        self.load_mesh_with_data(mesh)
    }

    fn create_texture_asset(&mut self, texture: &TextureAsset) -> TextureHandle {
        self.create_texture_asset_handle(texture)
    }

    fn upload_texture_asset(
        &mut self,
        texture: &TextureAsset,
        index: Option<u32>,
    ) -> TextureHandle {
        if index.is_none() {
            if let Some(handle) = self.textures_by_uuid.get(&texture.uuid).copied() {
                return handle;
            }
        }

        let handle = self.create_texture_asset_handle(texture);
        self.upload_texture(&handle, index);
        self.textures_by_uuid.insert(texture.uuid, handle);
        handle
    }

    fn is_texture_asset_uploaded(&self, uuid: Uuid) -> bool {
        self.textures_by_uuid.contains_key(&uuid)
    }

    fn upload_material_asset(&mut self, material: &Material, index: Option<u32>) -> u32 {
        let index = index.or_else(|| self.materials_by_uuid.get(&material.uuid).copied());

        let mut gpu_material = GpuMaterialData {
            diffuse_texture_index: 0,
            normal_texture_index: 0,
            roughness_texture_index: 0,
            flags: 0,
            base_color: [1.0, 0.0, 0.0, 1.0],
        };

        for binding in &material.bindings {
            let MaterialResource::Texture(uuid) = &binding.resource else {
                continue;
            };
            let Some(texture_index) = self.texture_index_for_uuid(*uuid) else {
                engine_info!(
                    "Material {:?} references texture {:?}, but it is not uploaded",
                    material.uuid,
                    uuid
                );
                continue;
            };

            match binding.name.as_str() {
                "diffuse" | "diffuse_texture" | "albedo" | "albedo_texture" => {
                    gpu_material.diffuse_texture_index = texture_index;
                }
                "normal" | "normal_map" | "normal_texture" => {
                    gpu_material.normal_texture_index = texture_index;
                    gpu_material.flags |= 1 << 0;
                }
                "roughness" | "roughness_texture" => {
                    gpu_material.roughness_texture_index = texture_index;
                    gpu_material.flags |= 1 << 1;
                }
                _ => {
                    gpu_material.diffuse_texture_index = texture_index;
                }
            }
        }

        let material_index = if let Some(index) = index {
            let material_index = index;
            let index = material_index as usize;
            if index >= self.uploaded_materials.len() {
                self.uploaded_materials
                    .resize(index + 1, GpuMaterialData::default());
            }
            self.uploaded_materials[index] = gpu_material;
            material_index
        } else {
            let material_index = self.uploaded_materials.len() as u32;
            self.uploaded_materials.push(gpu_material);
            material_index
        };

        self.dirty_global_materials = true;
        self.materials_by_uuid.insert(material.uuid, material_index);
        material_index
    }

    fn create_texture(&mut self, descriptor: &TextureDescriptor) -> TextureHandle {
        let (tex_width, tex_height) =
            texture_extent_from_descriptor(self.window.inner_size(), descriptor);

        let depth_or_array_layers = match descriptor.dimension {
            TextureDimension::Cube => 6,
            _ => 1,
        };

        let wgpu_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(descriptor.label.as_str()),
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

    fn create_sampler(&mut self, descriptor: &SamplerDescriptor) -> SamplerHandle {
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some(descriptor.label.as_str()),
            address_mode_u: descriptor.address_mode_u.into(),
            address_mode_v: descriptor.address_mode_v.into(),
            address_mode_w: descriptor.address_mode_w.into(),
            mag_filter: descriptor.mag_filter.into(),
            min_filter: descriptor.min_filter.into(),
            mipmap_filter: descriptor.mipmap_filter.into(),
            lod_min_clamp: descriptor.lod_min_clamp,
            lod_max_clamp: descriptor.lod_max_clamp,
            compare: descriptor.compare.map(Into::into),
            anisotropy_clamp: descriptor.anisotropy_clamp,
            border_color: descriptor.border_color.map(Into::into),
        });
        self.add_sampler(sampler)
    }

    fn create_buffer(&mut self, descriptor: &BufferDescriptor) -> BufferHandle {
        let wgpu_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(descriptor.label.as_str()),
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
                count: entry.count.and_then(NonZeroU32::new),
            })
            .collect();

        let wgpu_bind_group_layout =
            self.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some(descriptor.label.as_str()),
                    entries: &wgpu_entries,
                });

        self.add_bind_group_layout(wgpu_bind_group_layout)
    }

    fn create_bind_group(&mut self, descriptor: &BindGroupDescriptor) -> BindGroupHandle {
        let layout = self.get_bind_group_layout(descriptor.layout).unwrap();

        let mut texture_view_arrays: Vec<Vec<&wgpu::TextureView>> = Vec::new();

        for (_, entry) in &descriptor.entries {
            if let BindGroupEntry::TextureArray(texture_handles) = entry {
                let views = texture_handles
                    .iter()
                    .map(|handle| self.get_texture_view(**handle).unwrap())
                    .collect::<Vec<_>>();

                texture_view_arrays.push(views);
            }
        }

        let mut texture_array_index = 0;

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
                    BindGroupEntry::Sampler(handle) => {
                        wgpu::BindingResource::Sampler(self.get_sampler(*handle).unwrap())
                    }
                    BindGroupEntry::TextureArray(_) => {
                        let views = &texture_view_arrays[texture_array_index];
                        texture_array_index += 1;
                        wgpu::BindingResource::TextureViewArray(views)
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
            label: Some(descriptor.label.as_str()),
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
        let gpu_handle = self
            .upload_mesh(MeshUpload {
                label: &mesh.name,
                vertices: &mesh.vertices,
                indices: &mesh.indices,
                vertex_layout: &mesh.vertex_layout,
            })
            .unwrap_or_else(|error| panic!("failed to upload mesh '{}': {error}", mesh.name));
        if let Some(previous) = self.asset_gpu_meshes.insert(handle, gpu_handle) {
            self.remove_mesh(previous);
        }
        handle
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
    fn color_load_op(load_op: AttachmentLoadOp) -> wgpu::LoadOp<wgpu::Color> {
        match load_op {
            AttachmentLoadOp::Load => wgpu::LoadOp::Load,
            AttachmentLoadOp::ClearColor([r, g, b, a]) => {
                wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a })
            }
            AttachmentLoadOp::ClearDepth(_) => {
                panic!("ClearDepth cannot be used for a color attachment")
            }
        }
    }

    fn depth_load_op(load_op: AttachmentLoadOp) -> wgpu::LoadOp<f32> {
        match load_op {
            AttachmentLoadOp::Load => wgpu::LoadOp::Load,
            AttachmentLoadOp::ClearDepth(depth) => wgpu::LoadOp::Clear(depth),
            AttachmentLoadOp::ClearColor(_) => {
                panic!("ClearColor cannot be used for a depth attachment")
            }
        }
    }

    fn attachment_view<'a>(
        &'a self,
        name: &str,
        resources: &GraphResources,
        swapchain_view: &'a wgpu::TextureView,
    ) -> Option<&'a wgpu::TextureView> {
        if name == "swapchain_image" {
            return Some(swapchain_view);
        }

        let handle = resources.texture(name)?;
        self.get_texture_view(*handle)
    }

    fn attachment_format(&self, name: &str, resources: &GraphResources) -> Option<TextureFormat> {
        if name == "swapchain_image" {
            return Some(Self::texture_format_from_wgpu(self.surface_config.format));
        }

        let handle = resources.texture(name)?;
        let texture = self.get_texture(*handle)?;
        Some(Self::texture_format_from_wgpu(texture.format()))
    }

    fn texture_format_from_wgpu(format: wgpu::TextureFormat) -> TextureFormat {
        match format {
            wgpu::TextureFormat::Depth32Float => TextureFormat::Depth32Float,
            wgpu::TextureFormat::Depth24PlusStencil8 => TextureFormat::Depth24PlusStencil8,
            wgpu::TextureFormat::Depth24Plus => TextureFormat::Depth24Plus,
            wgpu::TextureFormat::Depth16Unorm => TextureFormat::Depth16Unorm,
            wgpu::TextureFormat::Depth32FloatStencil8 => TextureFormat::Depth32FloatStencil8,
            wgpu::TextureFormat::Stencil8 => TextureFormat::Stencil8,
            wgpu::TextureFormat::Rgba8Unorm => TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Rgba8UnormSrgb => TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureFormat::Bgra8Unorm => TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Bgra8UnormSrgb => TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Rgba16Float => TextureFormat::Rgba16Float,
            wgpu::TextureFormat::Rgba32Float => TextureFormat::Rgba32Float,
            wgpu::TextureFormat::Rg32Float => TextureFormat::Rg32Float,
            wgpu::TextureFormat::R32Float => TextureFormat::R32Float,
            other => panic!("Unsupported render target texture format: {other:?}"),
        }
    }

    fn pipeline_key(
        descriptor: &PipelineDescriptor,
        material_pass: MaterialPassId,
        vertex_entry: &str,
        fragment_entry: Option<&str>,
        bind_group_layouts: &[BindGroupLayoutHandle],
        target_info: &PipelineTargetInfo,
    ) -> WgpuPipelineKey {
        WgpuPipelineKey {
            pipeline: PipelineKey {
                material_pass,
                shader: descriptor.shader.clone(),
                vertex_entry: vertex_entry.to_owned(),
                fragment_entry: fragment_entry.map(str::to_owned),

                blend_mode: descriptor.blend_mode,
                cull_mode: descriptor.cull_mode,
                topology: descriptor.topology,
                front_face: descriptor.front_face,
                polygon_mode: descriptor.polygon_mode,
                depth_state: descriptor.depth_state,
                multisample_count: descriptor.multisample.count,
                vertex_layouts: descriptor.vertex_layouts.clone(),

                color_formats: target_info.color_formats.clone(),
                depth_format: target_info.depth_format,
            },
            bind_group_layouts: bind_group_layouts.to_vec(),
        }
    }

    pub async fn new(window: Arc<Window>) -> anyhow::Result<Self> {
        let size = window.inner_size();

        let mut flags = wgpu::InstanceFlags::default();
        flags.remove(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL);

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            #[cfg(all(not(target_arch = "wasm32"), target_os = "windows"))]
            backends: wgpu::Backends::DX12,
            flags,
            #[cfg(all(not(target_arch = "wasm32"), not(target_os = "windows")))]
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

        let info = adapter.get_info();
        log::info!("adapter backend: {:?}", info.backend);
        log::info!("adapter name: {}", info.name);
        log::info!("features: {:?}", adapter.features());
        log::info!(
            "max binding array elements: {}",
            adapter.limits().max_binding_array_elements_per_shader_stage
        );

        let supported_features = adapter.features();
        let limits = adapter.limits();

        log::info!("features: {:?}", supported_features);
        log::info!(
            "max binding array elements: {}",
            limits.max_binding_array_elements_per_shader_stage
        );

        let supported_features = adapter.features();
        let wanted_features = wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::PARTIALLY_BOUND_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
            | wgpu::Features::INDIRECT_FIRST_INSTANCE
            | wgpu::Features::PIPELINE_CACHE;
        let enabled_features = wanted_features & supported_features;

        if !supported_features.contains(wgpu::Features::INDIRECT_FIRST_INSTANCE) {
            anyhow::bail!("the renderer requires INDIRECT_FIRST_INSTANCE for terrain batching");
        }

        let _has_texture_binding_array =
            enabled_features.contains(wgpu::Features::TEXTURE_BINDING_ARRAY);

        let _has_partially_bound =
            enabled_features.contains(wgpu::Features::PARTIALLY_BOUND_BINDING_ARRAY);

        let adapter_limits = adapter.limits();

        let mut required_limits = wgpu::Limits::default();
        required_limits.max_binding_array_elements_per_shader_stage = adapter_limits
            .max_binding_array_elements_per_shader_stage
            .min(512);
        if !cfg!(target_arch = "wasm32") {
            // Native-only escape hatch for very large procedural worlds. This
            // may not be portable to browser WebGPU; the mesh pool should
            // eventually free old chunk meshes and cap pages per target.
            required_limits.max_buffer_size = adapter_limits.max_buffer_size;
            log::warn!(
                "requesting native adapter max_buffer_size={} bytes; large mesh pools may not work on web",
                required_limits.max_buffer_size
            );
        }

        unsafe {
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: None,
                    required_features: enabled_features,
                    experimental_features: wgpu::ExperimentalFeatures::enabled(),
                    required_limits,
                    memory_hints: Default::default(),
                    trace: wgpu::Trace::Off,
                })
                .await?;

            let wanted_texture_slots = 512;
            let adapter_limits = adapter.limits();

            let max_supported = adapter_limits.max_binding_array_elements_per_shader_stage;

            if max_supported < wanted_texture_slots {
                anyhow::bail!(
                    "Texture binding arrays need {wanted_texture_slots} slots, but this adapter only supports {max_supported}"
                );
            }

            if !supported_features.contains(wgpu::Features::TEXTURE_BINDING_ARRAY) {
                anyhow::bail!("This GPU/backend does not support TEXTURE_BINDING_ARRAY");
            }

            let surface_caps = surface.get_capabilities(&adapter);
            let surface_format = surface_caps
                .formats
                .iter()
                .find(|f| f.is_srgb())
                .copied()
                .unwrap_or(surface_caps.formats[0]);

            engine_info!("PRESENT MODE: {:?}", surface_caps.present_modes[0]);
            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: surface_format,
                width: size.width.max(1),
                height: size.height.max(1),
                present_mode: surface_caps.present_modes[0],
                alpha_mode: surface_caps.alpha_modes[0],
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            };

            let depth_texture =
                texture::Texture::create_depth_texture(&device, &config, "depth_texture");

            let pipeline_cache = enabled_features
                .contains(wgpu::Features::PIPELINE_CACHE)
                .then(|| {
                    device.create_pipeline_cache(&PipelineCacheDescriptor {
                        label: Some("pipeline_cache"),
                        data: None,
                        fallback: true,
                    })
                });

            Ok(Self {
                window,
                device,
                queue,
                surface,
                surface_config: config,
                supported_present_modes: surface_caps.present_modes,
                depth_texture,
                pipelines: HashMap::new(),
                pipelines_by_uuid: HashMap::new(),
                pipelines_by_key: HashMap::new(),
                buffers: HashMap::new(),
                bind_groups: HashMap::new(),
                bind_group_layouts: HashMap::new(),
                textures: HashMap::new(),
                texture_views: HashMap::new(),
                textures_by_uuid: HashMap::new(),
                materials_by_uuid: HashMap::new(),
                samplers: HashMap::new(),
                pool_manager: PoolManager::new(),
                gpu_meshes: GpuArena::new(),
                asset_gpu_meshes: HashMap::new(),
                shaders_hot_reload_data: HashMap::new(),
                white_texture: None,
                default_sampler: None,
                dirty_global_textures: true,
                dirty_global_materials: true,
                uploaded_textures: Vec::new(),
                uploaded_textures_last_available_index: 0,
                uploaded_materials: Vec::new(),
                pipeline_cache,
            })
        }
    }

    fn render_node(
        &mut self,
        graph_pass: GraphPassId,
        node: &mut dyn RenderNode,
        resources: &GraphResources,
        render_resources: &RenderResources,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        producers: &crate::renderer::RenderProducerRegistry,
        views: &crate::renderer::RenderViewRegistry,
    ) {
        crate::profile_scope!("wgpu.render_node");
        let render_node_descriptor = node.describe_pass();
        let mut color_attachments: Vec<Option<wgpu::RenderPassColorAttachment<'_>>> =
            Vec::with_capacity(render_node_descriptor.color_attachments.len());

        for attachment in &render_node_descriptor.color_attachments {
            let attachment_view = self
                .attachment_view(attachment.name, resources, view)
                .unwrap_or_else(|| panic!("Color attachment '{}' was not found", attachment.name));
            color_attachments.push(Some(wgpu::RenderPassColorAttachment {
                view: attachment_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: Self::color_load_op(attachment.load_op),
                    store: if attachment.store {
                        wgpu::StoreOp::Store
                    } else {
                        wgpu::StoreOp::Discard
                    },
                },
                depth_slice: None,
            }));
        }

        let depth_stencil_attachment =
            render_node_descriptor
                .depth_attachment
                .as_ref()
                .map(|attachment| {
                    let attachment_view = self
                        .attachment_view(attachment.name, resources, view)
                        .unwrap_or_else(|| {
                            panic!("Depth attachment '{}' was not found", attachment.name)
                        });

                    wgpu::RenderPassDepthStencilAttachment {
                        view: attachment_view,
                        depth_ops: Some(wgpu::Operations {
                            load: Self::depth_load_op(attachment.load_op),
                            store: if attachment.store {
                                wgpu::StoreOp::Store
                            } else {
                                wgpu::StoreOp::Discard
                            },
                        }),
                        stencil_ops: None,
                    }
                });

        if color_attachments.len() == 0 && depth_stencil_attachment.is_none() {
            return;
        }

        let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
        node.run(&mut ctx, render_resources);
        producers.record_pass(graph_pass, views, &mut ctx, render_resources);

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

    pub fn update_global_textures(&mut self, render_resources: &mut RenderResources) {
        let frame_bindings = render_resources
            .get_labeled_mut::<FrameBindings>("frame_bindings")
            .unwrap();

        let clone = self.uploaded_textures.clone();
        let texture_refs: Vec<&TextureHandle> = clone.iter().collect();

        let sampler = self.get_default_sampler();
        frame_bindings.materials_bind_group = self.create_bind_group(&BindGroupDescriptor {
            label: "materials_bind_group".to_string(),
            layout: frame_bindings.textures_layout.clone(),
            entries: vec![
                (0, BindGroupEntry::TextureArray(&texture_refs)),
                (1, BindGroupEntry::Sampler(sampler)),
                (
                    2,
                    BindGroupEntry::Buffer(frame_bindings.materials_ssbo_buffer),
                ),
            ],
        });
        self.dirty_global_textures = false;
    }

    pub fn update_global_materials(&mut self, render_resources: &mut RenderResources) {
        let frame_bindings = render_resources
            .get_labeled_mut::<FrameBindings>("frame_bindings")
            .unwrap();

        let clone = self.uploaded_materials.clone();
        self.write_buffer(
            frame_bindings.materials_ssbo_buffer,
            &bytemuck::cast_slice(clone.as_slice()),
        );
        self.dirty_global_materials = false;
    }

    pub fn find_first_empty_uploaded_texture_index(&self, start_index: u32) -> Option<u32> {
        let white_texture = self.white_texture?;

        self.uploaded_textures
            .iter()
            .enumerate()
            .skip(start_index as usize)
            .find_map(|(index, texture)| {
                if *texture == white_texture {
                    Some(index as u32)
                } else {
                    None
                }
            })
    }

    fn texture_index_for_uuid(&self, uuid: Uuid) -> Option<u32> {
        let handle = self.textures_by_uuid.get(&uuid)?;
        self.uploaded_textures
            .iter()
            .position(|uploaded| uploaded == handle)
            .map(|index| index as u32)
    }

    fn create_render_pipeline(
        &self,
        descriptor: &PipelineDescriptor,
        vertex_entry: &str,
        fragment_entry: Option<&str>,
        bind_group_layouts: &[BindGroupLayoutHandle],
        target_info: &PipelineTargetInfo,
    ) -> wgpu::RenderPipeline {
        engine_info!("Shader name: {:?}", descriptor.shader);
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Shader"),
                source: wgpu::ShaderSource::Wgsl(load_shader_source(&descriptor.shader).into()),
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

        let desc = descriptor;

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

        let depth_stencil = desc
            .depth_state
            .and_then(|ds| target_info.depth_format.map(|format| (ds, format)))
            .map(|(ds, format)| wgpu::DepthStencilState {
                format: format.into(),
                depth_write_enabled: ds.write_enabled,
                depth_compare: ds.compare.into(),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            });

        let color_targets: Vec<Option<wgpu::ColorTargetState>> = target_info
            .color_formats
            .iter()
            .map(|format| {
                Some(wgpu::ColorTargetState {
                    format: (*format).into(),
                    blend: blend_mode_to_wgpu(desc.blend_mode),
                    write_mask: wgpu::ColorWrites::ALL,
                })
            })
            .collect();

        self.device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&descriptor.shader),
                layout: Some(&render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex_entry),
                    buffers: &vertex_buffer_layouts,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: fragment_entry.map(|entry_point| wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry_point),
                    targets: &color_targets,
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
                    count: target_info.sample_count,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
                cache: self.pipeline_cache.as_ref(),
            })
    }

    fn _get_render_pipeline(
        &self,
        pipeline_handle: PipelineHandle,
    ) -> Option<&wgpu::RenderPipeline> {
        self.pipelines.get(&pipeline_handle)
    }

    fn add_render_pipeline(
        &mut self,
        pipeline: wgpu::RenderPipeline,
        pipeline_descriptor: &PipelineDescriptor,
        vertex_entry: &str,
        fragment_entry: Option<&str>,
        bind_group_layouts: &[BindGroupLayoutHandle],
        target_info: &PipelineTargetInfo,
    ) -> PipelineHandle {
        let handle = PipelineHandle(self.pipelines.len() as u32);
        self.pipelines.insert(handle, pipeline);
        self.pipelines_by_uuid
            .insert(pipeline_descriptor.uuid, handle);
        self.shaders_hot_reload_data
            .entry(pipeline_descriptor.shader.clone())
            .or_insert_with(Vec::new)
            .push(ShaderHotReloadData {
                pipeline_descriptor: pipeline_descriptor.clone(),
                vertex_entry: vertex_entry.to_owned(),
                fragment_entry: fragment_entry.map(str::to_owned),
                bind_group_layouts: bind_group_layouts.to_vec(),
                target_info: target_info.clone(),
                pipeline_handle: handle,
            });
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

    pub fn add_sampler(&mut self, sampler: wgpu::Sampler) -> SamplerHandle {
        let handle = SamplerHandle(self.samplers.len() as u32);
        self.samplers.insert(handle, sampler);
        handle
    }

    fn get_sampler(&self, handle: SamplerHandle) -> Option<&wgpu::Sampler> {
        self.samplers.get(&handle)
    }

    fn get_texture_view(&self, handle: TextureHandle) -> Option<&wgpu::TextureView> {
        self.texture_views.get(&handle)
    }

    fn create_texture_asset_handle(&mut self, texture: &TextureAsset) -> TextureHandle {
        let mip = texture
            .mip_levels
            .first()
            .expect("TextureAsset must contain at least one mip level");
        let format: wgpu::TextureFormat = texture.format.into();
        let bytes_per_pixel =
            WgpuBackend::bytes_per_pixel(format).expect("unsupported texture format");

        let wgpu_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(texture.name.as_str()),
            size: wgpu::Extent3d {
                width: texture.width,
                height: texture.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: texture.mip_levels.len().max(1) as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &wgpu_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &mip.bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_pixel * mip.width),
                rows_per_image: Some(mip.height),
            },
            wgpu::Extent3d {
                width: mip.width,
                height: mip.height,
                depth_or_array_layers: 1,
            },
        );

        self.add_texture(wgpu_texture)
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
