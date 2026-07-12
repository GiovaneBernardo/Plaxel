use crate::block::Block;

pub struct Assembly {
    pub position: engine::math::Vec3,
    pub blocks: Vec<Block>,
}
