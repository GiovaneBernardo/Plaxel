use crate::assets::manager::Asset;
use crate::assets::manager::AssetType;
use crate::model::ModelVertex;
use crate::model::TransformInstance;
use crate::model::Vertex;
use crate::model::VertexLayout;
use crate::renderer::BlendMode;
use crate::renderer::CompareFunction;
use crate::renderer::CullMode;
use crate::renderer::DepthState;
use crate::renderer::FrontFace;
use crate::renderer::MultisampleState;
use crate::renderer::PolygonMode;
use crate::renderer::SamplerDescriptor;
use crate::renderer::TextureFormat;
use crate::renderer::Topology;
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Material {
    pub uuid: Uuid,
    pub pipeline_descriptor: PipelineDescriptor,
    pub bindings: Vec<MaterialBinding>,     // For bound resources
    pub parameters: Vec<MaterialParameter>, // For when not using textures, e.g. diffuse_color as float4 instead of texture
    #[serde(skip)]
    pub material_index: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MaterialBinding {
    pub name: String, // "diffuse_texture", "normal_map", "terrain_textures"
    pub binding: u32, // shader binding index
    pub group: u32,   // usually material group
    pub resource: MaterialResource,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MaterialResource {
    Texture(Uuid),
    TextureArray(Vec<Uuid>),
    Sampler(SamplerDescriptor),
    Buffer(Uuid),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MaterialParameter {
    pub name: String,
    pub value: MaterialValue,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MaterialValue {
    Float(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Int(i32),
    Uint(u32),
    Bool(bool),
}

impl Asset for Material {
    const ASSET_TYPE: AssetType = AssetType::Material;
    fn uuid(&self) -> Uuid {
        self.uuid
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PipelineDescriptor {
    pub uuid: Uuid,
    pub shader: String,
    pub vertex_layouts: Vec<VertexLayout>,
    pub blend_mode: BlendMode,
    pub cull_mode: CullMode,
    pub topology: Topology,
    pub front_face: FrontFace,
    pub polygon_mode: PolygonMode,
    pub depth_state: Option<DepthState>,
    pub multisample: MultisampleState,
}

impl PipelineDescriptor {
    pub fn new(shader: String) -> Self {
        Self {
            uuid: Uuid::new_v4(),
            shader,
            vertex_layouts: vec![ModelVertex::layout(), TransformInstance::layout()],
            blend_mode: BlendMode::Replace,
            cull_mode: CullMode::None,
            topology: Topology::TriangleList,
            front_face: FrontFace::Ccw,
            polygon_mode: PolygonMode::Fill,
            depth_state: Some(DepthState {
                write_enabled: true,
                compare: CompareFunction::Greater,
            }),
            multisample: MultisampleState { count: 1 },
        }
    }
}

impl Material {
    pub fn new(shader: String) -> Self {
        let pipeline_descriptor = PipelineDescriptor::new(shader);

        Self {
            uuid: Uuid::new_v4(),
            pipeline_descriptor,
            bindings: Vec::new(),
            parameters: Vec::new(),
            material_index: 0,
        }
    }

    pub fn with_vertex_layouts(mut self, layouts: Vec<VertexLayout>) -> Self {
        self.pipeline_descriptor.vertex_layouts = layouts;
        self
    }

    pub fn with_blend(mut self, blend: BlendMode) -> Self {
        self.pipeline_descriptor.blend_mode = blend;
        self
    }

    pub fn with_cull(mut self, cull: CullMode) -> Self {
        self.pipeline_descriptor.cull_mode = cull;
        self
    }

    pub fn with_topology(mut self, topology: Topology) -> Self {
        self.pipeline_descriptor.topology = topology;
        self
    }

    pub fn with_front_face(mut self, front_face: FrontFace) -> Self {
        self.pipeline_descriptor.front_face = front_face;
        self
    }

    pub fn with_polygon_mode(mut self, polygon_mode: PolygonMode) -> Self {
        self.pipeline_descriptor.polygon_mode = polygon_mode;
        self
    }

    pub fn with_depth(mut self, depth: Option<DepthState>) -> Self {
        self.pipeline_descriptor.depth_state = depth;
        self
    }

    pub fn with_multisample(mut self, count: u32) -> Self {
        self.pipeline_descriptor.multisample = MultisampleState { count };
        self
    }
}

impl Default for Material {
    fn default() -> Self {
        Self::new("shaders/cube.wgsl".to_string())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TextureAsset {
    pub uuid: Uuid,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub format: TextureFormat,
    pub mip_levels: Vec<TextureMip>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TextureMip {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TextureCompression {
    None,
    Bc1,
    Bc3,
    Bc7,
    Astc,
}

impl Asset for TextureAsset {
    const ASSET_TYPE: AssetType = AssetType::Texture;
    fn uuid(&self) -> Uuid {
        self.uuid
    }
}
