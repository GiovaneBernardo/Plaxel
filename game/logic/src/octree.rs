use std::sync::atomic::AtomicU32;

use engine::ecs::entity::Entity;
use engine::math::{Vec3, vec3};
use game_types::{
    octree::{
        DensityRange, FaceNeighbor, FaceNeighborKind, NodeState, OctreeChanges, OctreeNode,
        PlanetMeshRequest,
    },
    planet::{PlanetTerrainEdits, TerrainBrickKey, TerrainBrickSamples},
    terrain::PlanetTerrainConfig,
};

use crate::{
    NodeKey,
    sdf::{TERRAIN_EDIT_BRICK_SIZE, terrain_height_bounds},
};

const INITIAL_SPLIT_DISTANCE_FACTOR: f32 = 1.25;
const SPLIT_DISTANCE_FACTOR: f32 = 4.0;
const MERGE_DISTANCE_FACTOR: f32 = 6.0;

#[allow(dead_code)]
pub static OCTREE_DEBUG_DEPTH: AtomicU32 = AtomicU32::new(0);
#[allow(dead_code)]
pub static OCTREE_MAX_DEPTH: AtomicU32 = AtomicU32::new(0);

const DEPTH_COLORS: [[f32; 4]; 10] = [
    [1.0, 0.2, 0.2, 1.0],
    [0.2, 1.0, 0.2, 1.0],
    [0.2, 0.4, 1.0, 1.0],
    [1.0, 1.0, 0.2, 1.0],
    [1.0, 0.2, 1.0, 1.0],
    [0.2, 1.0, 1.0, 1.0],
    [1.0, 0.6, 0.2, 1.0],
    [0.6, 0.2, 1.0, 1.0],
    [0.2, 1.0, 0.6, 1.0],
    [1.0, 0.4, 0.6, 1.0],
];

pub fn depth_color(depth: u32) -> [f32; 4] {
    DEPTH_COLORS[depth as usize % DEPTH_COLORS.len()]
}

pub fn is_behind_horizon(node_center: Vec3, camera_pos: Vec3, planet_center: Vec3) -> bool {
    let to_node = (node_center - planet_center).normalize();
    let to_camera = (camera_pos - planet_center).normalize();
    to_node.dot(to_camera) < 0.0
}

pub fn should_subdivide(node: &OctreeNode, camera_pos: Vec3, lod_strength: f32) -> bool {
    let center = node.min + Vec3::splat(node.size * 0.5);
    let distance = (center - camera_pos).length();
    distance < node.size * INITIAL_SPLIT_DISTANCE_FACTOR * lod_strength
}

