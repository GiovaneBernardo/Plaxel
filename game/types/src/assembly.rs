use crate::block::Block;

pub struct Assembly {
    pub position: cgmath::Point3<f32>,
    pub blocks: Vec<Block>,
}
