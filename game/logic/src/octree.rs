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
    let may_contain_surface = density_range.contains_zero();
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
            may_contain_surface,
            state: NodeState::Leaf,
        };

        if !may_contain_surface {
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
                may_contain_surface,
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
        may_contain_surface: has_surface,
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
    if has_pending_transition(node) {
        return;
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
        &[],
        true,
    );
}

// The runtime may plan disjoint batches while previous batches await GPU acknowledgment.
pub fn update_reserved(
    node: &mut OctreeNode,
    camera_pos: Vec3,
    planet_entity: Entity,
    planet_position: Vec3,
    terrain_config: &PlanetTerrainConfig,
    lod_strength: f32,
    changes: &mut Vec<OctreeChanges>,
    terrain_edits: &PlanetTerrainEdits,
    reserved: &[Aabb],
    repair_balance: bool,
) {
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
        reserved,
        repair_balance,
    );
}

fn touches(a: &Aabb, b: &Aabb) -> bool {
    a.min.cmple(b.max).all() && b.min.cmple(a.max).all()
}

fn transition_footprint(root: &OctreeNode, node: &OctreeNode) -> Vec<Aabb> {
    let mut neighbors = Vec::new();
    collect_face_neighbor_leaves(root, node.min, node.size, &mut neighbors);
    std::iter::once(node_bounds(node))
        .chain(neighbors.into_iter().map(node_bounds))
        .collect()
}

