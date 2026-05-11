use crate::model::{Material, MeshAsset};

#[allow(dead_code)]
pub struct MeshRendererComponent {
    pub mesh: MeshAsset,
    pub material: Material,
    pub model: crate::renderer::model::Model,
}
