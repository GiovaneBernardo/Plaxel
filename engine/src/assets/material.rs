use std::collections::HashMap;

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
use crate::renderer::ids::{MaterialPassId, material_passes};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Material {
    pub uuid: Uuid,
    pub technique: MaterialTechnique,
    pub bindings: Vec<MaterialBinding>,     // For bound resources
    pub parameters: Vec<MaterialParameter>, // For when not using textures, e.g. diffuse_color as float4 instead of texture
    #[serde(skip)]
    pub material_index: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MaterialTechnique {
    pub uuid: Uuid,
    pub name: String,
    pub passes: HashMap<MaterialPassId, ShaderPass>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShaderPass {
    pub vertex_entry: String,
    pub fragment_entry: Option<String>,
    pub pipeline: PipelineDescriptor,
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

impl MaterialTechnique {
    pub fn pass(&self, id: MaterialPassId) -> Option<&ShaderPass> {
        self.passes.get(&id)
    }

    pub fn pass_mut(&mut self, id: MaterialPassId) -> Option<&mut ShaderPass> {
        self.passes.get_mut(&id)
    }
}

impl Material {
    pub fn new(shader: String) -> Self {
        let mut passes = HashMap::new();
        for pass_id in [
            material_passes::FORWARD_OPAQUE,
            material_passes::DEPTH_ONLY,
            material_passes::SHADOW,
            material_passes::DEBUG,
            material_passes::FULLSCREEN,
        ] {
            passes.insert(
                pass_id,
                ShaderPass {
                    vertex_entry: "vs_main".into(),
                    fragment_entry: (!matches!(
                        pass_id,
                        material_passes::DEPTH_ONLY | material_passes::SHADOW
                    ))
                    .then(|| "fs_main".into()),
                    pipeline: PipelineDescriptor::new(shader.clone()),
                },
            );
        }

        Self {
            uuid: Uuid::new_v4(),
            technique: MaterialTechnique {
                uuid: Uuid::new_v4(),
                name: shader,
                passes,
            },
            bindings: Vec::new(),
            parameters: Vec::new(),
            material_index: 0,
        }
    }

    pub fn pass(&self, id: MaterialPassId) -> Option<&ShaderPass> {
        self.technique.pass(id)
    }

    /// Creates a deliberately single-pass material. This is useful for specialized surfaces
    /// such as water that must opt out of the normal depth/shadow routes.
    pub fn for_pass(shader: String, pass_id: MaterialPassId) -> Self {
        let name = shader.clone();
        let fragment_entry = (!matches!(
            pass_id,
            material_passes::DEPTH_ONLY | material_passes::SHADOW
        ))
        .then(|| "fs_main".into());
        Self {
            uuid: Uuid::new_v4(),
            technique: MaterialTechnique {
                uuid: Uuid::new_v4(),
                name,
                passes: HashMap::from([(
                    pass_id,
                    ShaderPass {
                        vertex_entry: "vs_main".into(),
                        fragment_entry,
                        pipeline: PipelineDescriptor::new(shader),
                    },
                )]),
            },
            bindings: Vec::new(),
            parameters: Vec::new(),
            material_index: 0,
        }
    }

    /// Adds a pass variant by cloning the first existing pipeline state. Call this after the
    /// common material builders when the new pass needs to inherit their layouts and state.
    pub fn with_pass_variant(
        mut self,
        id: MaterialPassId,
        vertex_entry: impl Into<String>,
        fragment_entry: Option<String>,
    ) -> Self {
        let mut pipeline = self
            .technique
            .passes
            .values()
            .next()
            .map(|pass| pass.pipeline.clone())
            .expect("a material must have at least one pass before cloning a variant");
        pipeline.uuid = Uuid::new_v4();
        self.insert_pass(
            id,
            ShaderPass {
                vertex_entry: vertex_entry.into(),
                fragment_entry,
                pipeline,
            },
        );
        self
    }

    pub fn supports_pass(&self, id: MaterialPassId) -> bool {
        self.technique.passes.contains_key(&id)
    }

    pub fn require_pass(&self, id: MaterialPassId) -> &ShaderPass {
        self.pass(id).unwrap_or_else(|| {
            panic!(
                "material technique '{}' does not implement pass {id}",
                self.technique.name
            )
        })
    }

    pub fn insert_pass(&mut self, id: MaterialPassId, pass: ShaderPass) -> Option<ShaderPass> {
        self.technique.passes.insert(id, pass)
    }

    pub fn remove_pass(&mut self, id: MaterialPassId) -> Option<ShaderPass> {
        self.technique.passes.remove(&id)
    }

    pub fn configure_pass(
        &mut self,
        id: MaterialPassId,
        configure: impl FnOnce(&mut ShaderPass),
    ) -> bool {
        let Some(pass) = self.technique.pass_mut(id) else {
            return false;
        };
        configure(pass);
        true
    }

    pub fn with_vertex_layouts(mut self, layouts: Vec<VertexLayout>) -> Self {
        for pass in self.technique.passes.values_mut() {
            pass.pipeline.vertex_layouts = layouts.clone();
        }
        self
    }

    pub fn set_vertex_layouts(&mut self, layouts: Vec<VertexLayout>) {
        for pass in self.technique.passes.values_mut() {
            pass.pipeline.vertex_layouts = layouts.clone();
        }
    }

    pub fn with_blend(mut self, blend: BlendMode) -> Self {
        for pass in self.technique.passes.values_mut() {
            pass.pipeline.blend_mode = blend;
        }
        self
    }

    pub fn with_cull(mut self, cull: CullMode) -> Self {
        for pass in self.technique.passes.values_mut() {
            pass.pipeline.cull_mode = cull;
        }
        self
    }

    pub fn with_topology(mut self, topology: Topology) -> Self {
        for pass in self.technique.passes.values_mut() {
            pass.pipeline.topology = topology;
        }
        self
    }

    pub fn with_front_face(mut self, front_face: FrontFace) -> Self {
        for pass in self.technique.passes.values_mut() {
            pass.pipeline.front_face = front_face;
        }
        self
    }

    pub fn with_polygon_mode(mut self, polygon_mode: PolygonMode) -> Self {
        for pass in self.technique.passes.values_mut() {
            pass.pipeline.polygon_mode = polygon_mode;
        }
        self
    }

    pub fn with_depth(mut self, depth: Option<DepthState>) -> Self {
        for pass in self.technique.passes.values_mut() {
            pass.pipeline.depth_state = depth;
        }
        self
    }

    pub fn with_multisample(mut self, count: u32) -> Self {
        for pass in self.technique.passes.values_mut() {
            pass.pipeline.multisample = MultisampleState { count };
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_material_has_independent_pipeline_variants() {
        let material = Material::new("shaders/test.wgsl".into());
        let forward = material.require_pass(material_passes::FORWARD_OPAQUE);
        let debug = material.require_pass(material_passes::DEBUG);

        assert_ne!(forward.pipeline.uuid, debug.pipeline.uuid);
        assert_eq!(forward.vertex_entry, "vs_main");
        assert_eq!(forward.fragment_entry.as_deref(), Some("fs_main"));
    }

    #[test]
    fn builders_update_all_default_passes() {
        let material = Material::new("shaders/test.wgsl".into())
            .with_cull(CullMode::Back)
            .with_depth(None);

        assert!(material.technique.passes.values().all(|pass| {
            pass.pipeline.cull_mode == CullMode::Back && pass.pipeline.depth_state.is_none()
        }));
    }

    #[test]
    fn opaque_material_opts_into_depth_and_shadow_variants() {
        let material = Material::new("shaders/test.wgsl".into());
        assert!(material.supports_pass(material_passes::DEPTH_ONLY));
        assert!(material.supports_pass(material_passes::SHADOW));
        assert!(
            material
                .require_pass(material_passes::DEPTH_ONLY)
                .fragment_entry
                .is_none()
        );
    }

    #[test]
    fn specialized_material_can_opt_into_only_one_pass() {
        let material = Material::for_pass("shaders/water.wgsl".into(), material_passes::WATER);
        assert!(material.supports_pass(material_passes::WATER));
        assert!(!material.supports_pass(material_passes::DEPTH_ONLY));
        assert!(!material.supports_pass(material_passes::SHADOW));
    }
}
