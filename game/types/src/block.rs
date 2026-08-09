#[derive(plaxel_reflect::Reflect)]
pub struct Block {
    pub position: engine::math::Vec3,
    pub block_id: u64,
}
