use cgmath::{self, EuclideanSpace, vec3};
use cgmath::{InnerSpace, Point3};
use engine::assets;
use engine::assets::material::Material;
use engine::engine_info;
use engine::renderer::GeometryPassNode;
use engine::renderer::PipelineHandle;
use engine::renderer::RenderNode;
use engine::{KeyCode, model::MeshAsset};
use game_types::planet::Planet;
use game_types::planet::PlanetMesh;
pub use game_types::render_graph;
use std::cmp;

#[unsafe(no_mangle)]
pub fn register_systems(state: &mut engine::State) {}

#[unsafe(no_mangle)]
pub fn render() {
    // libloading: load game_logic.dll, find "render", call it
}

#[unsafe(no_mangle)]
pub fn update(state: &mut engine::State) {
    for transform in &mut state.scene.transform_components {
        transform.scale = (0.01, 0.01, 0.01).into(); //(transform.velocity.x * 0.1);
        transform.position -= transform.velocity;
    }
}

#[unsafe(no_mangle)]
pub fn handle_key_press(state: &mut engine::State, key_code: KeyCode, pressed: bool) {
    if key_code == KeyCode::KeyU && pressed {
        for i in 0..cmp::min(state.scene.transform_components.len(), 3) {
            state.scene.transform_components[i].position.y += 0.1;
        }
    }

    if key_code == KeyCode::KeyT && pressed {}

    if key_code == KeyCode::KeyO && pressed {
        let mut planet = Planet::generate_planet();
        let mut material = Material::default();

        let camera_layout = state
            .renderer
            .render_graph
            .get_node_mut::<GeometryPassNode>(0)
            .and_then(|node| node.camera_bind_group_layout)
            .expect("GeometryPassNode must be compiled before creating pipelines");

        state
            .renderer
            .renderer_api
            .create_pipeline(&material, &[camera_layout]);

        planet.load_mesh();
        if planet.mesh.positions.len() > 0 {
            let render_data = state.renderer.renderer_api.create_render_data(
                planet.mesh.positions,
                planet.mesh.indices,
                material,
                &PipelineHandle(0),
            );

            if let Some(node) = state
                .renderer
                .render_graph
                .nodes
                .first_mut()
                .unwrap()
                .1
                .as_any_mut()
                .downcast_mut::<GeometryPassNode>()
            {
                node.add_render_data(render_data);
            }
        }
    }
}

trait PlanetExt {
    fn generate_planet() -> Self;
    fn load_mesh(&mut self);
    fn generate_grid(x: u32, y: u32, z: u32) -> Vec<Vec<Vec<f32>>>;
    fn sdf(p: cgmath::Vector3<f32>) -> f32;
    fn dual_contour_grid(grid: &Vec<Vec<Vec<f32>>>) -> (Vec<Point3<f32>>, Vec<u32>);
}

impl PlanetExt for Planet {
    fn generate_planet() -> Self {
        Planet {
            id: 0,
            name: String::new(),
            mesh: PlanetMesh::new(),
        }
    }

    fn load_mesh(&mut self) {
        let grid = Planet::generate_grid(256, 256, 256);
        let (positions, indices) = Planet::dual_contour_grid(&grid);
        self.mesh.positions = positions;
        self.mesh.indices = indices;
        //self.mesh.positions = vec![
        //    cgmath::Point3::new(-0.5, -0.5, -0.5),
        //    cgmath::Point3::new(0.5, -0.5, -0.5),
        //    cgmath::Point3::new(0.5, 0.5, -0.5),
        //    cgmath::Point3::new(-0.5, 0.5, -0.5),
        //    cgmath::Point3::new(-0.5, -0.5, 0.5),
        //    cgmath::Point3::new(0.5, -0.5, 0.5),
        //    cgmath::Point3::new(0.5, 0.5, 0.5),
        //    cgmath::Point3::new(-0.5, 0.5, 0.5),
        //];
        //
        //let indices: Vec<u32> = vec![
        //    4, 5, 6, 4, 6, 7, // front  (+z)
        //    1, 0, 3, 1, 3, 2, // back   (-z)
        //    5, 1, 2, 5, 2, 6, // right  (+x)
        //    0, 4, 7, 0, 7, 3, // left   (-x)
        //    3, 7, 6, 3, 6, 2, // top    (+y)
        //    0, 1, 5, 0, 5, 4, // bottom (-y)
        //];
        //self.mesh.indices = indices;
    }

    fn generate_grid(x: u32, y: u32, z: u32) -> Vec<Vec<Vec<f32>>> {
        let mut grid: Vec<Vec<Vec<f32>>> = Vec::new();
        for xi in 0..x {
            let mut plane = Vec::new();

            for yi in 0..y {
                let mut row = Vec::new();

                for zi in 0..z {
                    let p = cgmath::Vector3::new(xi as f32, yi as f32, zi as f32);
                    let sdf = Planet::sdf(p);
                    row.push(sdf);
                }

                plane.push(row);
            }

            grid.push(plane);
        }

        grid
    }

    fn sdf(p: cgmath::Vector3<f32>) -> f32 {
        p.magnitude() - 10.0
    }

    fn dual_contour_grid(grid: &Vec<Vec<Vec<f32>>>) -> (Vec<Point3<f32>>, Vec<u32>) {
        let mut vertices: Vec<Point3<f32>> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        let size_x = grid.len();
        let size_y = grid[0].len();
        let size_z = grid[0][0].len();

        // Store vertex index per cell
        let mut cell_vertex = vec![vec![vec![None; size_z - 1]; size_y - 1]; size_x - 1];

        // -----------------------------------------
        // PASS 1: Generate vertices per cell
        // -----------------------------------------
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

                    let center = vec3(
                        size_x as f32 / 2.0,
                        size_y as f32 / 2.0,
                        size_z as f32 / 2.0,
                    );

                    let positions = [
                        vec3(x as f32, y as f32, z as f32) - center,
                        vec3(x as f32 + 1.0, y as f32, z as f32) - center,
                        vec3(x as f32, y as f32 + 1.0, z as f32) - center,
                        vec3(x as f32 + 1.0, y as f32 + 1.0, z as f32) - center,
                        vec3(x as f32, y as f32, z as f32 + 1.0) - center,
                        vec3(x as f32 + 1.0, y as f32, z as f32 + 1.0) - center,
                        vec3(x as f32, y as f32 + 1.0, z as f32 + 1.0) - center,
                        vec3(x as f32 + 1.0, y as f32 + 1.0, z as f32 + 1.0) - center,
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
                    vertices.push(Point3::from_vec(avg));
                    cell_vertex[x][y][z] = Some(index);
                }
            }
        }

        // -----------------------------------------
        // PASS 2: Build indices (edges)
        // -----------------------------------------

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
}

trait PlanetMeshExt {
    fn new() -> Self;
}

impl PlanetMeshExt for PlanetMesh {
    fn new() -> Self {
        PlanetMesh {
            positions: Vec::new(),
            indices: Vec::new(),
        }
    }
}

//impl RenderNode for PlanetRendererNode {
//    fn new() -> Self {
//        PlanetRendererNode {
//            render_data: Vec::new(),
//        }
//    }
//
//    fn add_render_data(&mut self, render_data: RenderData) {
//        self.render_data.push(render_data);
//    }
//}
