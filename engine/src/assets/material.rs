use crate::assets::manager::Asset;
use crate::assets::manager::AssetContext;
use crate::assets::manager::AssetHeader;
use crate::assets::manager::Handle;
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Material {
    pub uuid: Uuid,
    //pub handle: Handle,
    pub pipeline_descriptor: PipelineDescriptor,
    pub pipeline_uuid: Uuid,
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

    pub fn new() -> Self {
        let pipeline_descriptor = PipelineDescriptor::default("shaders/cube.wgsl".to_string());
        Self {
            uuid: Uuid::new_v4(),
            pipeline_descriptor: pipeline_descriptor.clone(),
            pipeline_uuid: pipeline_descriptor.uuid,
        }
    }
}
