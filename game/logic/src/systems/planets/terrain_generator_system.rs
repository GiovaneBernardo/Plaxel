use std::sync::atomic::Ordering;

use cgmath::{EuclideanSpace, InnerSpace, Point3, Vector3, vec3};
use engine::{
    assets::material::Material,
    renderer::{DebugPassNode, GeometryPassNode, PipelineHandle},
};
use game_types::{
    octree::OctreeNode,
    planet::{Planet, PlanetMesh, PlanetVertex},
};

use crate::{
    octree,
    sdf::{self, EarthHeightmap},
    systems::planets::generate_grid_from_min,
};

pub trait PlanetExt {
    #[allow(dead_code)]
    fn load_meshes(
        &mut self,
        state: &mut engine::State,
        solid_material: &Material,
        line_material: &Material,
        planet_size: u32,
        chunk_size: u32,
    );
    fn dual_contour_grid(
        grid: &Vec<Vec<Vec<f32>>>,
        offset: Point3<f32>,
        resolution: f32,
        planet_position: Vector3<f32>,
        planet_size: u32,
        heightmap: Option<&EarthHeightmap>,
    ) -> (Vec<PlanetVertex>, Vec<u32>);
    fn create_octree(
        planet_position: cgmath::Vector3<f32>,
        planet_radius: u32,
        camera_position: &cgmath::Point3<f32>,
        planet_size: u32,
        chunk_size: u32,
    ) -> OctreeNode;
    fn collect_leaf_nodes(
        node: &OctreeNode,
        current_depth: u32,
        out: &mut Vec<(Point3<f32>, f32, u32)>,
    );
}

impl PlanetExt for Planet {
    fn load_meshes(
        &mut self,
        state: &mut engine::State,
        solid_material: &Material,
        line_material: &Material,
        planet_size: u32,
        chunk_size: u32,
    ) {
        let _debug_pass_node: &mut DebugPassNode = state
            .global_resources
            .renderer
            .render_graph
            .get_node_mut::<DebugPassNode>(2)
            .unwrap();

        let mut octree_nodes = Vec::new();
        Planet::collect_leaf_nodes(&self.octree_root, 0, &mut octree_nodes);
        println!("Leaf nodes: {}", octree_nodes.len());

        let mut meshes_count = 0;

        for (center, node_size, _node_depth) in &octree_nodes {
            let resolution = node_size / chunk_size as f32;
            let min_corner = Point3::new(
                center.x - node_size * 0.5,
                center.y - node_size * 0.5,
                center.z - node_size * 0.5,
            );
            let (positions, indices) = Planet::dual_contour_grid(
                &generate_grid_from_min(
                    34,
                    34,
                    34,
                    resolution,
                    min_corner.to_vec(),
                    self.position,
                    planet_size,
                    None,
                ),
                min_corner,
                resolution,
                self.position,
                planet_size,
                None,
            );

            let mesh = PlanetMesh { positions, indices };

            if mesh.positions.len() > 0 {
                meshes_count += 1;
                println!("Added meshes: {}", meshes_count);
                let vertex_bytes: Vec<u8> = bytemuck::cast_slice(&mesh.positions).to_vec();
                let solid_render_data = state
                    .global_resources
                    .renderer
                    .renderer_api
                    .create_render_data(
                        &vertex_bytes,
                        &mesh.indices,
                        solid_material.clone(),
                        &PipelineHandle(0),
                    );

                let _line_render_data = state
                    .global_resources
                    .renderer
                    .renderer_api
                    .create_render_data(
                        &vertex_bytes,
                        &mesh.indices,
                        line_material.clone(),
                        &PipelineHandle(0),
                    );

                if let Some(node) = state
                    .global_resources
                    .renderer
                    .render_graph
                    .nodes
                    .first_mut()
                    .unwrap()
                    .1
                    .as_any_mut()
                    .downcast_mut::<GeometryPassNode>()
                {
                    node.add_render_data(solid_render_data);
                }
            }
        }
        println!("Added meshes: {}", meshes_count);
    }

    fn dual_contour_grid(
        grid: &Vec<Vec<Vec<f32>>>,
        offset: Point3<f32>,
        resolution: f32,
        planet_position: Vector3<f32>,
        planet_size: u32,
        heightmap: Option<&EarthHeightmap>,
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

                    let normal_at = |p: Vector3<f32>| -> [f32; 3] {
                        let eps = resolution * 0.5;
                        let dx = sdf::sdf_at_center(
                            p + vec3(eps, 0.0, 0.0),
                            planet_position,
                            planet_size,
                            heightmap,
                        ) - sdf::sdf_at_center(
                            p - vec3(eps, 0.0, 0.0),
                            planet_position,
                            planet_size,
                            heightmap,
                        );
                        let dy = sdf::sdf_at_center(
                            p + vec3(0.0, eps, 0.0),
                            planet_position,
                            planet_size,
                            heightmap,
                        ) - sdf::sdf_at_center(
                            p - vec3(0.0, eps, 0.0),
                            planet_position,
                            planet_size,
                            heightmap,
                        );
                        let dz = sdf::sdf_at_center(
                            p + vec3(0.0, 0.0, eps),
                            planet_position,
                            planet_size,
                            heightmap,
                        ) - sdf::sdf_at_center(
                            p - vec3(0.0, 0.0, eps),
                            planet_position,
                            planet_size,
                            heightmap,
                        );

                        let n = vec3(dx, dy, dz).normalize();
                        [n.x, n.y, n.z]
                    };

                    let avg_norm = normal_at(Vector3::new(avg_pos.x, avg_pos.y, avg_pos.z));
                    let avg_vec = avg_pos.to_vec() - planet_position;
                    let up = if avg_vec.magnitude2() > 1e-6 {
                        avg_vec.normalize()
                    } else {
                        vec3(0.0, 1.0, 0.0)
                    };
                    let slope = avg_norm[0] * up.x + avg_norm[1] * up.y + avg_norm[2] * up.z;

                    let (mat_a, mat_b, blend) = if slope > 0.7 {
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
