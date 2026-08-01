use std::collections::{HashMap, HashSet};

use engine::math::{Vec3, vec3};
use game_types::{
    octree::{FaceNeighbor, FaceNeighborKind, OctreeNode},
    planet::{Planet, PlanetTerrainEdits, PlanetVertex},
    terrain::PlanetTerrainConfig,
};

use crate::{
    CHUNK_CELL_COUNT, octree,
    sdf::terrain_height_bounds,
    systems::terrain::terrain_sampler::{self, PlanetTerrainSamplerContext},
};

pub trait PlanetExt {
    fn dual_contour_grid(
        grid: &[Vec<Vec<f32>>],
        offset: Vec3,
        resolution: f32,
        terrain: &PlanetTerrainSamplerContext<'_>,
        face_neighbors: &[FaceNeighbor; 6],
    ) -> (Vec<PlanetVertex>, Vec<u32>);
    fn create_octree(
        planet_position: engine::math::Vec3,
        camera_position: &engine::math::Vec3,
        terrain_config: &PlanetTerrainConfig,
        chunk_size: u32,
        lod_strength: f32,
        terrain_edits: &PlanetTerrainEdits,
    ) -> OctreeNode;
    fn collect_leaf_nodes(node: &OctreeNode, current_depth: u32, out: &mut Vec<(Vec3, f32, u32)>);
}

type CellVertexGrid = Vec<Vec<Vec<Option<u32>>>>;

#[inline]
fn append_quad(indices: &mut Vec<u32>, vertices: [u32; 4], flip_winding: bool) {
    let [v0, v1, v2, v3] = vertices;
    if flip_winding {
        indices.extend_from_slice(&[v0, v2, v1, v2, v3, v1]);
    } else {
        indices.extend_from_slice(&[v0, v1, v2, v2, v1, v3]);
    }
}

