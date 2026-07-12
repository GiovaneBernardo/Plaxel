use engine::ecs::entity::Entity;
use engine::math::Vec3;

#[derive(Copy, Clone, Debug)]
pub enum NodeState {
    Leaf,
    Internal,
    Splitting,
    Merging,
    DirtyMesh,
}

#[derive(Debug, Clone)]
pub enum OctreeChanges {
    // Always prefer ReplaceMesh over Add and Remove, as the later can first remove to only in a few frames add the mesh, making it flicker
    ReplaceMesh {
        keys_to_remove: Vec<NodeKey>,
        requests: Vec<PlanetMeshRequest>,
    },
    AddMesh {
        request: PlanetMeshRequest,
    },
    RemoveMeshes {
        key: NodeKey,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct PlanetMeshRequest {
    pub planet_entity: Entity,
    pub planet_position: Vec3,
    pub planet_size: u32,
    pub node_min_corner: Vec3,
    pub node_size: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct QueuedMeshRequest {
    pub request: PlanetMeshRequest,
    pub version: u64,
    pub replacement_id: Option<u64>,
    pub priority: u32,
    pub sequence: u64,
}

#[derive(Clone, Debug)]
pub struct OctreeNode {
    pub key: NodeKey,
    pub min: Vec3, // corner
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