pub fn build_node(
    min: Vec3,
    size: f32,
    min_size: f32,
    first: bool,
    camera_position: &engine::math::Vec3,
    planet_center: Vec3,
    terrain_config: &PlanetTerrainConfig,
    lod_strength: f32,
    terrain_edits: &PlanetTerrainEdits,
) -> OctreeNode {
    engine::profile_scope!("terrain.octree.build_nodes");
    build_node_at_level(
        min,
        size,
        min_size,
        first,
        camera_position,
        planet_center,
        terrain_config,
        lod_strength,
        terrain_edits,
        0,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_node_at_level(
    min: Vec3,
    size: f32,
    min_size: f32,
    first: bool,
    camera_position: &engine::math::Vec3,
    planet_center: Vec3,
    terrain_config: &PlanetTerrainConfig,
    lod_strength: f32,
    terrain_edits: &PlanetTerrainEdits,
    level: i8,
) -> OctreeNode {
    let density_range = node_density_range(min, size, planet_center, terrain_config, terrain_edits);
    let has_surface = density_range.contains_zero();
    let _is_behind_horizon = is_behind_horizon(
        min + vec3(size * 0.5, size * 0.5, size * 0.5),
        vec3(camera_position.x, camera_position.y, camera_position.z),
        planet_center,
    );
    let key = NodeKey {
        level,
        x: min.x as i32,
        y: min.y as i32,
        z: min.z as i32,
    };

    if !first {
        let leaf = OctreeNode {
            key,
            min,
            size,
            children: None,
            vertex: None,
            density_range,
            has_surface,
            state: NodeState::Leaf,
        };

        if !has_surface {
            return leaf;
        }

        if size <= min_size
            || !should_subdivide(
                &leaf,
                vec3(camera_position.x, camera_position.y, camera_position.z),
                lod_strength,
            )
        {
            return OctreeNode {
                key,
                min,
                size,
                children: None,
                vertex: None,
                density_range,
                has_surface,
                state: NodeState::Leaf,
            };
        }
    }

    let child_size = size / 2.0;
    let child_level = level
        .checked_add(1)
        .expect("planet octree depth exceeds NodeKey capacity");
    let children = [
        Box::new(build_node_at_level(
            min + vec3(0.0, 0.0, 0.0),
            child_size,
            min_size,
            false,
            camera_position,
            planet_center,
            terrain_config,
            lod_strength,
            terrain_edits,
            child_level,
        )),
        Box::new(build_node_at_level(
            min + vec3(child_size, 0.0, 0.0),
            child_size,
            min_size,
            false,
            camera_position,
            planet_center,
            terrain_config,
            lod_strength,
            terrain_edits,
            child_level,
        )),
        Box::new(build_node_at_level(
            min + vec3(0.0, child_size, 0.0),
            child_size,
            min_size,
            false,
            camera_position,
            planet_center,
            terrain_config,
            lod_strength,
            terrain_edits,
            child_level,
        )),
        Box::new(build_node_at_level(
            min + vec3(child_size, child_size, 0.0),
            child_size,
            min_size,
            false,
            camera_position,
            planet_center,
            terrain_config,
            lod_strength,
            terrain_edits,
            child_level,
        )),
        Box::new(build_node_at_level(
            min + vec3(0.0, 0.0, child_size),
            child_size,
            min_size,
            false,
            camera_position,
            planet_center,
            terrain_config,
            lod_strength,
            terrain_edits,
            child_level,
        )),
        Box::new(build_node_at_level(
            min + vec3(child_size, 0.0, child_size),
            child_size,
            min_size,
            false,
            camera_position,
            planet_center,
            terrain_config,
            lod_strength,
            terrain_edits,
            child_level,
        )),
        Box::new(build_node_at_level(
            min + vec3(0.0, child_size, child_size),
            child_size,
            min_size,
            false,
            camera_position,
            planet_center,
            terrain_config,
            lod_strength,
            terrain_edits,
            child_level,
        )),
        Box::new(build_node_at_level(
            min + vec3(child_size, child_size, child_size),
            child_size,
            min_size,
            false,
            camera_position,
            planet_center,
            terrain_config,
            lod_strength,
            terrain_edits,
            child_level,
        )),
    ];
    let density_range = children[1..]
        .iter()
        .fold(children[0].density_range, |range, child| {
            range.union(child.density_range)
        });
    let has_surface = density_range.contains_zero();

    OctreeNode {
        key,
        min,
        size,
        children: Some(children),
        vertex: None,
        density_range,
        has_surface,
        state: NodeState::Internal,
    }
}

fn brick_sample_range(brick: &TerrainBrickSamples) -> DensityRange {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;

    for value in brick
        .iter()
        .flat_map(|plane| plane.iter())
        .flat_map(|row| row.iter())
        .copied()
        .filter(|value| value.is_finite())
    {
        min = min.min(value);
        max = max.max(value);
    }

    if min.is_finite() && max.is_finite() {
        DensityRange::new(min, max)
    } else {
        DensityRange::ZERO
    }
}

fn base_density_range(
    min: Vec3,
    size: f32,
    planet_center: Vec3,
    terrain_config: &PlanetTerrainConfig,
) -> DensityRange {
    let min = min.as_dvec3();
    let max = min + engine::math::DVec3::splat(f64::from(size));
    let planet_center = planet_center.as_dvec3();
    let closest = engine::math::dvec3(
        planet_center.x.clamp(min.x, max.x),
        planet_center.y.clamp(min.y, max.y),
        planet_center.z.clamp(min.z, max.z),
    );
    let min_radius = (closest - planet_center).length();

    let local_min = min - planet_center;
    let local_max = max - planet_center;
    let farthest = engine::math::dvec3(
        local_min.x.abs().max(local_max.x.abs()),
        local_min.y.abs().max(local_max.y.abs()),
        local_min.z.abs().max(local_max.z.abs()),
    );
    let max_radius = farthest.length();

    let radius = f64::from(terrain_config.radius);
    let (min_height, max_height) = terrain_height_bounds(terrain_config, None);
    DensityRange::new(
        (min_radius - (radius + f64::from(max_height))) as f32,
        (max_radius - (radius + f64::from(min_height))) as f32,
    )
}

fn edit_density_range(
    local_min: Vec3,
    size: f32,
    terrain_edits: &PlanetTerrainEdits,
) -> DensityRange {
    let local_max = local_min + Vec3::splat(size);
    let mut range = None;
    let mut found_key_count = 0_i64;

    let min_key = [
        (local_min.x / TERRAIN_EDIT_BRICK_SIZE).floor() as i32,
        (local_min.y / TERRAIN_EDIT_BRICK_SIZE).floor() as i32,
        (local_min.z / TERRAIN_EDIT_BRICK_SIZE).floor() as i32,
    ];
    let max_key = [
        (local_max.x / TERRAIN_EDIT_BRICK_SIZE).ceil() as i32 - 1,
        (local_max.y / TERRAIN_EDIT_BRICK_SIZE).ceil() as i32 - 1,
        (local_max.z / TERRAIN_EDIT_BRICK_SIZE).ceil() as i32 - 1,
    ];
    let covered_key_count = (i64::from(max_key[0]) - i64::from(min_key[0]) + 1)
        .saturating_mul(i64::from(max_key[1]) - i64::from(min_key[1]) + 1)
        .saturating_mul(i64::from(max_key[2]) - i64::from(min_key[2]) + 1);

    let mut include_brick = |key: &TerrainBrickKey, brick: &TerrainBrickSamples| {
        let brick_range = terrain_edits
            .modified_ranges
            .get(key)
            .copied()
            .unwrap_or_else(|| brick_sample_range(brick));
        range = Some(
            range
                .map(|range: DensityRange| range.union(brick_range))
                .unwrap_or(brick_range),
        );
        found_key_count += 1;
    };

    // Small nodes usually cover one or a handful of bricks, so direct hash
    // lookups are O(covered bricks). Large nodes scan the sparse edit set to
    // avoid iterating a potentially enormous empty coordinate volume.
    if covered_key_count <= terrain_edits.modified_chunks.len() as i64 {
        for x in min_key[0]..=max_key[0] {
            for y in min_key[1]..=max_key[1] {
                for z in min_key[2]..=max_key[2] {
                    let key = TerrainBrickKey { x, y, z, level: 0 };
                    if let Some(brick) = terrain_edits.modified_chunks.get(&key) {
                        include_brick(&key, brick);
                    }
                }
            }
        }
    } else {
        for (key, brick) in &terrain_edits.modified_chunks {
            if key.level == 0
                && key.x >= min_key[0]
                && key.x <= max_key[0]
                && key.y >= min_key[1]
                && key.y <= max_key[1]
                && key.z >= min_key[2]
                && key.z <= max_key[2]
            {
                include_brick(key, brick);
            }
        }
    }

    let Some(range) = range else {
        return DensityRange::ZERO;
    };

    // Unmodified bricks evaluate to zero. Include that value unless modified
    // bricks cover the node's complete brick-coordinate range.
    if found_key_count < covered_key_count {
        range.union(DensityRange::ZERO)
    } else {
        range
    }
}

pub fn node_density_range(
    min: Vec3,
    size: f32,
    planet_center: Vec3,
    terrain_config: &PlanetTerrainConfig,
    terrain_edits: &PlanetTerrainEdits,
) -> DensityRange {
    base_density_range(min, size, planet_center, terrain_config).add(edit_density_range(
        min - planet_center,
        size,
        terrain_edits,
    ))
}

pub fn has_surface(
    min: Vec3,
    size: f32,
    planet_center: Vec3,
    terrain_config: &PlanetTerrainConfig,
    terrain_edits: &PlanetTerrainEdits,
) -> bool {
    node_density_range(min, size, planet_center, terrain_config, terrain_edits).contains_zero()
}

pub fn collect_octree_nodes_at_depth(
    node: &OctreeNode,
    current_depth: u32,
    target_depth: u32,
    out: &mut Vec<(Vec3, f32, u32)>,
) {
    if current_depth == target_depth {
        let half = node.size / 2.0;
        let center = Vec3::new(node.min.x + half, node.min.y + half, node.min.z + half);
        out.push((center, node.size, current_depth));
        return;
    }
    if let Some(children) = &node.children {
        for child in children.iter() {
            collect_octree_nodes_at_depth(child, current_depth + 1, target_depth, out);
        }
    }
}

pub fn collect_octree_nodes(
    node: &OctreeNode,
    current_depth: u32,
    out: &mut Vec<(Vec3, f32, u32)>,
) {
    let half = node.size / 2.0;
    let center = Vec3::new(node.min.x + half, node.min.y + half, node.min.z + half);
    out.push((center, node.size, current_depth));

    if let Some(children) = &node.children {
        for child in children.iter() {
            collect_octree_nodes(child, current_depth + 1, out);
        }
    }
}

pub fn octree_max_depth(node: &OctreeNode, current: u32) -> u32 {
    match &node.children {
        None => current,
        Some(children) => children
            .iter()
            .map(|c| octree_max_depth(c, current + 1))
            .max()
            .unwrap_or(current),
    }
}

pub fn collect_leaf_nodes<'a>(node: &'a OctreeNode, out: &mut Vec<&'a OctreeNode>) {
    match &node.children {
        None => {
            out.push(node);
        }
        Some(children) => children
            .iter()
            .map(|c| collect_leaf_nodes(c, out))
            .max()
            .unwrap_or(()),
    }
}

pub fn has_pending_transition(node: &OctreeNode) -> bool {
    if matches!(node.state, NodeState::Splitting | NodeState::Merging) {
        return true;
    }
    node.children
        .as_ref()
        .is_some_and(|children| children.iter().any(|child| has_pending_transition(child)))
}

const FACE_AXES: [(usize, bool); 6] = [
    (0, false),
    (0, true),
    (1, false),
    (1, true),
    (2, false),
    (2, true),
];

fn component(value: Vec3, axis: usize) -> f32 {
    match axis {
        0 => value.x,
        1 => value.y,
        _ => value.z,
    }
}

fn face_leaf_neighbors<'a>(
    node: &'a OctreeNode,
    target_min: Vec3,
    target_size: f32,
    face: usize,
    output: &mut Vec<&'a OctreeNode>,
) {
    let (axis, positive) = FACE_AXES[face];
    let target_max = target_min + Vec3::splat(target_size);
    let node_max = node.min + Vec3::splat(node.size);
    let plane = if positive {
        component(target_max, axis)
    } else {
        component(target_min, axis)
    };
    let epsilon = target_size.max(node.size) * 1e-5;
    if plane < component(node.min, axis) - epsilon || plane > component(node_max, axis) + epsilon {
        return;
    }

    for tangent in 0..3 {
        if tangent == axis {
            continue;
        }
        // Point/edge contacts do not share a face and must not affect topology.
        if component(node.min, tangent) >= component(target_max, tangent) - epsilon
            || component(node_max, tangent) <= component(target_min, tangent) + epsilon
        {
            return;
        }
    }

    if let Some(children) = node.children.as_ref() {
        for child in children {
            face_leaf_neighbors(child, target_min, target_size, face, output);
        }
    } else {
        let candidate_plane = if positive {
            component(node.min, axis)
        } else {
            component(node_max, axis)
        };
        if (candidate_plane - plane).abs() <= epsilon {
            output.push(node);
        }
    }
}

