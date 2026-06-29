use std::sync::atomic::AtomicU32;

use cgmath::{InnerSpace, Point3, Vector3, point3, vec3};
use engine::ecs::entity::Entity;
use game_types::{
    octree::{NodeState, OctreeChanges, OctreeNode, PlanetMeshRequest},
    planet::PlanetTerrainEdits,
    terrain,
};

use crate::{
    NodeKey,
    sdf::{EarthHeightmap, sdf_at_center},
};

const THRESHOLD: f32 = 0.3;

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

pub fn is_behind_horizon(
    node_center: Vector3<f32>,
    camera_pos: Vector3<f32>,
    planet_center: Vector3<f32>,
) -> bool {
    let to_node = (node_center - planet_center).normalize();
    let to_camera = (camera_pos - planet_center).normalize();
    cgmath::dot(to_node, to_camera) < 0.0
}

pub fn should_subdivide(node: OctreeNode, camera_pos: Vector3<f32>) -> bool {
    let center = node.min + vec3(node.size * 0.5, node.size * 0.5, node.size * 0.5);
    let dist = (center - camera_pos).magnitude();
    let error = node.size / dist;
    error > THRESHOLD
}

pub fn build_node(
    min: Vector3<f32>,
    size: f32,
    min_size: f32,
    first: bool,
    camera_position: &cgmath::Point3<f32>,
    planet_center: Vector3<f32>,
    planet_size: u32,
    heightmap: Option<&EarthHeightmap>,
    terrain_edits: &PlanetTerrainEdits,
) -> OctreeNode {
    let has_surface = has_surface(
        min,
        size,
        planet_center,
        planet_size,
        heightmap,
        terrain_edits,
    );
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
            has_surface: false,
            state: NodeState::Leaf,
        };

        if !has_surface && size < planet_size as f32 / 4.0 {
            return leaf;
        }

        if size <= min_size
            || !should_subdivide(
                leaf,
                vec3(camera_position.x, camera_position.y, camera_position.z),
            )
        {
            return OctreeNode {
                key,
                min,
                size,
                children: None,
                vertex: None,
                has_surface,
                state: NodeState::Leaf,
            };
        }
    }

    let child_size = size / 2.0;
    let children = Some([
        Box::new(build_node(
            min + vec3(0.0, 0.0, 0.0),
            child_size,
            min_size,
            false,
            camera_position,
            planet_center,
            planet_size,
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
            heightmap,
            terrain_edits,
        )),
    ]);

    OctreeNode {
        key,
        min,
        size,
        children,
        vertex: None,
        has_surface,
        state: NodeState::Internal,
    }
}

pub fn has_surface(
    min: Vector3<f32>,
    size: f32,
    planet_center: Vector3<f32>,
    planet_size: u32,
    heightmap: Option<&EarthHeightmap>,
    terrain_edits: &PlanetTerrainEdits,
) -> bool {
    let mut has_neg = false;
    let mut has_pos = false;
    for dx in [0.0, size] {
        for dy in [0.0, size] {
            for dz in [0.0, size] {
                let p = min + vec3(dx, dy, dz);
                let d = sdf_at_center(p, planet_center, planet_size, heightmap, terrain_edits);
                if d < 0.0 {
                    has_neg = true;
                }
                if d > 0.0 {
                    has_pos = true;
                }
            }
        }
    }
    has_neg && has_pos
}

pub fn collect_octree_nodes_at_depth(
    node: &OctreeNode,
    current_depth: u32,
    target_depth: u32,
    out: &mut Vec<(Point3<f32>, f32, u32)>,
) {
    if current_depth == target_depth {
        let half = node.size / 2.0;
        let center = Point3::new(node.min.x + half, node.min.y + half, node.min.z + half);
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
    out: &mut Vec<(Point3<f32>, f32, u32)>,
) {
    let half = node.size / 2.0;
    let center = Point3::new(node.min.x + half, node.min.y + half, node.min.z + half);
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
    pub min: cgmath::Vector3<f32>,
    pub max: cgmath::Vector3<f32>,
}

impl Aabb {
    pub fn size(&self) -> f32 {
        (self.max.x - self.min.x)
            .max(self.max.y - self.min.y)
            .max(self.max.z - self.min.z)
    }

    pub fn center(&self) -> cgmath::Vector3<f32> {
        (self.min + self.max) * 0.5
    }

    pub fn distance_to_point(&self, p: cgmath::Vector3<f32>) -> f32 {
        let closest = vec3(
            p.x.clamp(self.min.x, self.max.x),
            p.y.clamp(self.min.y, self.max.y),
            p.z.clamp(self.min.z, self.max.z),
        );
        (p - closest).magnitude()
    }
}

pub fn update(
    node: &mut OctreeNode,
    camera_pos: Vector3<f32>,
    planet_entity: Entity,
    planet_position: Vector3<f32>,
    planet_size: u32,
    changes: &mut Vec<OctreeChanges>,
    heightmap: Option<&EarthHeightmap>,
    terrain_edits: &PlanetTerrainEdits,
) {
    let min_node_size = 32.0;
    let is_root_node = node.size >= planet_size as f32 * 0.5;

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

    if should_split(node, camera_pos, min_node_size, planet_size) {
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

    if !is_root_node && should_merge(node, camera_pos, min_node_size) {
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
                changes,
                heightmap,
                terrain_edits,
            );
        }
    }
}

