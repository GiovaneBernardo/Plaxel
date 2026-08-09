use crate::block::Block;

#[derive(plaxel_reflect::Reflect)]
pub struct Assembly {
    pub position: engine::math::Vec3,
    pub blocks: Vec<Block>,
}
