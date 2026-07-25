use std::collections::HashMap;

use engine::{
    game_info,
    math::{Vec3, vec3},
};
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

    // There is one positive-side ghost cell. It supplies boundary vertices but
    // does not own edge segments along the edge's direction.
    for x in 0..(size_x - 2) {
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
) {
    let size_x = grid.len();
    let size_y = grid[0].len();
    let size_z = grid[0][0].len();

    for x in 1..(size_x - 1) {
        for y in 0..(size_y - 2) {
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
) {
    let size_x = grid.len();
    let size_y = grid[0].len();
    let size_z = grid[0][0].len();

    for x in 1..(size_x - 1) {
        for y in 1..(size_y - 1) {
            for z in 0..(size_z - 2) {
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

fn append_boundary_skirts(vertices: &mut Vec<PlanetVertex>, indices: &mut Vec<u32>, depth: f32) {
    if depth <= 0.0 || indices.is_empty() {
        return;
    }

    // Preserve the directed form from the triangle that owns each boundary
    // edge. Interior edges occur twice and are discarded.
    let mut edges: HashMap<(u32, u32), (u32, u32, u32)> = HashMap::new();
    for triangle in indices.chunks_exact(3) {
        for (start, end) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            let key = if start < end {
                (start, end)
            } else {
                (end, start)
            };
            edges
                .entry(key)
                .and_modify(|edge| edge.0 += 1)
                .or_insert((1, start, end));
        }
    }

    let mut extruded_vertices = HashMap::<u32, u32>::new();
    for (_, (count, start, end)) in edges {
        if count != 1 {
            continue;
        }

        let mut extrude = |index: u32, vertices: &mut Vec<PlanetVertex>| {
            *extruded_vertices.entry(index).or_insert_with(|| {
                let mut vertex = vertices[index as usize];
                vertex.position[0] -= vertex.normal[0] * depth;
                vertex.position[1] -= vertex.normal[1] * depth;
                vertex.position[2] -= vertex.normal[2] * depth;
                let extruded_index = vertices.len() as u32;
                vertices.push(vertex);
                extruded_index
            })
        };

        let start_extruded = extrude(start, vertices);
        let end_extruded = extrude(end, vertices);

        // The boundary edge follows the source triangle's winding. These two
        // triangles face away from the solid when extrusion follows -normal.
        indices.extend_from_slice(&[
            start,
            end_extruded,
            end,
            start,
            start_extruded,
            end_extruded,
        ]);
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
        append_boundary_skirts(&mut vertices, &mut indices, resolution * 2.0);

        (vertices, indices)
    }

    fn create_octree(
        planet_position: engine::math::Vec3,
        planet_radius: u32,
        camera_position: &engine::math::Vec3,
        planet_size: u32,
        chunk_size: u32,
        lod_strength: f32,
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
            lod_strength,
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use game_types::planet::PlanetTerrainEdits;

    use super::*;

    fn empty_edits() -> PlanetTerrainEdits {
        PlanetTerrainEdits {
            modified_chunks: HashMap::new(),
            modified_ranges: HashMap::new(),
        }
    }

    fn density_grid(
        sample_count: usize,
        offset: Vec3,
        density: impl Fn(Vec3) -> f32,
    ) -> Vec<Vec<Vec<f32>>> {
        (0..sample_count)
            .map(|x| {
                (0..sample_count)
                    .map(|y| {
                        (0..sample_count)
                            .map(|z| density(offset + vec3(x as f32, y as f32, z as f32)))
                            .collect()
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn positive_ghost_cell_does_not_emit_owned_edge_segments() {
        // Four samples represent two owned cells plus one positive ghost cell.
        // This surface exists only in the ghost cell at x = 2.
        let grid = density_grid(4, Vec3::ZERO, |p| p.x - 2.5);
        let (_, indices) =
            Planet::dual_contour_grid(&grid, Vec3::ZERO, 1.0, Vec3::ZERO, 0, None, &empty_edits());

        assert!(indices.is_empty());
    }

    #[test]
    fn owned_cell_still_emits_geometry() {
        let grid = density_grid(4, Vec3::ZERO, |p| p.x - 1.5);
        let (_, indices) =
            Planet::dual_contour_grid(&grid, Vec3::ZERO, 1.0, Vec3::ZERO, 0, None, &empty_edits());

        assert!(!indices.is_empty());
    }

    #[test]
    fn owned_and_ghost_copies_of_a_cell_have_matching_vertices_and_normals() {
        let density = |p: Vec3| p.x + 2.0 * p.y - 5.0;
        let lower_grid = density_grid(4, Vec3::ZERO, density);
        let neighbor_offset = vec3(2.0, 0.0, 0.0);
        let neighbor_grid = density_grid(4, neighbor_offset, density);

        let lower =
            contour_cell_vertex(&lower_grid, 2, 1, 1, Vec3::ZERO, 1.0, Vec3::ZERO, None).unwrap();
        let neighbor = contour_cell_vertex(
            &neighbor_grid,
            0,
            1,
            1,
            neighbor_offset,
            1.0,
            Vec3::ZERO,
            None,
        )
        .unwrap();

        for axis in 0..3 {
            assert!((lower.position[axis] - neighbor.position[axis]).abs() < 1e-6);
            assert!((lower.normal[axis] - neighbor.normal[axis]).abs() < 1e-6);
        }
    }

    #[test]
    fn boundary_skirts_extrude_open_edges_into_solid() {
        let vertex = |position: [f32; 3]| PlanetVertex {
            position,
            normal: [0.0, 0.0, 1.0],
            mat_a: 0,
            mat_b: 0,
            blend: 0,
            _pad: [0; 3],
        };
        let mut vertices = vec![
            vertex([0.0, 0.0, 0.0]),
            vertex([1.0, 0.0, 0.0]),
            vertex([0.0, 1.0, 0.0]),
        ];
        let mut indices = vec![0, 1, 2];

        append_boundary_skirts(&mut vertices, &mut indices, 2.0);

        assert_eq!(vertices.len(), 6);
        assert_eq!(indices.len(), 21);
        assert!(
            vertices[3..]
                .iter()
                .all(|vertex| (vertex.position[2] + 2.0).abs() < 1e-6)
        );
    }
}