/// Records only the topology needed by the mesher. This query follows octree
/// branches touching each face, so its cost is proportional to the neighboring
/// leaves rather than to all leaves in the planet.
pub fn annotate_mesh_request(root: &OctreeNode, request: &mut PlanetMeshRequest) {
    request.face_neighbors = [FaceNeighbor::SAME_OR_ABSENT; 6];
    for face in 0..6 {
        let mut neighbors = Vec::new();
        face_leaf_neighbors(
            root,
            request.node_min_corner,
            request.node_size,
            face,
            &mut neighbors,
        );
        if neighbors.is_empty() {
            continue;
        }

        if let Some(neighbor) = neighbors
            .iter()
            .copied()
            .filter(|neighbor| neighbor.size > request.node_size)
            .max_by(|a, b| a.size.total_cmp(&b.size))
        {
            request.face_neighbors[face] = FaceNeighbor {
                kind: FaceNeighborKind::Coarser,
                min: neighbor.min,
                size: neighbor.size,
            };
        } else if neighbors
            .iter()
            .any(|neighbor| neighbor.size < request.node_size)
        {
            request.face_neighbors[face] = FaceNeighbor {
                kind: FaceNeighborKind::Finer,
                min: Vec3::ZERO,
                size: 0.0,
            };
        }
    }
}