pub fn replacement_footprint(root: &OctreeNode, change: &OctreeChanges) -> Vec<Aabb> {
    let OctreeChanges::ReplaceMeshes {
        transition_key,
        additional_transitions,
        requests,
        ..
    } = change
    else {
        return Vec::new();
    };
    let mut regions = Vec::new();
    for key in
        std::iter::once(transition_key).chain(additional_transitions.iter().map(|(key, _)| key))
    {
        let size = root.size / 2.0_f32.powi(i32::from(key.level - root.key.level));
        let min = vec3(key.x as f32, key.y as f32, key.z as f32);
        regions.push(Aabb {
            min,
            max: min + Vec3::splat(size),
        });
        let mut neighbors = Vec::new();
        collect_face_neighbor_leaves(root, min, size, &mut neighbors);
        regions.extend(neighbors.into_iter().map(node_bounds));
    }
    regions.extend(requests.iter().map(|r| Aabb {
        min: r.node_min_corner,
        max: r.node_min_corner + Vec3::splat(r.node_size),
    }));
    regions
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
            (is_root_node && node.may_contain_surface)
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

const SPLITS_PER_REPLACEMENT: usize = 4;
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
    reserved: &[Aabb],
    repair_balance: bool,
) {
    let Some((key, split)) =
        next_lod_change_reserved(node, camera_pos, lod_strength, reserved, repair_balance)
    else {
        return;
    };

    if split {
        // Capture old leaves before splitting so their rendered meshes get removed.
        let original_leaves = {
            let mut leaves = Vec::new();
            collect_leaf_nodes(node, &mut leaves);

            leaves
                .into_iter()
                .map(|leaf| leaf.key)
                .collect::<std::collections::HashSet<_>>()
        };

        let target = find_node_mut(node, key).expect("selected node exists");
        let refinement_bounds = node_bounds(target);
        let mut splits = Vec::new();

        split_node(
            target,
            &mut splits,
            planet_entity,
            planet_position,
            terrain_config,
            terrain_edits,
        );

        for _ in 1..SPLITS_PER_REPLACEMENT {
            let Some((key, true)) =
                next_lod_change_reserved(node, camera_pos, lod_strength, reserved, repair_balance)
            else {
                break;
            };

            let target = find_node_mut(node, key).unwrap();
            let bounds = node_bounds(target);
            // Do not make the nearby region wait for unrelated terrain in this atomic batch.
            if !bounds.min.cmpge(refinement_bounds.min).all()
                || !bounds.max.cmple(refinement_bounds.max).all()
            {
                break;
            }

            split_node(
                target,
                &mut splits,
                planet_entity,
                planet_position,
                terrain_config,
                terrain_edits,
            );
        }

        let mut removals = Vec::new();
        let mut requests = Vec::new();
        let mut transitions = Vec::new();

        for change in splits {
            let OctreeChanges::ReplaceMeshes {
                transition_key,
                keys_to_remove,
                requests: new_requests,
                ..
            } = change
            else {
                unreachable!();
            };

            removals.extend(keys_to_remove);
            requests.extend(new_requests);
            transitions.push((transition_key, NodeState::Internal));
        }

        requests.retain(|request| {
            find_node_mut(node, request.node_key)
                .is_some_and(|node| node.children.is_none() && node.may_contain_surface)
        });

        let mut seen = std::collections::HashSet::new();
        requests.retain(|request| seen.insert(request.node_key));

        let mut seen = std::collections::HashSet::new();

        removals.retain(|key| original_leaves.contains(key) && seen.insert(*key));

        let (transition_key, completed_state) = transitions.remove(0);

        changes.push(OctreeChanges::ReplaceMeshes {
            planet_entity,
            transition_key,
            completed_state,
            additional_transitions: transitions,
            keys_to_remove: removals,
            requests,
        });
    } else {
        let target = find_node_mut(node, key).expect("selected node exists");
        let mut merges = Vec::new();
        merge_node(target, &mut merges, planet_entity, planet_position);
        // Plan against the evolving topology, but expose all merges in one GPU swap.
        for _ in 1..8 {
            let Some((key, false)) =
                next_lod_change_reserved(node, camera_pos, lod_strength, reserved, repair_balance)
            else {
                break;
            };
            merge_node(
                find_node_mut(node, key).unwrap(),
                &mut merges,
                planet_entity,
                planet_position,
            );
        }
        let mut removals = Vec::new();
        let mut requests = Vec::new();
        let mut transitions = Vec::new();
        for merge in merges {
            if let OctreeChanges::ReplaceMeshes {
                transition_key,
                keys_to_remove,
                requests: insert,
                ..
            } = merge
            {
                removals.extend(keys_to_remove);
                requests.extend(insert);
                transitions.push((transition_key, NodeState::Leaf));
            }
        }
        // A later merge may absorb an earlier one: never generate that intermediate mesh.
        requests.retain(|request| find_node_mut(node, request.node_key).is_some());
        transitions.retain(|(key, _)| find_node_mut(node, *key).is_some());
        let (transition_key, completed_state) = transitions.remove(0);
        changes.push(OctreeChanges::ReplaceMeshes {
            planet_entity,
            transition_key,
            completed_state,
            additional_transitions: transitions,
            keys_to_remove: removals,
            requests,
        });
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
            may_contain_surface: density_range.contains_zero(),
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

    node.may_contain_surface = node.density_range.contains_zero();
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

    if !node.may_contain_surface {
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
    changes: &mut Vec<OctreeChanges>,
    planet_entity: Entity,
    planet_position: Vec3,
    terrain_config: &PlanetTerrainConfig,
    terrain_edits: &PlanetTerrainEdits,
) {
    let children = create_children(node, planet_position, terrain_config, terrain_edits);

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

/// Prepare an entire replacement without generating intermediate LOD meshes.
pub fn prepare_terrain_replacement(
    root: &mut OctreeNode,
    camera: Vec3,
    strength: f32,
    entity: Entity,
    position: Vec3,
    config: &PlanetTerrainConfig,
    edits: &PlanetTerrainEdits,
) -> Vec<PlanetMeshRequest> {
    // The normal LOD policy always splits a surface-bearing root once.
    if root.may_contain_surface {
        let mut children = create_children(root, position, config, edits);
        for child in &mut children {
            refine_new_subtree(child, camera, 32.0, strength, position, config, edits);
        }
        root.children = Some(children);
        root.state = NodeState::Internal;
    }

    // Density pruning can leave coarse empty neighbors beside fine surface
    // leaves. Balance those too before assigning mesh seam ownership.
    loop {
        let mut leaves = Vec::new();
        collect_leaf_nodes(root, &mut leaves);
        let mut split = std::collections::HashSet::new();
        for leaf in leaves {
            let mut neighbors = Vec::new();
            collect_face_neighbor_leaves(root, leaf.min, leaf.size, &mut neighbors);
            for neighbor in neighbors {
                if neighbor.size > leaf.size * 2.0 {
                    split.insert(neighbor.key);
                }
            }
        }
        if split.is_empty() {
            break;
        }
        for key in split {
            let node = find_node_mut(root, key).unwrap();
            node.children = Some(create_children(node, position, config, edits));
            node.state = NodeState::Internal;
        }
    }

    let mut requests = Vec::new();
    if let Some(children) = &root.children {
        collect_surface_leaf_requests(children, entity, position, &mut requests);
    }
    for request in &mut requests {
        annotate_mesh_request(root, request);
    }
    requests
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
        } else if child.may_contain_surface {
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

    let requests = if node.may_contain_surface {
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
            None if child.may_contain_surface => output.push(child.key),
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
        if node.may_contain_surface {
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

/// Apply only acknowledged proofs to live leaves. Edits use the normal range refresh.
pub fn apply_uniform_proofs(root: &mut OctreeNode, keys: &[NodeKey], edits: &PlanetTerrainEdits) {
    if !edits.modified_chunks.is_empty() {
        return;
    }
    for &key in keys {
        if let Some(node) = find_node_mut(root, key) {
            if node.children.is_none() {
                node.may_contain_surface = false;
            }
        }
    }
}

pub fn find_node_mut(node: &mut OctreeNode, key: NodeKey) -> Option<&mut OctreeNode> {
    if node.key == key {
        return Some(node);
    }
    node.children
        .as_mut()?
        .iter_mut()
        .find_map(|child| find_node_mut(child, key))
}

pub fn next_lod_change(root: &OctreeNode, camera: Vec3, strength: f32) -> Option<(NodeKey, bool)> {
    next_lod_change_reserved(root, camera, strength, &[], true)
}

fn next_lod_change_reserved(
    root: &OctreeNode,
    camera: Vec3,
    strength: f32,
    reserved: &[Aabb],
    repair_balance: bool,
) -> Option<(NodeKey, bool)> {
    let available = |node: &OctreeNode| {
        reserved.is_empty()
            || !transition_footprint(root, node)
                .iter()
                .any(|a| reserved.iter().any(|b| touches(a, b)))
    };
    engine::profile_scope!("terrain.octree.select_lod");
    fn gather<'a>(
        node: &'a OctreeNode,
        root: NodeKey,
        camera: Vec3,
        strength: f32,
        candidates: &mut Vec<(f32, &'a OctreeNode, bool)>,
    ) {
        let distance = node_bounds(node)
            .distance_to_point(camera)
            .max(node.size * 0.25);
        if node.children.is_none() {
            if (node.key == root && node.may_contain_surface)
                || should_split(node, camera, 32.0, strength)
            {
                candidates.push((node.size / distance, node, true));
            }
        } else {
            if node.key != root && should_merge(node, camera, 32.0, strength) {
                // Prefer a large eligible subtree over rebuilding its intermediate LODs.
                // The inverse ratio favored tiny distant merges and serialized the retreat.
                candidates.push((node.size / distance, node, false));
            }
            for child in node.children.as_ref().unwrap() {
                gather(child, root, camera, strength, candidates);
            }
        }
    }
    let mut candidates = Vec::new();
    gather(root, root.key, camera, strength, &mut candidates);
    // Coarsening must not wait behind refinement elsewhere on the planet.
    candidates.sort_by(|a, b| {
        a.2.cmp(&b.2)
            .then_with(|| b.0.total_cmp(&a.0))
            .then_with(|| a.1.key.cmp(&b.1.key))
    });
    for (_, node, split) in candidates {
        if !split {
            let mut neighbors = Vec::new();
            collect_face_neighbor_leaves(root, node.min, node.size, &mut neighbors);
            // A merged leaf must not touch leaves more than one level finer.
            if neighbors.iter().all(|n| n.size >= node.size * 0.5) && available(node) {
                return Some((node.key, false));
            }
        } else {
            let target = balanced_split_target(root, node);
            if available(target) {
                return Some((target.key, true));
            }
        }
    }
    // Once validated, local split/merge checks preserve balance. Camera motion
    // and density edits do not themselves change the topology.
    if !repair_balance {
        return None;
    }
    // Repair legacy gaps only after useful camera-driven work. Previously this
    // scanned every face before every merge and forced refinement while retreating.
    engine::profile_scope!("terrain.octree.repair_balance");
    let mut leaves = Vec::new();
    collect_leaf_nodes(root, &mut leaves);
    for leaf in leaves {
        let mut neighbors = Vec::new();
        collect_face_neighbor_leaves(root, leaf.min, leaf.size, &mut neighbors);
        if let Some(coarse) = neighbors.into_iter().find(|n| n.size > leaf.size * 2.0) {
            let target = balanced_split_target(root, coarse);
            if available(target) {
                return Some((target.key, true));
            }
        }
    }
    None
}

fn balanced_split_target<'a>(root: &'a OctreeNode, mut node: &'a OctreeNode) -> &'a OctreeNode {
    loop {
        let mut neighbors = Vec::new();
        collect_face_neighbor_leaves(root, node.min, node.size, &mut neighbors);
        if let Some(coarser) = neighbors
            .into_iter()
            .filter(|n| n.size > node.size)
            .max_by(|a, b| a.size.total_cmp(&b.size))
        {
            node = coarser;
        } else {
            return node;
        }
    }
}

pub fn rendered_overlap(
    root: &OctreeNode,
    rendered: &std::collections::HashSet<NodeKey>,
) -> Option<(NodeKey, NodeKey)> {
    fn visit(
        node: &OctreeNode,
        rendered: &std::collections::HashSet<NodeKey>,
        ancestor: Option<NodeKey>,
    ) -> Option<(NodeKey, NodeKey)> {
        let mut ancestor = ancestor;
        if rendered.contains(&node.key) {
            if let Some(parent) = ancestor {
                return Some((parent, node.key));
            }
            ancestor = Some(node.key);
        }
        node.children
            .as_ref()?
            .iter()
            .find_map(|child| visit(child, rendered, ancestor))
    }
    visit(root, rendered, None)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use game_types::planet::PlanetTerrainEdits;

    use super::*;
    use crate::systems::universe::planet_system::default_planet_terrain_config;

    fn test_leaf(min: Vec3, size: f32, level: i8) -> OctreeNode {
        OctreeNode {
            key: NodeKey {
                level,
                x: min.x as i32,
                y: min.y as i32,
                z: min.z as i32,
            },
            min,
            size,
            children: None,
            vertex: None,
            density_range: DensityRange::new(-1.0, 1.0),
            may_contain_surface: true,
            state: NodeState::Leaf,
        }
    }

    #[test]
    #[ignore = "manual surface-to-space scheduling benchmark"]
    fn measure_surface_to_space_scheduling() {
        let config = crate::systems::universe::planet_system::earth_like_planet_terrain_config();
        let edits = PlanetTerrainEdits {
            modified_chunks: Default::default(),
            modified_ranges: Default::default(),
        };
        let start = std::time::Instant::now();
        let mut root = build_node(
            Vec3::splat(-8388608.0),
            16777216.0,
            32.0,
            true,
            &vec3(config.radius + 32.0, 0.0, 0.0),
            Vec3::ZERO,
            &config,
            1.0,
            &edits,
        );
        println!("initial build {:?}", start.elapsed());
        for (phase, camera) in [
            ("surface", vec3(config.radius + 32.0, 0.0, 0.0)),
            ("space", vec3(config.radius * 2.0, 0.0, 0.0)),
        ] {
            let mut splits = 0;
            let mut merges = 0;
            let start = std::time::Instant::now();
            for batch in 0..2000 {
                let mut changes = Vec::new();
                update(
                    &mut root,
                    camera,
                    Entity::PLACEHOLDER,
                    Vec3::ZERO,
                    &config,
                    1.0,
                    &mut changes,
                    &edits,
                );
                if changes.is_empty() {
                    println!(
                        "{phase} converged at {batch}: splits={splits} merges={merges} {:?}",
                        start.elapsed()
                    );
                    break;
                }
                for change in changes {
                    if let OctreeChanges::ReplaceMeshes {
                        transition_key,
                        completed_state,
                        additional_transitions,
                        ..
                    } = change
                    {
                        if matches!(completed_state, NodeState::Internal) {
                            splits += 1;
                        } else {
                            merges += 1 + additional_transitions.len();
                        }
                        find_node_mut(&mut root, transition_key).unwrap().state = completed_state;
                        for (key, state) in additional_transitions {
                            find_node_mut(&mut root, key).unwrap().state = state;
                        }
                    }
                }
                if batch % 100 == 0 {
                    println!(
                        "{phase} batch={batch} splits={splits} merges={merges} {:?}",
                        start.elapsed()
                    );
                }
            }
        }
    }

    fn test_split(node: &mut OctreeNode) {
        let size = node.size * 0.5;
        node.children = Some(std::array::from_fn(|i| {
            Box::new(test_leaf(
                node.min
                    + vec3((i & 1) as f32, ((i >> 1) & 1) as f32, ((i >> 2) & 1) as f32) * size,
                size,
                node.key.level + 1,
            ))
        }));
        node.state = NodeState::Internal;
    }

    #[test]
    fn uniform_feedback_stops_refinement_until_an_edit_refreshes_the_leaf() {
        let mut root = test_leaf(Vec3::new(200.0, 0.0, 0.0), 64.0, 0);
        let key = root.key;
        let mut edits = PlanetTerrainEdits {
            modified_chunks: HashMap::new(),
            modified_ranges: HashMap::new(),
        };
        apply_uniform_proofs(&mut root, &[key], &edits);
        assert!(!should_split(&root, root.min, 32.0, 1.0));
        let mut config =
            crate::systems::universe::planet_system::earth_like_planet_terrain_config();
        config.radius = 100.0;
        config.field_graph.as_mut().unwrap().layers.clear();
        let brick = TerrainBrickKey {
            x: 6,
            y: 0,
            z: 0,
            level: 0,
        };
        edits.modified_chunks.insert(
            brick,
            std::sync::Arc::new(vec![vec![vec![-200.0; 2]; 2]; 2]),
        );
        edits
            .modified_ranges
            .insert(brick, DensityRange::new(-200.0, 0.0));
        refresh_density_ranges_in_bounds(
            &mut root,
            Vec3::new(192.0, 0.0, 0.0),
            Vec3::new(224.0, 32.0, 32.0),
            Vec3::ZERO,
            &config,
            &edits,
        );
        assert!(root.may_contain_surface);
        apply_uniform_proofs(&mut root, &[key], &edits);
        assert!(
            root.may_contain_surface,
            "an old worker proof cannot override edits"
        );
        assert!(should_split(&root, root.min, 32.0, 1.0));
    }

    #[test]
    fn independent_replacements_preserve_balance_in_either_completion_order() {
        fn run(reverse: bool) -> std::collections::HashSet<NodeKey> {
            let mut root = test_leaf(Vec3::splat(-512.0), 1024.0, 0);
            test_split(&mut root);
            for child in root.children.as_mut().unwrap() {
                test_split(child);
                for child in child.children.as_mut().unwrap() {
                    test_split(child);
                }
            }
            let mut config = default_planet_terrain_config();
            config.radius = 400.0;
            let edits = PlanetTerrainEdits {
                modified_chunks: HashMap::new(),
                modified_ranges: HashMap::new(),
            };
            let mut pending: Vec<(OctreeChanges, Vec<Aabb>)> = Vec::new();
            let mut leaves = Vec::new();
            collect_leaf_nodes(&root, &mut leaves);
            let mut rendered: std::collections::HashSet<_> = leaves.iter().map(|n| n.key).collect();
            let mut concurrent = false;
            for _ in 0..300 {
                let reserved: Vec<_> = pending
                    .iter()
                    .flat_map(|(_, regions)| regions.iter().copied())
                    .collect();
                if pending.len() < 4 {
                    let mut changes = Vec::new();
                    update_reserved(
                        &mut root,
                        Vec3::new(400.0, 0.0, 0.0),
                        Entity::PLACEHOLDER,
                        Vec3::ZERO,
                        &config,
                        0.5,
                        &mut changes,
                        &edits,
                        &reserved,
                        true,
                    );
                    for change in changes {
                        let footprint = replacement_footprint(&root, &change);
                        assert!(
                            !footprint
                                .iter()
                                .any(|a| reserved.iter().any(|b| touches(a, b)))
                        );
                        pending.push((change, footprint));
                    }
                }
                concurrent |= pending.len() > 1;
                if pending.is_empty() {
                    break;
                }
                // Give the planner a chance to fill independent slots before completing work.
                if pending.len() < 4
                    && next_lod_change_reserved(
                        &root,
                        Vec3::new(400.0, 0.0, 0.0),
                        0.5,
                        &pending
                            .iter()
                            .flat_map(|(_, regions)| regions.iter().copied())
                            .collect::<Vec<_>>(),
                        true,
                    )
                    .is_some()
                {
                    continue;
                }
                let index = if reverse { pending.len() - 1 } else { 0 };
                let (change, _) = pending.remove(index);
                let OctreeChanges::ReplaceMeshes {
                    transition_key,
                    completed_state,
                    additional_transitions,
                    keys_to_remove,
                    requests,
                    ..
                } = change
                else {
                    unreachable!()
                };
                if matches!(completed_state, NodeState::Internal) {
                    assert_eq!(
                        keys_to_remove.len(),
                        1,
                        "a refinement batch must not wait for unrelated regions"
                    );
                }
                for key in keys_to_remove {
                    rendered.remove(&key);
                }
                for request in requests {
                    assert!(
                        find_node_mut(&mut root, request.node_key)
                            .unwrap()
                            .children
                            .is_none()
                    );
                    rendered.insert(request.node_key);
                }
                for (key, state) in
                    std::iter::once((transition_key, completed_state)).chain(additional_transitions)
                {
                    find_node_mut(&mut root, key).unwrap().state = state;
                }
                assert_eq!(rendered_overlap(&root, &rendered), None);
                let mut leaves = Vec::new();
                collect_leaf_nodes(&root, &mut leaves);
                for leaf in leaves {
                    let mut neighbors = Vec::new();
                    collect_face_neighbor_leaves(&root, leaf.min, leaf.size, &mut neighbors);
                    assert!(
                        neighbors
                            .iter()
                            .all(|n| (n.key.level - leaf.key.level).abs() <= 1)
                    );
                }
            }
            assert!(
                concurrent,
                "independent regions should generate concurrently"
            );
            assert!(pending.is_empty(), "stationary camera must converge");
            assert!(next_lod_change(&root, Vec3::new(400.0, 0.0, 0.0), 0.5).is_none());
            rendered
        }
        assert_eq!(run(false), run(true));
    }

    #[test]
    fn validated_tree_skips_legacy_repair_but_still_selects_lod_changes() {
        let mut root = test_leaf(Vec3::ZERO, 512.0, 0);
        test_split(&mut root);
        let camera = Vec3::splat(250.0);
        assert_eq!(
            next_lod_change_reserved(&root, camera, 1.0, &[], false),
            next_lod_change_reserved(&root, camera, 1.0, &[], true)
        );
        // An imported, unbalanced tree needs validation. Ordinary updates preserve
        // this invariant and can skip the fallback once validation has completed.
        test_split(&mut root.children.as_mut().unwrap()[0]);
        test_split(
            &mut root.children.as_mut().unwrap()[0]
                .children
                .as_mut()
                .unwrap()[1],
        );
        fn disable_surface(node: &mut OctreeNode) {
            node.may_contain_surface = false;
            if let Some(children) = &mut node.children {
                for child in children {
                    disable_surface(child);
                }
            }
        }
        disable_surface(&mut root);
        assert!(next_lod_change_reserved(&root, camera, 10000.0, &[], true).is_some());
        assert!(next_lod_change_reserved(&root, camera, 10000.0, &[], false).is_none());
    }

    #[test]
    #[ignore = "manual idle balance-scan benchmark"]
    fn measure_validated_idle_selection() {
        fn populate(node: &mut OctreeNode, depth: usize) {
            node.may_contain_surface = false;
            if depth == 0 {
                return;
            }
            test_split(node);
            for child in node.children.as_mut().unwrap() {
                populate(child, depth - 1);
            }
        }
        let mut root = test_leaf(Vec3::ZERO, 1024.0, 0);
        populate(&mut root, 4); // 4096 balanced leaves, no LOD work.
        for repair in [true, false] {
            let start = std::time::Instant::now();
            for _ in 0..20 {
                assert!(
                    next_lod_change_reserved(&root, Vec3::splat(512.0), 10000.0, &[], repair)
                        .is_none()
                );
            }
            println!("repair={repair} mean={:?}", start.elapsed() / 20);
        }
    }

    #[test]
    fn lod_priority_follows_camera_instead_of_child_order() {
        let mut root = test_leaf(Vec3::ZERO, 128.0, 0);
        test_split(&mut root);
        let last = root.children.as_ref().unwrap()[7].key;
        assert_eq!(
            next_lod_change(&root, Vec3::splat(112.0), 1.0),
            Some((last, true))
        );
    }

    #[test]
    fn retreat_merges_eligible_ancestor_before_intermediate_levels() {
        let mut root = test_leaf(Vec3::ZERO, 512.0, 0);
        test_split(&mut root);
        let parent = &mut root.children.as_mut().unwrap()[0];
        test_split(parent);
        test_split(&mut parent.children.as_mut().unwrap()[0]);
        let expected = parent.key;
        assert_eq!(
            next_lod_change(&root, Vec3::splat(10000.0), 1.0),
            Some((expected, false))
        );
    }

    #[test]
    fn retreat_batches_merges_and_preserves_all_removals() {
        let mut root = test_leaf(Vec3::ZERO, 512.0, 0);
        test_split(&mut root);
        for child in root.children.as_mut().unwrap() {
            test_split(child);
        }
        let mut old_leaves = Vec::new();
        collect_leaf_nodes(&root, &mut old_leaves);
        let old_keys: std::collections::HashSet<_> = old_leaves.iter().map(|n| n.key).collect();
        let config = default_planet_terrain_config();
        let edits = PlanetTerrainEdits {
            modified_chunks: HashMap::new(),
            modified_ranges: HashMap::new(),
        };
        let mut changes = Vec::new();
        update(
            &mut root,
            Vec3::splat(10000.0),
            Entity::PLACEHOLDER,
            Vec3::ZERO,
            &config,
            1.0,
            &mut changes,
            &edits,
        );
        let [
            OctreeChanges::ReplaceMeshes {
                keys_to_remove,
                requests,
                additional_transitions,
                ..
            },
        ] = changes.as_slice()
        else {
            panic!("coarsening must produce one atomic batch");
        };
        assert_eq!(requests.len(), 8);
        assert_eq!(additional_transitions.len(), 7);
        assert_eq!(
            keys_to_remove
                .iter()
                .copied()
                .collect::<std::collections::HashSet<_>>(),
            old_keys
        );
        assert!(
            root.children
                .as_ref()
                .unwrap()
                .iter()
                .all(|n| n.children.is_none())
        );
        assert!(
            has_pending_transition(&root),
            "wait for the GPU acknowledgment"
        );
    }

    #[test]
    fn repeated_merge_batches_leave_no_stale_meshes_or_level_gaps() {
        let mut root = test_leaf(Vec3::ZERO, 512.0, 0);
        test_split(&mut root);
        for child in root.children.as_mut().unwrap() {
            test_split(child);
            for grandchild in child.children.as_mut().unwrap() {
                test_split(grandchild);
            }
        }
        let mut leaves = Vec::new();
        collect_leaf_nodes(&root, &mut leaves);
        let mut rendered: std::collections::HashSet<_> = leaves.iter().map(|n| n.key).collect();
        let config = default_planet_terrain_config();
        let edits = PlanetTerrainEdits {
            modified_chunks: HashMap::new(),
            modified_ranges: HashMap::new(),
        };
        for _ in 0..10 {
            let mut changes = Vec::new();
            update(
                &mut root,
                Vec3::splat(10000.0),
                Entity::PLACEHOLDER,
                Vec3::ZERO,
                &config,
                1.0,
                &mut changes,
                &edits,
            );
            for change in changes {
                let OctreeChanges::ReplaceMeshes {
                    transition_key,
                    additional_transitions,
                    keys_to_remove,
                    requests,
                    ..
                } = change
                else {
                    panic!()
                };
                for key in keys_to_remove {
                    rendered.remove(&key);
                }
                rendered.extend(requests.iter().map(|r| r.node_key));
                for key in std::iter::once(transition_key)
                    .chain(additional_transitions.into_iter().map(|(key, _)| key))
                {
                    find_node_mut(&mut root, key).unwrap().state = NodeState::Leaf;
                }
            }
            assert_eq!(rendered_overlap(&root, &rendered), None);
            let mut leaves = Vec::new();
            collect_leaf_nodes(&root, &mut leaves);
            assert_eq!(rendered, leaves.iter().map(|n| n.key).collect());
            for leaf in leaves {
                let mut neighbors = Vec::new();
                collect_face_neighbor_leaves(&root, leaf.min, leaf.size, &mut neighbors);
                assert!(
                    neighbors
                        .iter()
                        .all(|n| (n.key.level - leaf.key.level).abs() <= 1)
                );
            }
        }
        assert_eq!(
            rendered.len(),
            8,
            "retreat should reach the eight root children within ten batches"
        );
    }

    #[test]
    fn lod_changes_preserve_face_balance_as_camera_moves() {
        let mut root = test_leaf(Vec3::ZERO, 512.0, 0);
        test_split(&mut root);
        let mut splits = 0;
        let mut merges = 0;
        for camera in [Vec3::splat(240.0), Vec3::splat(400.0), Vec3::splat(10000.0)] {
            for _ in 0..64 {
                let Some((key, split)) = next_lod_change(&root, camera, 0.5) else {
                    break;
                };
                let node = find_node_mut(&mut root, key).unwrap();
                if split {
                    test_split(node);
                    splits += 1;
                } else {
                    node.children = None;
                    node.state = NodeState::Leaf;
                    merges += 1;
                }
                let mut leaves = Vec::new();
                collect_leaf_nodes(&root, &mut leaves);
                for leaf in leaves {
                    let mut neighbors = Vec::new();
                    collect_face_neighbor_leaves(&root, leaf.min, leaf.size, &mut neighbors);
                    assert!(
                        neighbors
                            .iter()
                            .all(|n| (n.key.level - leaf.key.level).abs() <= 1)
                    );
                }
            }
        }
        assert!(splits > 0 && merges > 0);
    }

    #[test]
    fn rendered_overlap_detects_ancestor_but_allows_adjacent_leaves() {
        let mut root = test_leaf(Vec3::ZERO, 128.0, 0);
        test_split(&mut root);
        let children = root.children.as_ref().unwrap();
        let mut rendered = std::collections::HashSet::from([children[0].key, children[1].key]);
        assert_eq!(rendered_overlap(&root, &rendered), None);
        rendered.insert(root.key);
        assert_eq!(
            rendered_overlap(&root, &rendered),
            Some((root.key, children[0].key))
        );
    }

    #[test]
    fn retreat_coarsens_instead_of_refining_to_repair_an_old_gap() {
        let mut root = test_leaf(Vec3::ZERO, 512.0, 0);
        test_split(&mut root);
        test_split(&mut root.children.as_mut().unwrap()[0]);
        test_split(
            &mut root.children.as_mut().unwrap()[0]
                .children
                .as_mut()
                .unwrap()[1],
        );
        let (key, split) = next_lod_change(&root, Vec3::splat(10000.0), 1.0).unwrap();
        assert!(
            !split,
            "an eligible merge should replace unnecessary refinement"
        );
        assert_eq!(key.level, 1, "coarsen the detailed subtree directly");
    }

    #[test]
    fn refinement_is_bounded_and_waits_for_visible_replacement() {
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
            may_contain_surface: density_range.contains_zero(),
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
                .all(|request| request.node_key.level > node.key.level
                    && request.node_key.level <= node.key.level + SPLITS_PER_REPLACEMENT as i8),
            "refinement depth is bounded by the split budget"
        );
        assert!(requests.len() <= 1 + 7 * SPLITS_PER_REPLACEMENT);

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

        assert!(
            changes.is_empty(),
            "camera motion must not supersede an unacknowledged replacement"
        );
        assert!(matches!(node.state, NodeState::Splitting));
    }
}
