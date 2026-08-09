use uuid::Uuid;

use crate::{assets::manager::Handle, model::MeshAsset};

#[derive(plaxel_reflect::Reflect)]
#[reflect(from_reflect = false)]
#[allow(dead_code)]
pub struct MeshRendererComponent {
    #[reflect(ignore)]
    pub mesh: Handle<MeshAsset>,
    pub material: Uuid,
}
