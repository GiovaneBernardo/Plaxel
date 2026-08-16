use crate::prelude::*;

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

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum StepMode {
    Vertex,
    Instance,
}

#[derive(Debug, Clone, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VertexLayout {
    pub stride: u64,
    pub step_mode: StepMode,
    pub attributes: Vec<VertexAttribute>,
}

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VertexAttribute {
    pub offset: u64,
    pub shader_location: u32,
    pub format: AttributeFormat,
}

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AttributeFormat {
    Float32,
    Float32x2,
    Float32x3,
    Float32x4,
    Uint32,
    Uint8x4,
    Snorm8x4,
    Unorm8x4,
    // add as needed
}
