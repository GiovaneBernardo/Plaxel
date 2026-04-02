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
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PipelineDescriptor {
    pub shader: String,
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
}
