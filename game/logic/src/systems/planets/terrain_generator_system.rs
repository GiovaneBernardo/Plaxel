use engine::math::{Vec3, vec3};
use game_types::{
    octree::OctreeNode,
    planet::{Planet, PlanetTerrainEdits, PlanetVertex},
};

use crate::{octree, sdf::EarthHeightmap};

pub trait PlanetExt {
    fn dual_contour_grid(
        grid: &[Vec<Vec<f32>>],
        offset: Vec3,
        resolution: f32,
        planet_position: Vec3,
        planet_size: u32,
        heightmap: Option<&EarthHeightmap>,
        terrain_edits: &PlanetTerrainEdits,
    ) -> (Vec<PlanetVertex>, Vec<u32>);
    fn create_octree(
        planet_position: engine::math::Vec3,
        planet_radius: u32,
        camera_position: &engine::math::Vec3,
        planet_size: u32,
        chunk_size: u32,
        terrain_edits: &PlanetTerrainEdits,
    ) -> OctreeNode;
    fn collect_leaf_nodes(node: &OctreeNode, current_depth: u32, out: &mut Vec<(Vec3, f32, u32)>);
}

type CellVertexGrid = Vec<Vec<Vec<Option<u32>>>>;

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
    heightmap: Option<&EarthHeightmap>,
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

    let has_negative = corners.iter().any(|&density| density < 0.0);
    let has_positive = corners.iter().any(|&density| density > 0.0);
    if !(has_negative && has_positive) {
        return None;
    }

    let base = vec3(
        x as f32 * resolution + offset.x,
        y as f32 * resolution + offset.y,
        z as f32 * resolution + offset.z,
    );
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

    let size_x = grid.len();
    let size_y = grid[0].len();
    let size_z = grid[0][0].len();
    let average_position = Vec3::from(average);
    let dx = grid[(x + 1).min(size_x - 1)][y][z] - grid[x.saturating_sub(1)][y][z];
    let dy = grid[x][(y + 1).min(size_y - 1)][z] - grid[x][y.saturating_sub(1)][z];
    let dz = grid[x][y][(z + 1).min(size_z - 1)] - grid[x][y][z.saturating_sub(1)];
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
    let is_ocean = heightmap
        .and_then(|heightmap| heightmap.sample_unit_height(up))
        .is_some_and(|height| height == 0.0);

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
) {
    let size_x = grid.len();
    let size_y = grid[0].len();
    let size_z = grid[0][0].len();

    for x in 0..(size_x - 1) {
        for y in 1..(size_y - 1) {
            for z in 1..(size_z - 1) {
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
                    indices.extend_from_slice(&[v0, v1, v2, v2, v1, v3]);
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
) {
    let size_x = grid.len();
    let size_y = grid[0].len();
    let size_z = grid[0][0].len();

    for x in 1..(size_x - 1) {
        for y in 0..(size_y - 1) {
            for z in 1..(size_z - 1) {
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
                    indices.extend_from_slice(&[v0, v1, v2, v2, v1, v3]);
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
) {
    let size_x = grid.len();
    let size_y = grid[0].len();
    let size_z = grid[0][0].len();

    for x in 1..(size_x - 1) {
        for y in 1..(size_y - 1) {
            for z in 0..(size_z - 1) {
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
                    indices.extend_from_slice(&[v0, v1, v2, v2, v1, v3]);
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
        planet_position: Vec3,
        _planet_size: u32,
        heightmap: Option<&EarthHeightmap>,
        _terrain_edits: &PlanetTerrainEdits,
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
                        planet_position,
                        heightmap,
                    ) else {
                        continue;
                    };
                    let index = vertices.len() as u32;
                    vertices.push(vertex);
                    cell_vertex[x][y][z] = Some(index);
                }
            }
        }

        append_x_edge_indices(grid, &cell_vertex, &mut indices);
        append_y_edge_indices(grid, &cell_vertex, &mut indices);
        append_z_edge_indices(grid, &cell_vertex, &mut indices);

        (vertices, indices)
    }

    fn create_octree(
        planet_position: engine::math::Vec3,
        planet_radius: u32,
        camera_position: &engine::math::Vec3,
        planet_size: u32,
        chunk_size: u32,
        terrain_edits: &PlanetTerrainEdits,
    ) -> OctreeNode {
        let r = planet_radius as f32 / 2.0;
        octree::build_node(
            Vec3 {
                x: planet_position.x + -r,
                y: planet_position.y + -r,
                z: planet_position.z + -r,
            },
            planet_radius as f32,
            chunk_size as f32,
            true,
            camera_position,
            planet_position,
            planet_size,
            None,
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