const CELL_EDGES: [(usize, usize); 12] = [
    (0, 1),
    (1, 3),
    (3, 2),
    (2, 0),
    (4, 5),
    (5, 7),
    (7, 6),
    (6, 4),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

// Avoid large Windows hotpatch stack frames.
#[inline(never)]
fn contour_cell_vertex(
    grid: &[Vec<Vec<f32>>],
    x: usize,
    y: usize,
    z: usize,
    offset: Vec3,
    resolution: f32,
    planet_position: Vec3,
) -> Option<PlanetVertex> {
    let corners = [
        grid[x][y][z],
        grid[x + 1][y][z],
        grid[x][y + 1][z],
        grid[x + 1][y + 1][z],
        grid[x][y][z + 1],
        grid[x + 1][y][z + 1],
        grid[x][y + 1][z + 1],
        grid[x + 1][y + 1][z + 1],
    ];

    let half_size = resolution * 0.5;

    let base_local = vec3(
        x as f32 * resolution - half_size,
        y as f32 * resolution - half_size,
        z as f32 * resolution - half_size,
    );
    contour_cell_from_corners(corners, base_local, resolution, planet_position)
}

fn contour_cell_from_corners(
    corners: [f32; 8],
    base: Vec3,
    resolution: f32,
    planet_position: Vec3,
) -> Option<PlanetVertex> {
    let has_negative = corners.iter().any(|&density| density < 0.0);
    let has_positive = corners.iter().any(|&density| density > 0.0);
    if !(has_negative && has_positive) {
        return None;
    }

    let positions = [
        base,
        base + vec3(resolution, 0.0, 0.0),
        base + vec3(0.0, resolution, 0.0),
        base + vec3(resolution, resolution, 0.0),
        base + vec3(0.0, 0.0, resolution),
        base + vec3(resolution, 0.0, resolution),
        base + vec3(0.0, resolution, resolution),
        base + vec3(resolution, resolution, resolution),
    ];

    let mut intersection_sum = vec3(0.0, 0.0, 0.0);
    let mut intersection_count = 0;
    for (first, second) in CELL_EDGES {
        let first_density = corners[first];
        let second_density = corners[second];
        let denominator = first_density - second_density;
        if denominator.abs() < 1e-6 || first_density * second_density >= 0.0 {
            continue;
        }

        let first_position = positions[first];
        let second_position = positions[second];
        let t = (first_density / denominator).clamp(0.0, 1.0);
        intersection_sum += first_position + (second_position - first_position) * t;
        intersection_count += 1;
    }

    if intersection_count == 0 {
        return None;
    }

    let average = intersection_sum / intersection_count as f32;

    if !average.x.is_finite() || !average.y.is_finite() || !average.z.is_finite() {
        return None;
    }

    let average_position = Vec3::from(average);
    let local = ((average_position - base) / resolution).clamp(Vec3::ZERO, Vec3::ONE);
    let tx = local.x;
    let ty = local.y;
    let tz = local.z;

    // Different chunks can represent the same boundary cell as either an
    // owned cell or a ghost cell. Deriving the gradient solely from this
    // cell's eight corners makes both copies produce an identical normal.
    let dx0 = (corners[1] - corners[0]) * (1.0 - ty) + (corners[3] - corners[2]) * ty;
    let dx1 = (corners[5] - corners[4]) * (1.0 - ty) + (corners[7] - corners[6]) * ty;
    let dx = (dx0 * (1.0 - tz) + dx1 * tz) / resolution;

    let dy0 = (corners[2] - corners[0]) * (1.0 - tx) + (corners[3] - corners[1]) * tx;
    let dy1 = (corners[6] - corners[4]) * (1.0 - tx) + (corners[7] - corners[5]) * tx;
    let dy = (dy0 * (1.0 - tz) + dy1 * tz) / resolution;

    let dz0 = (corners[4] - corners[0]) * (1.0 - tx) + (corners[5] - corners[1]) * tx;
    let dz1 = (corners[6] - corners[2]) * (1.0 - tx) + (corners[7] - corners[3]) * tx;
    let dz = (dz0 * (1.0 - ty) + dz1 * ty) / resolution;
    let gradient = vec3(dx, dy, dz);
    let normal = if gradient.length_squared() > 1e-12 {
        gradient.normalize()
    } else {
        (average_position - planet_position).normalize()
    };
    let average_normal = [normal.x, normal.y, normal.z];
    let radial = average_position - planet_position;
    let up = if radial.length_squared() > 1e-6 {
        radial.normalize()
    } else {
        vec3(0.0, 1.0, 0.0)
    };
    let slope = average_normal[0] * up.x + average_normal[1] * up.y + average_normal[2] * up.z;
    //let is_ocean = heightmap
    //    .and_then(|heightmap| heightmap.sample_unit_height(up))
    //    .is_some_and(|height| height == 0.0);
    let is_ocean = false;

    // Define material
    let (mat_a, mat_b, blend) = if is_ocean {
        (3u16, 3u16, 0u8)
    } else if slope > 0.7 {
        (0u16, 1u16, 0u8)
    } else if slope > 0.4 {
        let blend = ((0.7 - slope) / 0.3 * 255.0) as u8;
        (0u16, 1u16, blend)
    } else {
        (1u16, 1u16, 0u8)
    };

    Some(PlanetVertex {
        position: [average_position.x, average_position.y, average_position.z],
        normal: average_normal,
        mat_a,
        mat_b,
        blend,
        _pad: [0, 0, 0],
    })
}

#[inline(never)]
fn append_x_edge_indices(
    grid: &[Vec<Vec<f32>>],
    cell_vertex: &CellVertexGrid,
    indices: &mut Vec<u32>,
    face_neighbors: &[FaceNeighbor; 6],
) {
    let size_x = grid.len();
    let size_y = grid[0].len();
    let size_z = grid[0][0].len();

    // There is one positive-side ghost cell. It supplies boundary vertices but
    // does not own edge segments along the edge's direction.
    for x in 0..(size_x - 2) {
        for y in 1..(size_y - 1) {
            for z in 1..(size_z - 1) {
                if (y == size_y - 2 && face_neighbors[3].kind != FaceNeighborKind::SameOrAbsent)
                    || (z == size_z - 2 && face_neighbors[5].kind != FaceNeighborKind::SameOrAbsent)
                {
                    continue;
                }
                if grid[x][y][z] * grid[x + 1][y][z] >= 0.0 {
                    continue;
                }

                let vertices = [
                    cell_vertex[x][y][z],
                    cell_vertex[x][y - 1][z],
                    cell_vertex[x][y][z - 1],
                    cell_vertex[x][y - 1][z - 1],
                ];
                if let [Some(v0), Some(v1), Some(v2), Some(v3)] = vertices {
                    append_quad(indices, [v0, v1, v2, v3], grid[x][y][z] > 0.0);
                }
            }
        }
    }
}

#[inline(never)]
fn append_y_edge_indices(
    grid: &[Vec<Vec<f32>>],
    cell_vertex: &CellVertexGrid,
    indices: &mut Vec<u32>,
    face_neighbors: &[FaceNeighbor; 6],
) {
    let size_x = grid.len();
    let size_y = grid[0].len();
    let size_z = grid[0][0].len();

    for x in 1..(size_x - 1) {
        for y in 0..(size_y - 2) {
            for z in 1..(size_z - 1) {
                if (x == size_x - 2 && face_neighbors[1].kind != FaceNeighborKind::SameOrAbsent)
                    || (z == size_z - 2 && face_neighbors[5].kind != FaceNeighborKind::SameOrAbsent)
                {
                    continue;
                }
                if grid[x][y][z] * grid[x][y + 1][z] >= 0.0 {
                    continue;
                }

                let vertices = [
                    cell_vertex[x][y][z],
                    cell_vertex[x - 1][y][z],
                    cell_vertex[x][y][z - 1],
                    cell_vertex[x - 1][y][z - 1],
                ];
                if let [Some(v0), Some(v1), Some(v2), Some(v3)] = vertices {
                    append_quad(indices, [v0, v1, v2, v3], grid[x][y][z] < 0.0);
                }
            }
        }
    }
}

#[inline(never)]
fn append_z_edge_indices(
    grid: &[Vec<Vec<f32>>],
    cell_vertex: &CellVertexGrid,
    indices: &mut Vec<u32>,
    face_neighbors: &[FaceNeighbor; 6],
) {
    let size_x = grid.len();
    let size_y = grid[0].len();
    let size_z = grid[0][0].len();

    for x in 1..(size_x - 1) {
        for y in 1..(size_y - 1) {
            for z in 0..(size_z - 2) {
                if (x == size_x - 2 && face_neighbors[1].kind != FaceNeighborKind::SameOrAbsent)
                    || (y == size_y - 2 && face_neighbors[3].kind != FaceNeighborKind::SameOrAbsent)
                {
                    continue;
                }
                if grid[x][y][z] * grid[x][y][z + 1] >= 0.0 {
                    continue;
                }

                let vertices = [
                    cell_vertex[x][y][z],
                    cell_vertex[x - 1][y][z],
                    cell_vertex[x][y - 1][z],
                    cell_vertex[x - 1][y - 1][z],
                ];
                if let [Some(v0), Some(v1), Some(v2), Some(v3)] = vertices {
                    append_quad(indices, [v0, v1, v2, v3], grid[x][y][z] > 0.0);
                }
            }
        }
    }
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct TransitionCellKey {
    min: [u32; 3],
    spacing: u32,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct TransitionEdgeKey {
    start: [u32; 3],
    axis: u8,
}

#[inline]
fn axis_value(value: Vec3, axis: usize) -> f32 {
    match axis {
        0 => value.x,
        1 => value.y,
        _ => value.z,
    }
}

#[inline]
fn with_axis(mut value: Vec3, axis: usize, component: f32) -> Vec3 {
    match axis {
        0 => value.x = component,
        1 => value.y = component,
        _ => value.z = component,
    }
    value
}

fn transition_cell_vertex(
    probe: Vec3,
    face: usize,
    fine_min: Vec3,
    fine_size: f32,
    fine_spacing: f32,
    coarse: FaceNeighbor,
    cache: &mut HashMap<TransitionCellKey, Option<u32>>,
    vertices: &mut Vec<PlanetVertex>,
    terrain: &PlanetTerrainSamplerContext<'_>,
) -> Option<u32> {
    let normal_axis = face / 2;
    let positive_face = face % 2 == 1;
    let plane = axis_value(fine_min, normal_axis) + if positive_face { fine_size } else { 0.0 };
    let on_fine_side = if positive_face {
        axis_value(probe, normal_axis) < plane
    } else {
        axis_value(probe, normal_axis) > plane
    };
    let (origin, spacing) = if on_fine_side {
        (fine_min, fine_spacing)
    } else {
        (coarse.min, coarse.size / CHUNK_CELL_COUNT as f32)
    };

    let aligned = vec3(
        origin.x + ((probe.x - origin.x) / spacing).floor() * spacing,
        origin.y + ((probe.y - origin.y) / spacing).floor() * spacing,
        origin.z + ((probe.z - origin.z) / spacing).floor() * spacing,
    );
    let key = TransitionCellKey {
        min: [
            aligned.x.to_bits(),
            aligned.y.to_bits(),
            aligned.z.to_bits(),
        ],
        spacing: spacing.to_bits(),
    };
    if let Some(index) = cache.get(&key) {
        return *index;
    }

    let corners = [
        aligned,
        aligned + vec3(spacing, 0.0, 0.0),
        aligned + vec3(0.0, spacing, 0.0),
        aligned + vec3(spacing, spacing, 0.0),
        aligned + vec3(0.0, 0.0, spacing),
        aligned + vec3(spacing, 0.0, spacing),
        aligned + vec3(0.0, spacing, spacing),
        aligned + vec3(spacing, spacing, spacing),
    ]
    .map(|position| terrain_sampler::sample_final_density(terrain, position));
    let index = contour_cell_from_corners(corners, aligned, spacing, terrain.planet_position).map(
        |vertex| {
            let index = vertices.len() as u32;
            vertices.push(vertex);
            index
        },
    );
    cache.insert(key, index);
    index
}

#[allow(clippy::too_many_arguments)]
fn append_transition_edge(
    start: Vec3,
    edge_axis: usize,
    density_at_start: f32,
    face: usize,
    fine_min: Vec3,
    fine_size: f32,
    fine_spacing: f32,
    coarse: FaceNeighbor,
    cache: &mut HashMap<TransitionCellKey, Option<u32>>,
    emitted_edges: &mut HashSet<TransitionEdgeKey>,
    vertices: &mut Vec<PlanetVertex>,
    indices: &mut Vec<u32>,
    terrain: &PlanetTerrainSamplerContext<'_>,
) {
    let edge_key = TransitionEdgeKey {
        start: [start.x.to_bits(), start.y.to_bits(), start.z.to_bits()],
        axis: edge_axis as u8,
    };
    if !emitted_edges.insert(edge_key) {
        return;
    }

    let (first_perpendicular, second_perpendicular) = match edge_axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let midpoint = with_axis(
        start,
        edge_axis,
        axis_value(start, edge_axis) + fine_spacing * 0.5,
    );
    let epsilon = fine_spacing * 0.25;
    let quadrants = [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)];
    let mut quadrant_vertices = [None; 4];
    for (slot, (first_sign, second_sign)) in quadrants.into_iter().enumerate() {
        let probe = with_axis(
            with_axis(
                midpoint,
                first_perpendicular,
                axis_value(midpoint, first_perpendicular) + first_sign * epsilon,
            ),
            second_perpendicular,
            axis_value(midpoint, second_perpendicular) + second_sign * epsilon,
        );
        quadrant_vertices[slot] = transition_cell_vertex(
            probe,
            face,
            fine_min,
            fine_size,
            fine_spacing,
            coarse,
            cache,
            vertices,
            terrain,
        );
    }

    let Some(quadrant_vertices) = quadrant_vertices.into_iter().collect::<Option<Vec<u32>>>()
    else {
        return;
    };
    let mut ring = Vec::with_capacity(4);
    for slot in [0, 1, 3, 2] {
        let vertex = quadrant_vertices[slot];
        if ring.last().copied() != Some(vertex) {
            ring.push(vertex);
        }
    }
    if ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
    }
    if ring.len() < 3 {
        return;
    }

    let flip = match edge_axis {
        0 | 2 => density_at_start > 0.0,
        _ => density_at_start < 0.0,
    };
    if flip {
        ring.reverse();
    }
    for triangle in 1..(ring.len() - 1) {
        indices.extend_from_slice(&[ring[0], ring[triangle], ring[triangle + 1]]);
    }
}

#[allow(clippy::too_many_arguments)]
fn append_transition_faces(
    grid: &[Vec<Vec<f32>>],
    fine_min: Vec3,
    fine_size: f32,
    fine_spacing: f32,
    face_neighbors: &[FaceNeighbor; 6],
    vertices: &mut Vec<PlanetVertex>,
    indices: &mut Vec<u32>,
    terrain: &PlanetTerrainSamplerContext<'_>,
) {
    let cell_count = CHUNK_CELL_COUNT;
    let mut cache = HashMap::new();
    let mut emitted_edges = HashSet::new();

    for face in 0..6 {
        let coarse = face_neighbors[face];
        if coarse.kind != FaceNeighborKind::Coarser {
            continue;
        }
        let normal_axis = face / 2;
        let (first_tangent, second_tangent) = match normal_axis {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        let face_sample = if face % 2 == 1 { cell_count } else { 0 };

        for edge_axis in [first_tangent, second_tangent] {
            let fixed_tangent = if edge_axis == first_tangent {
                second_tangent
            } else {
                first_tangent
            };
            for segment in 0..cell_count {
                for fixed_sample in 1..=cell_count {
                    let mut sample = [0usize; 3];
                    sample[normal_axis] = face_sample;
                    sample[edge_axis] = segment;
                    sample[fixed_tangent] = fixed_sample;
                    let mut end_sample = sample;
                    end_sample[edge_axis] += 1;
                    let density_at_start = grid[sample[0]][sample[1]][sample[2]];
                    let density_at_end = grid[end_sample[0]][end_sample[1]][end_sample[2]];
                    if density_at_start * density_at_end >= 0.0 {
                        continue;
                    }

                    let start = fine_min
                        + vec3(
                            sample[0] as f32 * fine_spacing,
                            sample[1] as f32 * fine_spacing,
                            sample[2] as f32 * fine_spacing,
                        );
                    append_transition_edge(
                        start,
                        edge_axis,
                        density_at_start,
                        face,
                        fine_min,
                        fine_size,
                        fine_spacing,
                        coarse,
                        &mut cache,
                        &mut emitted_edges,
                        vertices,
                        indices,
                        terrain,
                    );
                }
            }
        }
    }
}

impl PlanetExt for Planet {
    fn dual_contour_grid(
        grid: &[Vec<Vec<f32>>],
        offset: Vec3,
        resolution: f32,
        terrain: &PlanetTerrainSamplerContext<'_>,
        face_neighbors: &[FaceNeighbor; 6],
    ) -> (Vec<PlanetVertex>, Vec<u32>) {
        let mut vertices: Vec<PlanetVertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        let size_x = grid.len();
        let size_y = grid[0].len();
        let size_z = grid[0][0].len();

        let mut cell_vertex = vec![vec![vec![None; size_z - 1]; size_y - 1]; size_x - 1];

        for x in 0..(size_x - 1) {
            for y in 0..(size_y - 1) {
                for z in 0..(size_z - 1) {
                    let Some(vertex) = contour_cell_vertex(
                        grid,
                        x,
                        y,
                        z,
                        offset,
                        resolution,
                        terrain.planet_position,
                    ) else {
                        continue;
                    };
                    let index = vertices.len() as u32;
                    vertices.push(vertex);
                    cell_vertex[x][y][z] = Some(index);
                }
            }
        }

        append_x_edge_indices(grid, &cell_vertex, &mut indices, face_neighbors);
        append_y_edge_indices(grid, &cell_vertex, &mut indices, face_neighbors);
        append_z_edge_indices(grid, &cell_vertex, &mut indices, face_neighbors);
        append_transition_faces(
            grid,
            offset,
            resolution * CHUNK_CELL_COUNT as f32,
            resolution,
            face_neighbors,
            &mut vertices,
            &mut indices,
            terrain,
        );

        (vertices, indices)
    }

    fn create_octree(
        planet_position: engine::math::Vec3,
        camera_position: &engine::math::Vec3,
        terrain_config: &PlanetTerrainConfig,
        chunk_size: u32,
        lod_strength: f32,
        terrain_edits: &PlanetTerrainEdits,
    ) -> OctreeNode {
        let (_, max_height) = terrain_height_bounds(terrain_config.radius, None);
        let required_diameter = (terrain_config.radius + max_height) * 2.0;
        let root_size = (required_diameter.ceil() as u32).next_power_of_two() as f32;
        let half_root_size = root_size * 0.5;
        octree::build_node(
            Vec3 {
                x: planet_position.x - half_root_size,
                y: planet_position.y - half_root_size,
                z: planet_position.z - half_root_size,
            },
            root_size,
            chunk_size as f32,
            true,
            camera_position,
            planet_position,
            terrain_config,
            lod_strength,
            terrain_edits,
        )
    }

    fn collect_leaf_nodes(node: &OctreeNode, current_depth: u32, out: &mut Vec<(Vec3, f32, u32)>) {
        if node.children.iter().count() == 0 {
            let half = node.size / 2.0;
            let center = Vec3::new(node.min.x + half, node.min.y + half, node.min.z + half);
            if node.has_surface {
                out.push((center, node.size, current_depth));
            }
        } else {
            if let Some(children) = &node.children {
                for child in children.iter() {
                    Planet::collect_leaf_nodes(child, current_depth + 1, out);
                }
            }
        }
    }
}
