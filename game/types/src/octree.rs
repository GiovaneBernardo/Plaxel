use cgmath::Vector3;
use engine::ecs::entity::Entity;

#[derive(Clone)]
pub enum NodeState {
    Leaf,
    Internal,
    Splitting,
    Merging,
    DirtyMesh,
}

#[derive(Debug, Clone)]
pub enum OctreeChanges {
    AddMesh { request: PlanetMeshRequest },
    RemoveMesh { key: NodeKey },
}

#[derive(Debug, Clone, Copy)]
pub struct PlanetMeshRequest {
    pub planet_entity: Entity,
    pub planet_position: Vector3<f32>,
    pub planet_size: u32,
    pub node_min_corner: Vector3<f32>,
    pub node_size: f32,
}

#[derive(Clone)]
pub struct OctreeNode {
    pub key: NodeKey,
    pub min: Vector3<f32>, // corner
    pub size: f32,
    pub children: Option<[Box<OctreeNode>; 8]>,
    pub vertex: Option<u32>,
    pub has_surface: bool,
    pub state: NodeState,
}

#[derive(Hash, Eq, PartialEq, Clone, Copy, Debug, PartialOrd, Ord)]
pub struct NodeKey {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub size: i32,
}
