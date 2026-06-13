use uuid::Uuid;

use crate::{assets::manager::Handle, model::MeshAsset};

#[allow(dead_code)]
pub struct MeshRendererComponent {
    pub mesh: Handle<MeshAsset>,
    pub material: Uuid,
}