pub fn collect_face_neighbor_leaves<'a>(
    root: &'a OctreeNode,
    min: Vec3,
    size: f32,
    output: &mut Vec<&'a OctreeNode>,
) {
    for face in 0..6 {
        face_leaf_neighbors(root, min, size, face, output);
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Aabb {
    pub min: engine::math::Vec3,
    pub max: engine::math::Vec3,
}

impl Aabb {
    pub fn size(&self) -> f32 {
        (self.max.x - self.min.x)
            .max(self.max.y - self.min.y)
            .max(self.max.z - self.min.z)
    }

    pub fn center(&self) -> engine::math::Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn distance_to_point(&self, p: engine::math::Vec3) -> f32 {
        let closest = vec3(
            p.x.clamp(self.min.x, self.max.x),
            p.y.clamp(self.min.y, self.max.y),
            p.z.clamp(self.min.z, self.max.z),
        );
        (p - closest).length()
    }
}

pub fn update(
    node: &mut OctreeNode,
    camera_pos: Vec3,
    planet_entity: Entity,
    planet_position: Vec3,
    terrain_config: &PlanetTerrainConfig,
    lod_strength: f32,
    changes: &mut Vec<OctreeChanges>,
    terrain_edits: &PlanetTerrainEdits,
) {
    const MIN_NODE_SIZE: f32 = 32.0;

    if has_pending_transition(node) {
        // Merges discard their old child topology, so let those short atomic
        // transitions finish. Splits retain the rendered ancestor and can be
        // rolled back safely when the camera asks for a different target.
        if has_pending_merge(node)
            || !topology_target_changed(node, camera_pos, MIN_NODE_SIZE, lod_strength, true)
        {
            return;
        }

        rollback_pending_splits(node);
        changes.push(OctreeChanges::CancelPlanetReplacements { planet_entity });
    }

    update_node(
        node,
        camera_pos,
        planet_entity,
        planet_position,
        terrain_config,
        lod_strength,
        changes,
        terrain_edits,
        true,
    );
}

fn has_pending_merge(node: &OctreeNode) -> bool {
    matches!(node.state, NodeState::Merging)
        || node
            .children
            .as_ref()
            .is_some_and(|children| children.iter().any(|child| has_pending_merge(child)))
}

fn topology_target_changed(
    node: &OctreeNode,
    camera_pos: Vec3,
    min_node_size: f32,
    lod_strength: f32,
    is_root_node: bool,
) -> bool {
    if matches!(node.state, NodeState::Merging) {
        return false;
    }

    match node.children.as_ref() {
        None => {
            (is_root_node && node.has_surface)
                || should_split(node, camera_pos, min_node_size, lod_strength)
        }
        Some(children) => {
            if !is_root_node && should_merge(node, camera_pos, min_node_size, lod_strength) {
                return true;
            }
            children.iter().any(|child| {
                topology_target_changed(child, camera_pos, min_node_size, lod_strength, false)
            })
        }
    }
}

fn rollback_pending_splits(node: &mut OctreeNode) {
    if matches!(node.state, NodeState::Splitting) {
        node.state = NodeState::Leaf;
        node.children = None;
        return;
    }
    if let Some(children) = node.children.as_mut() {
        for child in children {
            rollback_pending_splits(child);
        }
    }
}

fn update_node(
    node: &mut OctreeNode,
    camera_pos: Vec3,
    planet_entity: Entity,
    planet_position: Vec3,
    terrain_config: &PlanetTerrainConfig,
    lod_strength: f32,
    changes: &mut Vec<OctreeChanges>,
    terrain_edits: &PlanetTerrainEdits,
    is_root_node: bool,
) {
    let min_node_size = 32.0;

    // Do not refine a topology that has not become visible yet. Otherwise a
    // child replacement can supersede its parent replacement before anything
    // takes responsibility for removing the currently rendered ancestor.
    if matches!(node.state, NodeState::Splitting | NodeState::Merging) {
        return;
    }

    if is_root_node && node.children.is_none() {
        split_node(
            node,
            camera_pos,
            min_node_size,
            lod_strength,
            changes,
            planet_entity,
            planet_position,
            terrain_config,
            terrain_edits,
        );
        return;
    }

    if should_split(node, camera_pos, min_node_size, lod_strength) {
        split_node(
            node,
            camera_pos,
            min_node_size,
            lod_strength,
            changes,
            planet_entity,
            planet_position,
            terrain_config,
            terrain_edits,
        );
        return;
    }

    if !is_root_node && should_merge(node, camera_pos, min_node_size, lod_strength) {
        merge_node(node, changes, planet_entity, planet_position);
        return;
    }

    if let Some(children) = node.children.as_mut() {
        for child in children.iter_mut() {
            update_node(
                child,
                camera_pos,
                planet_entity,
                planet_position,
                terrain_config,
                lod_strength,
                changes,
                terrain_edits,
                false,
            );
        }
    }
}

pub fn create_children(
    parent: &OctreeNode,
    planet_position: Vec3,
    terrain_config: &PlanetTerrainConfig,
    terrain_edits: &PlanetTerrainEdits,
) -> [Box<OctreeNode>; 8] {
    let bounds = node_bounds(parent);
    let min = bounds.min;
    let mid = bounds.center();
    let child_size = parent.size * 0.5;

    let make_child = |min: engine::math::Vec3| {
        let density_range = node_density_range(
            min,
            child_size,
            planet_position,
            terrain_config,
            terrain_edits,
        );
        OctreeNode {
            key: NodeKey {
                level: parent.key.level + 1,
                x: min.x as i32,
                y: min.y as i32,
                z: min.z as i32,
            },
            min,
            size: child_size,
            children: None,
            vertex: None,
            density_range,
            has_surface: density_range.contains_zero(),
            state: NodeState::Leaf,
        }
    };

    [
        Box::new(make_child(engine::math::vec3(min.x, min.y, min.z))),
        Box::new(make_child(engine::math::vec3(mid.x, min.y, min.z))),
        Box::new(make_child(engine::math::vec3(min.x, mid.y, min.z))),
        Box::new(make_child(engine::math::vec3(mid.x, mid.y, min.z))),
        Box::new(make_child(engine::math::vec3(min.x, min.y, mid.z))),
        Box::new(make_child(engine::math::vec3(mid.x, min.y, mid.z))),
        Box::new(make_child(engine::math::vec3(min.x, mid.y, mid.z))),
        Box::new(make_child(engine::math::vec3(mid.x, mid.y, mid.z))),
    ]
}

pub fn collect_child_mesh_removals(
    planet_entity: Entity,
    children: &[Box<OctreeNode>; 8],
    changes: &mut Vec<OctreeChanges>,
) {
    engine::profile_scope!("terrain.octree.update_topology");
    for child in children {
        if let Some(grandchildren) = child.children.as_ref() {
            collect_child_mesh_removals(planet_entity, grandchildren, changes);
        } else {
            changes.push(OctreeChanges::RemoveMeshes {
                planet_entity,
                key: child.key,
            });
        }
    }
}

pub fn node_bounds(node: &OctreeNode) -> Aabb {
    Aabb {
        min: node.min,
        max: node.min + vec3(node.size, node.size, node.size),
    }
}

fn bounds_overlap(min_a: Vec3, max_a: Vec3, min_b: Vec3, max_b: Vec3) -> bool {
    min_a.x <= max_b.x
        && max_a.x >= min_b.x
        && min_a.y <= max_b.y
        && max_a.y >= min_b.y
        && min_a.z <= max_b.z
        && max_a.z >= min_b.z
}

/// Recomputes density intervals only along octree branches affected by an edit,
/// then propagates child ranges back to their ancestors.
pub fn refresh_density_ranges_in_bounds(
    node: &mut OctreeNode,
    dirty_min: Vec3,
    dirty_max: Vec3,
    planet_position: Vec3,
    terrain_config: &PlanetTerrainConfig,
    terrain_edits: &PlanetTerrainEdits,
) {
    let node_max = node.min + Vec3::splat(node.size);
    if !bounds_overlap(node.min, node_max, dirty_min, dirty_max) {
        return;
    }

    if let Some(children) = node.children.as_mut() {
        for child in children.iter_mut() {
            refresh_density_ranges_in_bounds(
                child,
                dirty_min,
                dirty_max,
                planet_position,
                terrain_config,
                terrain_edits,
            );
        }

        let mut range = children[0].density_range;
        for child in &children[1..] {
            range = range.union(child.density_range);
        }
        node.density_range = range;
    } else {
        node.density_range = node_density_range(
            node.min,
            node.size,
            planet_position,
            terrain_config,
            terrain_edits,
        );
    }

    node.has_surface = node.density_range.contains_zero();
}

pub fn should_split(
    node: &OctreeNode,
    camera_pos: engine::math::Vec3,
    min_node_size: f32,
    lod_strength: f32,
) -> bool {
    if node.children.is_some() {
        return false;
    }

    let bounds = node_bounds(node);

    if bounds.size() <= min_node_size {
        return false;
    }

    if !node.has_surface {
        return false;
    }

    let distance = bounds.distance_to_point(camera_pos);
    let split_distance = bounds.size() * SPLIT_DISTANCE_FACTOR * lod_strength;

    distance < split_distance
}

pub fn should_merge(
    node: &OctreeNode,
    camera_pos: engine::math::Vec3,
    min_node_size: f32,
    lod_strength: f32,
) -> bool {
    if node.children.is_none() {
        return false;
    }

    let bounds = node_bounds(node);

    if bounds.size() <= min_node_size {
        return false;
    }

    let distance = bounds.distance_to_point(camera_pos);
    let merge_distance = bounds.size() * MERGE_DISTANCE_FACTOR * lod_strength;

    distance > merge_distance
}

fn mesh_request(
    node: &OctreeNode,
    planet_entity: Entity,
    planet_position: Vec3,
) -> PlanetMeshRequest {
    PlanetMeshRequest {
        planet_entity,
        node_key: node.key,
        planet_position,
        node_min_corner: node.min,
        node_size: node.size,
        face_neighbors: [FaceNeighbor::SAME_OR_ABSENT; 6],
    }
}

fn split_node(
    node: &mut OctreeNode,
    camera_pos: Vec3,
    min_node_size: f32,
    lod_strength: f32,
    changes: &mut Vec<OctreeChanges>,
    planet_entity: Entity,
    planet_position: Vec3,
    terrain_config: &PlanetTerrainConfig,
    terrain_edits: &PlanetTerrainEdits,
) {
    let mut children = create_children(node, planet_position, terrain_config, terrain_edits);
    for child in &mut children {
        refine_new_subtree(
            child,
            camera_pos,
            min_node_size,
            lod_strength,
            planet_position,
            terrain_config,
            terrain_edits,
        );
    }

    let mut requests = Vec::new();
    collect_surface_leaf_requests(&children, planet_entity, planet_position, &mut requests);

    changes.push(OctreeChanges::ReplaceMeshes {
        planet_entity,
        transition_key: node.key,
        completed_state: NodeState::Internal,
        additional_transitions: Vec::new(),
        keys_to_remove: vec![node.key],
        requests,
    });

    node.state = NodeState::Splitting;
    node.children = Some(children);
}

/// Builds directly to the LOD required by the current camera position. Only
/// the root of this newly built subtree is marked as a pending transition, so
/// its currently rendered ancestor remains visible until all final leaf meshes
/// can replace it atomically.
fn refine_new_subtree(
    node: &mut OctreeNode,
    camera_pos: Vec3,
    min_node_size: f32,
    lod_strength: f32,
    planet_position: Vec3,
    terrain_config: &PlanetTerrainConfig,
    terrain_edits: &PlanetTerrainEdits,
) {
    if !should_split(node, camera_pos, min_node_size, lod_strength) {
        return;
    }

    let mut children = create_children(node, planet_position, terrain_config, terrain_edits);
    for child in &mut children {
        refine_new_subtree(
            child,
            camera_pos,
            min_node_size,
            lod_strength,
            planet_position,
            terrain_config,
            terrain_edits,
        );
    }
    node.state = NodeState::Internal;
    node.children = Some(children);
}

fn collect_surface_leaf_requests(
    children: &[Box<OctreeNode>; 8],
    planet_entity: Entity,
    planet_position: Vec3,
    requests: &mut Vec<PlanetMeshRequest>,
) {
    for child in children {
        if let Some(grandchildren) = child.children.as_ref() {
            collect_surface_leaf_requests(grandchildren, planet_entity, planet_position, requests);
        } else if child.has_surface {
            requests.push(mesh_request(child, planet_entity, planet_position));
        }
    }
}

fn merge_node(
    node: &mut OctreeNode,
    changes: &mut Vec<OctreeChanges>,
    planet_entity: Entity,
    planet_position: Vec3,
) {
    let mut keys_to_remove = Vec::new();

    if let Some(children) = &node.children {
        collect_leaf_keys(children, &mut keys_to_remove);
    }

    let requests = if node.has_surface {
        vec![mesh_request(node, planet_entity, planet_position)]
    } else {
        Vec::new()
    };

    changes.push(OctreeChanges::ReplaceMeshes {
        planet_entity,
        transition_key: node.key,
        completed_state: NodeState::Leaf,
        additional_transitions: Vec::new(),
        keys_to_remove,
        requests,
    });

    node.state = NodeState::Merging;
    node.children = None;
}

fn collect_leaf_keys(children: &[Box<OctreeNode>; 8], output: &mut Vec<NodeKey>) {
    for child in children {
        match &child.children {
            Some(children) => collect_leaf_keys(children, output),
            None if child.has_surface => output.push(child.key),
            None => {}
        }
    }
}

pub fn ray_intersects(
    node: &OctreeNode,
    ray_origin: Vec3,
    ray_direction: Vec3,
) -> Option<(f32, f32)> {
    let inv_dir = vec3(
        1.0 / ray_direction.x,
        1.0 / ray_direction.y,
        1.0 / ray_direction.z,
    );
    let max = vec3(
        node.min.x + node.size,
        node.min.y + node.size,
        node.min.z + node.size,
    );

    let mut tmin = (node.min.x - ray_origin.x) * inv_dir.x;
    let mut tmax = (max.x - ray_origin.x) * inv_dir.x;
    if tmin > tmax {
        std::mem::swap(&mut tmin, &mut tmax);
    }

    let mut tymin = (node.min.y - ray_origin.y) * inv_dir.y;
    let mut tymax = (max.y - ray_origin.y) * inv_dir.y;
    if tymin > tymax {
        std::mem::swap(&mut tymin, &mut tymax);
    }

    if tmin > tymax || tymin > tmax {
        return None;
    }

    tmin = tmin.max(tymin);
    tmax = tmax.min(tymax);

    let mut tzmin = (node.min.z - ray_origin.z) * inv_dir.z;
    let mut tzmax = (max.z - ray_origin.z) * inv_dir.z;
    if tzmin > tzmax {
        std::mem::swap(&mut tzmin, &mut tzmax);
    }

    if tmin > tzmax || tzmin > tmax {
        return None;
    }

    tmin = tmin.max(tzmin);
    tmax = tmax.min(tzmax);

    if tmax >= 0.0 {
        Some((tmin.max(0.0), tmax))
    } else {
        None
    }
}

pub fn traverse_octree(
    ray_origin: Vec3,
    ray_direction: Vec3,
    node: &OctreeNode,
    best_t: &mut f32,
    last_node: &mut Option<OctreeNode>,
) {
    *last_node = Some(node.clone());
    let Some((t_enter, _t_exit)) = ray_intersects(node, ray_origin, ray_direction) else {
        return;
    };

    if t_enter > *best_t {
        return;
    }

    let Some(children) = node.children.as_ref() else {
        if node.has_surface {
            *best_t = t_enter;
        }
        return;
    };

    let mut children: Vec<(&OctreeNode, f32)> = children
        .iter()
        .filter_map(|child| {
            let child = child.as_ref();
            let (t, _) = ray_intersects(child, ray_origin, ray_direction)?;
            if t > *best_t {
                return None;
            }
            Some((child, t))
        })
        .collect();

    children.sort_unstable_by(|a, b| a.1.total_cmp(&b.1));

    for (child, _) in children {
        traverse_octree(ray_origin, ray_direction, child, best_t, last_node);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use game_types::planet::PlanetTerrainEdits;

    use super::*;
    use crate::systems::planets::planet_system::default_planet_terrain_config;

    #[test]
    fn refinement_skips_intermediate_lod_replacements() {
        let config = default_planet_terrain_config();
        let planet_position = Vec3::ZERO;
        let size = 4_096.0;
        let min = vec3(config.radius - size * 0.5, -size * 0.5, -size * 0.5);
        let density_range = node_density_range(
            min,
            size,
            planet_position,
            &config,
            &PlanetTerrainEdits {
                modified_chunks: HashMap::new(),
                modified_ranges: HashMap::new(),
            },
        );
        let mut node = OctreeNode {
            key: NodeKey {
                level: 8,
                x: min.x as i32,
                y: min.y as i32,
                z: min.z as i32,
            },
            min,
            size,
            children: None,
            vertex: None,
            density_range,
            has_surface: density_range.contains_zero(),
            state: NodeState::Leaf,
        };
        let edits = PlanetTerrainEdits {
            modified_chunks: HashMap::new(),
            modified_ranges: HashMap::new(),
        };
        let mut changes = Vec::new();

        update(
            &mut node,
            vec3(config.radius, 0.0, 0.0),
            Entity::PLACEHOLDER,
            planet_position,
            &config,
            1.0,
            &mut changes,
            &edits,
        );

        let [OctreeChanges::ReplaceMeshes { requests, .. }] = changes.as_slice() else {
            panic!("refinement should be submitted as one atomic replacement");
        };
        assert!(
            requests
                .iter()
                .any(|request| request.node_key.level >= node.key.level + 3),
            "the replacement should target the required deep LOD directly"
        );

        changes.clear();
        update(
            &mut node,
            vec3(-config.radius, 0.0, 0.0),
            Entity::PLACEHOLDER,
            planet_position,
            &config,
            1.0,
            &mut changes,
            &edits,
        );

        assert!(matches!(
            changes.first(),
            Some(OctreeChanges::CancelPlanetReplacements { .. })
        ));
        let Some(OctreeChanges::ReplaceMeshes { requests, .. }) = changes.get(1) else {
            panic!("moving away should replace the obsolete pending LOD target");
        };
        assert!(
            requests
                .iter()
                .all(|request| request.node_key.level == node.key.level + 1),
            "the replacement should be rebuilt for the latest camera position"
        );
    }
}
