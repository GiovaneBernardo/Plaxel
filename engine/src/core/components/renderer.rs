use crate::{assets::manager::Handle, assets::material::Material, model::MeshAsset};

#[allow(dead_code)]
pub struct MeshRendererComponent {
    pub mesh: Handle<MeshAsset>,
    pub material: Material,
}
