use std::sync::atomic::AtomicU32;

use engine::ecs::entity::Entity;
use engine::math::{Vec3, vec3};
use game_types::{
    octree::{DensityRange, NodeState, OctreeChanges, OctreeNode, PlanetMeshRequest},
    planet::{PlanetTerrainEdits, TerrainBrickKey, TerrainBrickSamples},
};

use crate::{
    NodeKey,
    sdf::{EarthHeightmap, TERRAIN_EDIT_BRICK_SIZE, planet_radius, terrain_height_bounds},
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
    planet_size: u32,
    lod_strength: f32,
    heightmap: Option<&EarthHeightmap>,
    terrain_edits: &PlanetTerrainEdits,
) -> OctreeNode {
    let density_range = node_density_range(
        min,
        size,
        planet_center,
        planet_size,
        heightmap,
        terrain_edits,
    );
    let has_surface = density_range.contains_zero();
    let _is_behind_horizon = is_behind_horizon(
        min + vec3(size * 0.5, size * 0.5, size * 0.5),
        vec3(camera_position.x, camera_position.y, camera_position.z),
        planet_center,
    );
    let key = NodeKey {
        x: min.x as i32,
        y: min.y as i32,
        z: min.z as i32,
        size: size as i32,
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
    let children = [
        Box::new(build_node(
            min + vec3(0.0, 0.0, 0.0),
            child_size,
            min_size,
            false,
            camera_position,
            planet_center,
            planet_size,
            lod_strength,
            heightmap,
            terrain_edits,
        )),
        Box::new(build_node(
            min + vec3(child_size, 0.0, 0.0),
            child_size,
            min_size,
            false,
            camera_position,
            planet_center,
            planet_size,
            lod_strength,
            heightmap,
            terrain_edits,
        )),
        Box::new(build_node(
            min + vec3(0.0, child_size, 0.0),
            child_size,
            min_size,
            false,
            camera_position,
            planet_center,
            planet_size,
            lod_strength,
            heightmap,
            terrain_edits,
        )),
        Box::new(build_node(
            min + vec3(child_size, child_size, 0.0),
            child_size,
            min_size,
            false,
            camera_position,
            planet_center,
            planet_size,
            lod_strength,
            heightmap,
            terrain_edits,
        )),
        Box::new(build_node(
            min + vec3(0.0, 0.0, child_size),
            child_size,
            min_size,
            false,
            camera_position,
            planet_center,
            planet_size,
            lod_strength,
            heightmap,
            terrain_edits,
        )),
        Box::new(build_node(
            min + vec3(child_size, 0.0, child_size),
            child_size,
            min_size,
            false,
            camera_position,
            planet_center,
            planet_size,
            lod_strength,
            heightmap,
            terrain_edits,
        )),
        Box::new(build_node(
            min + vec3(0.0, child_size, child_size),
            child_size,
            min_size,
            false,
            camera_position,
            planet_center,
            planet_size,
            lod_strength,
            heightmap,
            terrain_edits,
        )),
        Box::new(build_node(
            min + vec3(child_size, child_size, child_size),
            child_size,
            min_size,
            false,
            camera_position,
            planet_center,
            planet_size,
            lod_strength,
            heightmap,
            terrain_edits,
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
    planet_size: u32,
    heightmap: Option<&EarthHeightmap>,
) -> DensityRange {
    let max = min + Vec3::splat(size);
    let closest = vec3(
        planet_center.x.clamp(min.x, max.x),
        planet_center.y.clamp(min.y, max.y),
        planet_center.z.clamp(min.z, max.z),
    );
    let min_radius = (closest - planet_center).length();

    let local_min = min - planet_center;
    let local_max = max - planet_center;
    let farthest = vec3(
        local_min.x.abs().max(local_max.x.abs()),
        local_min.y.abs().max(local_max.y.abs()),
        local_min.z.abs().max(local_max.z.abs()),
    );
    let max_radius = farthest.length();

    let radius = planet_radius(planet_size);
    let (min_height, max_height) = terrain_height_bounds(planet_size, heightmap);
    DensityRange::new(
        min_radius - (radius + max_height),
        max_radius - (radius + min_height),
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
    planet_size: u32,
    heightmap: Option<&EarthHeightmap>,
    terrain_edits: &PlanetTerrainEdits,
) -> DensityRange {
    base_density_range(min, size, planet_center, planet_size, heightmap).add(edit_density_range(
        min - planet_center,
        size,
        terrain_edits,
    ))
}

pub fn has_surface(
    min: Vec3,
    size: f32,
    planet_center: Vec3,
    planet_size: u32,
    heightmap: Option<&EarthHeightmap>,
    terrain_edits: &PlanetTerrainEdits,
) -> bool {
    node_density_range(
        min,
        size,
        planet_center,
        planet_size,
        heightmap,
        terrain_edits,
    )
    .contains_zero()
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
    planet_size: u32,
    lod_strength: f32,
    changes: &mut Vec<OctreeChanges>,
    heightmap: Option<&EarthHeightmap>,
    terrain_edits: &PlanetTerrainEdits,
) {
    let min_node_size = 32.0;
    let is_root_node = node.size >= planet_size as f32 * 0.5;

    // Do not refine a topology that has not become visible yet. Otherwise a
    // child replacement can supersede its parent replacement before anything
    // takes responsibility for removing the currently rendered ancestor.
    if matches!(node.state, NodeState::Splitting | NodeState::Merging) {
        return;
    }

    if is_root_node && node.children.is_none() {
        split_node(
            node,
            changes,
            planet_entity,
            planet_position,
            planet_size,
            heightmap,
            terrain_edits,
        );
        return;
    }

    if should_split(node, camera_pos, min_node_size, lod_strength) {
        split_node(
            node,
            changes,
            planet_entity,
            planet_position,
            planet_size,
            heightmap,
            terrain_edits,
        );
        return;
    }

    if !is_root_node && should_merge(node, camera_pos, min_node_size, lod_strength) {
        merge_node(node, changes, planet_entity, planet_position, planet_size);
        return;
    }

    if let Some(children) = node.children.as_mut() {
        for child in children.iter_mut() {
            update(
                child,
                camera_pos,
                planet_entity,
                planet_position,
                planet_size,
                lod_strength,
                changes,
                heightmap,
                terrain_edits,
            );
        }
    }
}

pub fn create_children(
    parent: &OctreeNode,
    planet_position: Vec3,
    planet_size: u32,
    heightmap: Option<&EarthHeightmap>,
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
            planet_size,
            heightmap,
            terrain_edits,
        );
        OctreeNode {
            key: NodeKey {
                x: min.x as i32,
                y: min.y as i32,
                z: min.z as i32,
                size: child_size as i32,
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
    children: &[Box<OctreeNode>; 8],
    changes: &mut Vec<OctreeChanges>,
) {
    for child in children {
        if let Some(grandchildren) = child.children.as_ref() {
            collect_child_mesh_removals(grandchildren, changes);
        } else {
            changes.push(OctreeChanges::RemoveMeshes { key: child.key });
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
    planet_size: u32,
    heightmap: Option<&EarthHeightmap>,
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
                planet_size,
                heightmap,
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
            planet_size,
            heightmap,
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
    planet_size: u32,
) -> PlanetMeshRequest {
    PlanetMeshRequest {
        planet_entity,
        planet_position,
        planet_size,
        node_min_corner: node.min,
        node_size: node.size,
    }
}

fn split_node(
    node: &mut OctreeNode,
    changes: &mut Vec<OctreeChanges>,
    planet_entity: Entity,
    planet_position: Vec3,
    planet_size: u32,
    heightmap: Option<&EarthHeightmap>,
    terrain_edits: &PlanetTerrainEdits,
) {
    let children = create_children(node, planet_position, planet_size, heightmap, terrain_edits);

    let requests = children
        .iter()
        .filter(|child| child.has_surface)
        .map(|child| mesh_request(child, planet_entity, planet_position, planet_size))
        .collect();

    changes.push(OctreeChanges::ReplaceMeshes {
        planet_entity,
        transition_key: node.key,
        completed_state: NodeState::Internal,
        keys_to_remove: vec![node.key],
        requests,
    });

    node.state = NodeState::Splitting;
    node.children = Some(children);
}

fn merge_node(
    node: &mut OctreeNode,
    changes: &mut Vec<OctreeChanges>,
    planet_entity: Entity,
    planet_position: Vec3,
    planet_size: u32,
) {
    let mut keys_to_remove = Vec::new();

    if let Some(children) = &node.children {
        collect_leaf_keys(children, &mut keys_to_remove);
    }

    let requests = if node.has_surface {
        vec![mesh_request(
            node,
            planet_entity,
            planet_position,
            planet_size,
        )]
    } else {
        Vec::new()
    };

    changes.push(OctreeChanges::ReplaceMeshes {
        planet_entity,
        transition_key: node.key,
        completed_state: NodeState::Leaf,
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
    use std::{collections::HashMap, sync::Arc};

    use game_types::planet::TerrainBrickKey;

    use super::*;

    fn empty_edits() -> PlanetTerrainEdits {
        PlanetTerrainEdits {
            modified_chunks: HashMap::new(),
            modified_ranges: HashMap::new(),
        }
    }

    #[test]
    fn interval_keeps_surface_that_corner_signs_can_miss() {
        let range = node_density_range(
            vec3(99.0, -30.0, -30.0),
            60.0,
            Vec3::ZERO,
            800,
            None,
            &empty_edits(),
        );

        assert!(range.contains_zero());
    }

    #[test]
    fn large_nodes_far_from_terrain_are_pruned_immediately() {
        let edits = empty_edits();
        let min = vec3(500.0, 500.0, 500.0);
        let size = 200.0;
        let camera = min + Vec3::splat(size * 0.5);
        let node = build_node(
            min,
            size,
            32.0,
            false,
            &camera,
            Vec3::ZERO,
            800,
            1.0,
            None,
            &edits,
        );

        assert!(node.density_range.min > 0.0);
        assert!(!node.has_surface);
        assert!(node.children.is_none());
    }

    #[test]
    fn edit_range_can_create_and_propagate_a_surface_candidate() {
        let min = vec3(105.0, 0.0, 0.0);
        let size = 64.0;
        let camera = min + Vec3::splat(size * 0.5);
        let mut node = build_node(
            min,
            size,
            32.0,
            false,
            &camera,
            Vec3::ZERO,
            800,
            1.0,
            None,
            &empty_edits(),
        );
        assert!(!node.has_surface);

        let key = TerrainBrickKey {
            x: 3,
            y: 0,
            z: 0,
            level: 0,
        };
        let samples = Arc::new(vec![vec![vec![-16.0; 2]; 2]; 2]);
        let edits = PlanetTerrainEdits {
            modified_chunks: HashMap::from([(key, samples)]),
            modified_ranges: HashMap::from([(key, DensityRange::new(-16.0, -16.0))]),
        };

        refresh_density_ranges_in_bounds(
            &mut node,
            vec3(96.0, 0.0, 0.0),
            vec3(128.0, 32.0, 32.0),
            Vec3::ZERO,
            800,
            None,
            &edits,
        );

        assert!(node.density_range.contains_zero());
        assert!(node.has_surface);
        assert!(should_split(&node, camera, 32.0, 1.0));
    }
}
