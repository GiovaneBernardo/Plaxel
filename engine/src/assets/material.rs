use crate::assets::manager::Asset;
use crate::model::ModelVertex;
use crate::model::Vertex;
use crate::model::VertexLayout;
use crate::renderer::BlendMode;
use crate::renderer::CullMode;
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Material {
    pub uuid: Uuid,
    //pub handle: Handle,
    pub pipeline_descriptor: PipelineDescriptor,
    pub pipeline_uuid: Uuid,
    pub shader: String,              // from material
    pub blend_state: BlendMode,      // from material
    pub cull_mode: CullMode,         // from material
    pub vertex_layout: VertexLayout, // from mesh/material
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PipelineDescriptor {
    pub uuid: Uuid,
    pub shader: String,
}

impl PipelineDescriptor {
    pub fn default(shader: String) -> PipelineDescriptor {
        PipelineDescriptor {
            uuid: Uuid::new_v4(),
            shader,
        }
    }
}

impl Asset for Material {
    fn uuid(&self) -> Uuid {
        self.uuid
    }

    //fn handle(&self) -> Handle {
    //    self.handle
    //}
}

impl Material {
    //pub fn load(header: &AssetHeader, ctx: &AssetContext) -> Self {
    //    let material = ctx.renderer_api.load_material(header);
    //    material
    //}

    pub fn default() -> Self {
        let pipeline_descriptor = PipelineDescriptor::default("shaders/cube.wgsl".to_string());
        Self {
            uuid: Uuid::new_v4(),
            pipeline_descriptor: pipeline_descriptor.clone(),
            pipeline_uuid: pipeline_descriptor.uuid,
            blend_state: BlendMode::NoBlend,
            shader: "shaders/cube.wgsl".to_string(),
            cull_mode: CullMode::None,
            vertex_layout: ModelVertex::layout(),
        }
    }
}
