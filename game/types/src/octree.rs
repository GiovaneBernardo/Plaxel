use cgmath::Vector3;

#[derive(Clone)]
pub struct OctreeNode {
    pub min: Vector3<f32>, // corner
    pub size: f32,
    pub children: Option<[Box<OctreeNode>; 8]>,
    pub vertex: Option<u32>,
    pub has_surface: bool,
}
