use cgmath::{EuclideanSpace, InnerSpace, Point3, Vector3, vec3};
use game_types::{
    octree::OctreeNode,
    planet::{Planet, PlanetTerrainEdits, PlanetVertex},
};

use crate::{octree, sdf::EarthHeightmap};

pub trait PlanetExt {
    fn dual_contour_grid(
        grid: &Vec<Vec<Vec<f32>>>,
        offset: Point3<f32>,
        resolution: f32,
        planet_position: Vector3<f32>,
        planet_size: u32,
        heightmap: Option<&EarthHeightmap>,
        terrain_edits: &PlanetTerrainEdits,
    ) -> (Vec<PlanetVertex>, Vec<u32>);
    fn create_octree(
        planet_position: cgmath::Vector3<f32>,
        planet_radius: u32,
        camera_position: &cgmath::Point3<f32>,
        planet_size: u32,
        chunk_size: u32,
        terrain_edits: &PlanetTerrainEdits,
    ) -> OctreeNode;
    fn collect_leaf_nodes(
        node: &OctreeNode,
        current_depth: u32,
        out: &mut Vec<(Point3<f32>, f32, u32)>,
    );
}

impl PlanetExt for Planet {
    fn dual_contour_grid(
        grid: &Vec<Vec<Vec<f32>>>,
        offset: Point3<f32>,
        resolution: f32,
        planet_position: Vector3<f32>,
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

                    let has_neg = corners.iter().any(|&d| d < 0.0);
                    let has_pos = corners.iter().any(|&d| d > 0.0);

                    if !(has_neg && has_pos) {
                        continue;
                    }

                    let bx = x as f32 * resolution + offset.x;
                    let by = y as f32 * resolution + offset.y;
                    let bz = z as f32 * resolution + offset.z;
                    let positions = [
                        vec3(bx, by, bz),
                        vec3(bx + resolution, by, bz),
                        vec3(bx, by + resolution, bz),
                        vec3(bx + resolution, by + resolution, bz),
                        vec3(bx, by, bz + resolution),
                        vec3(bx + resolution, by, bz + resolution),
                        vec3(bx, by + resolution, bz + resolution),
                        vec3(bx + resolution, by + resolution, bz + resolution),
                    ];

                    let edges = [
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

                    let mut intersections = Vec::new();
                    for (i1, i2) in edges {
                        let d1 = corners[i1];
                        let d2 = corners[i2];
                        let denom = d1 - d2;
                        if denom.abs() < 1e-6 {
                            continue;
                        }
                        if d1 * d2 < 0.0 {
                            let p1 = positions[i1];
                            let p2 = positions[i2];
                            let t = (d1 / denom).clamp(0.0, 1.0);
                            let p = p1 + (p2 - p1) * t;
                            intersections.push(p);
                        }
                    }

                    if intersections.is_empty() {
                        continue;
                    }

                    let mut avg = vec3(0.0, 0.0, 0.0);
                    for p in &intersections {
                        avg += *p;
                    }
                    avg /= intersections.len() as f32;

                    if !avg.x.is_finite() || !avg.y.is_finite() || !avg.z.is_finite() {
                        continue;
                    }

                    let index = vertices.len() as u32;
                    let avg_pos: Point3<f32> = Point3::from_vec(avg);

                    let dx = grid[(x + 1).min(size_x - 1)][y][z] - grid[x.saturating_sub(1)][y][z];
                    let dy = grid[x][(y + 1).min(size_y - 1)][z] - grid[x][y.saturating_sub(1)][z];
                    let dz = grid[x][y][(z + 1).min(size_z - 1)] - grid[x][y][z.saturating_sub(1)];
                    let n = vec3(dx, dy, dz);
                    let n = if n.magnitude2() > 1e-12 {
                        n.normalize()
                    } else {
                        (avg_pos.to_vec() - planet_position).normalize()
                    };
                    let avg_norm = [n.x, n.y, n.z];
                    let avg_vec = avg_pos.to_vec() - planet_position;
                    let up = if avg_vec.magnitude2() > 1e-6 {
                        avg_vec.normalize()
                    } else {
                        vec3(0.0, 1.0, 0.0)
                    };
                    let slope = avg_norm[0] * up.x + avg_norm[1] * up.y + avg_norm[2] * up.z;
                    let is_ocean = heightmap
                        .and_then(|heightmap| heightmap.sample_unit_height(up))
                        .is_some_and(|height| height == 0.0);

                    let (mat_a, mat_b, blend) = if is_ocean {
                        (3u16, 3u16, 0u8)
                    } else if slope > 0.7 {
                        (0u16, 1u16, 0u8)
                    } else if slope > 0.4 {
                        let t = (0.7 - slope) / 0.3;
                        (0u16, 1u16, (t * 255.0) as u8)
                    } else {
                        (1u16, 1u16, 0u8)
                    };

                    vertices.push(PlanetVertex {
                        position: [avg_pos.x, avg_pos.y, avg_pos.z],
                        normal: avg_norm,
                        mat_a,
                        mat_b,
                        blend,
                        _pad: [0, 0, 0],
                    });
                    cell_vertex[x][y][z] = Some(index);
                }
            }
        }

        // X edges
        for x in 0..(size_x - 1) {
            for y in 1..(size_y - 1) {
                for z in 1..(size_z - 1) {
                    let d1 = grid[x][y][z];
                    let d2 = grid[x + 1][y][z];
                    if d1 * d2 >= 0.0 {
                        continue;
                    }
                    let v0 = cell_vertex[x][y][z];
                    let v1 = cell_vertex[x][y - 1][z];
                    let v2 = cell_vertex[x][y][z - 1];
                    let v3 = cell_vertex[x][y - 1][z - 1];
                    if let (Some(v0), Some(v1), Some(v2), Some(v3)) = (v0, v1, v2, v3) {
                        indices.extend_from_slice(&[v0, v1, v2]);
                        indices.extend_from_slice(&[v2, v1, v3]);
                    }
                }
            }
        }

        // Y edges
        for x in 1..(size_x - 1) {
            for y in 0..(size_y - 1) {
                for z in 1..(size_z - 1) {
                    let d1 = grid[x][y][z];
                    let d2 = grid[x][y + 1][z];
                    if d1 * d2 >= 0.0 {
                        continue;
                    }
                    let v0 = cell_vertex[x][y][z];
                    let v1 = cell_vertex[x - 1][y][z];
                    let v2 = cell_vertex[x][y][z - 1];
                    let v3 = cell_vertex[x - 1][y][z - 1];
                    if let (Some(v0), Some(v1), Some(v2), Some(v3)) = (v0, v1, v2, v3) {
                        indices.extend_from_slice(&[v0, v1, v2]);
                        indices.extend_from_slice(&[v2, v1, v3]);
                    }
                }
            }
        }

        // Z edges
        for x in 1..(size_x - 1) {
            for y in 1..(size_y - 1) {
                for z in 0..(size_z - 1) {
                    let d1 = grid[x][y][z];
                    let d2 = grid[x][y][z + 1];
                    if d1 * d2 >= 0.0 {
                        continue;
                    }
                    let v0 = cell_vertex[x][y][z];
                    let v1 = cell_vertex[x - 1][y][z];
                    let v2 = cell_vertex[x][y - 1][z];
                    let v3 = cell_vertex[x - 1][y - 1][z];
                    if let (Some(v0), Some(v1), Some(v2), Some(v3)) = (v0, v1, v2, v3) {
                        indices.extend_from_slice(&[v0, v1, v2]);
                        indices.extend_from_slice(&[v2, v1, v3]);
                    }
                }
            }
        }

        (vertices, indices)
    }

    fn create_octree(
        planet_position: cgmath::Vector3<f32>,
        planet_radius: u32,
        camera_position: &cgmath::Point3<f32>,
        planet_size: u32,
        chunk_size: u32,
        terrain_edits: &PlanetTerrainEdits,
    ) -> OctreeNode {
        let r = planet_radius as f32 / 2.0;
        octree::build_node(
            Vector3 {
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

    fn collect_leaf_nodes(
        node: &OctreeNode,
        current_depth: u32,
        out: &mut Vec<(Point3<f32>, f32, u32)>,
    ) {
        if node.children.iter().count() == 0 {
            let half = node.size / 2.0;
            let center = Point3::new(node.min.x + half, node.min.y + half, node.min.z + half);
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
