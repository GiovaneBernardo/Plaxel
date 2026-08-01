use engine::ecs::entity::Entity;
use engine::math::Vec3;

use crate::planet::PlanetVertex;

#[derive(Clone, Copy, Debug)]
pub struct PlanetLodSettings {
    /// Scales how far from the camera octree nodes split and merge.
    /// Values above 1.0 keep higher-detail nodes farther away.
    pub strength: f32,
}

impl Default for PlanetLodSettings {
    fn default() -> Self {
        Self { strength: 1.0 }
    }
}

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
    ReplaceMeshes {
        planet_entity: Entity,
        transition_key: NodeKey,
        completed_state: NodeState,
        additional_transitions: Vec<(NodeKey, NodeState)>,
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
    pub node_key: NodeKey,
    pub planet_position: Vec3,
    pub node_min_corner: Vec3,
    pub node_size: f32,
    pub face_neighbors: [FaceNeighbor; 6],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceNeighborKind {
    SameOrAbsent,
    Coarser,
    Finer,
}

#[derive(Debug, Clone, Copy)]
pub struct FaceNeighbor {
    pub kind: FaceNeighborKind,
    pub min: Vec3,
    pub size: f32,
}

impl FaceNeighbor {
    pub const SAME_OR_ABSENT: Self = Self {
        kind: FaceNeighborKind::SameOrAbsent,
        min: Vec3::ZERO,
        size: 0.0,
    };
}

#[derive(Debug, Clone, Copy)]
pub struct QueuedMeshRequest {
    pub request: PlanetMeshRequest,
    pub version: u64,
    pub replacement_id: Option<u64>,
    pub priority: u32,
    pub sequence: u64,
}

#[derive(Debug, Clone)]
pub struct GeneratedReplacement {
    pub replacement_id: u64,
    pub planet_entity: Entity,
    pub transition_key: NodeKey,
    pub completed_state: NodeState,
    pub additional_transitions: Vec<(NodeKey, NodeState)>,
    pub keys_to_remove: Vec<NodeKey>,
    pub requests: Vec<PlanetMeshRequest>,
    pub meshes: Vec<GeneratedMesh>,
}

#[derive(Debug, Clone)]
pub struct GeneratedMesh {
    pub key: NodeKey,
    pub version: u64,
    pub urgent: bool,
    pub vertices: Vec<PlanetVertex>,
    pub indices: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DensityRange {
    pub min: f32,
    pub max: f32,
}

impl DensityRange {
    pub const ZERO: Self = Self { min: 0.0, max: 0.0 };

    pub fn new(min: f32, max: f32) -> Self {
        debug_assert!(min <= max);
        Self { min, max }
    }

    pub fn contains_zero(self) -> bool {
        self.min <= 0.0 && self.max >= 0.0
    }

    pub fn add(self, other: Self) -> Self {
        Self::new(self.min + other.min, self.max + other.max)
    }

    pub fn union(self, other: Self) -> Self {
        Self::new(self.min.min(other.min), self.max.max(other.max))
    }
}

#[derive(Clone, Debug)]
pub struct OctreeNode {
    pub key: NodeKey,
    pub min: Vec3, // corner
    pub size: f32,
    pub children: Option<[Box<OctreeNode>; 8]>,
    pub vertex: Option<u32>,
    /// Conservative minimum and maximum density anywhere inside this node.
    pub density_range: DensityRange,
    /// Cached equivalent of `density_range.contains_zero()`.
    pub has_surface: bool,
    pub state: NodeState,
}

#[derive(Hash, Eq, PartialEq, Clone, Copy, Debug, PartialOrd, Ord)]
pub struct NodeKey {
    pub level: i8,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}