pub fn create_children(
    parent: &OctreeNode,
    planet_position: Vector3<f32>,
    planet_size: u32,
    heightmap: Option<&EarthHeightmap>,
    terrain_edits: &PlanetTerrainEdits,
) -> [Box<OctreeNode>; 8] {
    let bounds = node_bounds(parent);
    let min = bounds.min;
    let mid = bounds.center();
    let child_size = parent.size * 0.5;

    let make_child = |min: cgmath::Vector3<f32>| {
        let has_surface = has_surface(
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
            has_surface,
            state: NodeState::Leaf,
        }
    };

    [
        Box::new(make_child(cgmath::vec3(min.x, min.y, min.z))),
        Box::new(make_child(cgmath::vec3(mid.x, min.y, min.z))),
        Box::new(make_child(cgmath::vec3(min.x, mid.y, min.z))),
        Box::new(make_child(cgmath::vec3(mid.x, mid.y, min.z))),
        Box::new(make_child(cgmath::vec3(min.x, min.y, mid.z))),
        Box::new(make_child(cgmath::vec3(mid.x, min.y, mid.z))),
        Box::new(make_child(cgmath::vec3(min.x, mid.y, mid.z))),
        Box::new(make_child(cgmath::vec3(mid.x, mid.y, mid.z))),
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
            changes.push(OctreeChanges::RemoveMesh { key: child.key });
        }
    }
}

pub fn node_bounds(node: &OctreeNode) -> Aabb {
    Aabb {
        min: node.min,
        max: node.min + vec3(node.size, node.size, node.size),
    }
}

pub fn should_split(
    node: &OctreeNode,
    camera_pos: cgmath::Vector3<f32>,
    min_node_size: f32,
    planet_size: u32,
) -> bool {
    if node.children.is_some() {
        return false;
    }

    if !node.has_surface && node.size < planet_size as f32 / 4.0 {
        return false;
    }

    let bounds = node_bounds(node);

    if bounds.size() <= min_node_size {
        return false;
    }

    let distance = bounds.distance_to_point(camera_pos);
    let split_distance = bounds.size() * 4.0;

    distance < split_distance
}

pub fn should_merge(
    node: &OctreeNode,
    camera_pos: cgmath::Vector3<f32>,
    min_node_size: f32,
) -> bool {
    if node.children.is_none() {
        return false;
    }

    let bounds = node_bounds(node);

    if bounds.size() <= min_node_size {
        return false;
    }

    let distance = bounds.distance_to_point(camera_pos);
    let merge_distance = bounds.size() * 6.0;

    distance > merge_distance
}

fn mesh_request(
    node: &OctreeNode,
    planet_entity: Entity,
    planet_position: Vector3<f32>,
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
    planet_position: Vector3<f32>,
    planet_size: u32,
    heightmap: Option<&EarthHeightmap>,
    terrain_edits: &PlanetTerrainEdits,
) {
    changes.push(OctreeChanges::RemoveMesh { key: node.key });

    let children = create_children(node, planet_position, planet_size, heightmap, terrain_edits);

    for child in children.iter() {
        if child.has_surface {
            changes.push(OctreeChanges::AddMesh {
                request: mesh_request(child, planet_entity, planet_position, planet_size),
            });
        }
    }

    node.state = NodeState::Internal;
    node.children = Some(children);
}

fn merge_node(
    node: &mut OctreeNode,
    changes: &mut Vec<OctreeChanges>,
    planet_entity: Entity,
    planet_position: Vector3<f32>,
    planet_size: u32,
) {
    if let Some(children) = node.children.as_ref() {
        collect_child_mesh_removals(children, changes);
    }

    node.children = None;
    node.state = NodeState::Leaf;

    if node.has_surface {
        changes.push(OctreeChanges::AddMesh {
            request: mesh_request(node, planet_entity, planet_position, planet_size),
        });
    }
}

pub fn ray_intersects(
    node: &OctreeNode,
    ray_origin: Point3<f32>,
    ray_direction: Vector3<f32>,
) -> Option<(f32, f32)> {
    let inv_dir = vec3(
        1.0 / ray_direction.x,
        1.0 / ray_direction.y,
        1.0 / ray_direction.z,
    );
    let max = point3(
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
    ray_origin: Point3<f32>,
    ray_direction: Vector3<f32>,
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

    children.sort_by(|a, b| a.1.total_cmp(&b.1));

    for (child, _) in children {
        traverse_octree(ray_origin, ray_direction, child, best_t, last_node);
    }
}
