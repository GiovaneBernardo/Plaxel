use cgmath::{self, Array, EuclideanSpace, Vector3, vec3};
use cgmath::{InnerSpace, Point3};
use engine::assets;
use engine::assets::material::Material;
use engine::engine_info;
use engine::model::{ModelVertex, Vertex};
use engine::renderer::RenderNode;
use engine::renderer::{CullMode, DepthState, PipelineHandle};
use engine::renderer::{DebugPassNode, GeometryPassNode};
use engine::{KeyCode, model::MeshAsset};
use game_types::planet::{Planet, PlanetVertex};
use game_types::planet::{PlanetInstance, PlanetMesh};
pub use game_types::render_graph;
use std::cmp;
use std::sync::atomic::{AtomicU32, Ordering};

const PLANET_SIZE: usize = 256;

static OCTREE_DEBUG_DEPTH: AtomicU32 = AtomicU32::new(0);
static OCTREE_MAX_DEPTH: AtomicU32 = AtomicU32::new(0);

const DEPTH_COLORS: [[f32; 4]; 10] = [
    [1.0, 0.2, 0.2, 1.0], // red
    [0.2, 1.0, 0.2, 1.0], // green
    [0.2, 0.4, 1.0, 1.0], // blue
    [1.0, 1.0, 0.2, 1.0], // yellow
    [1.0, 0.2, 1.0, 1.0], // magenta
    [0.2, 1.0, 1.0, 1.0], // cyan
    [1.0, 0.6, 0.2, 1.0], // orange
    [0.6, 0.2, 1.0, 1.0], // purple
    [0.2, 1.0, 0.6, 1.0], // teal
    [1.0, 0.4, 0.6, 1.0], // pink
];

fn depth_color(depth: u32) -> [f32; 4] {
    DEPTH_COLORS[depth as usize % DEPTH_COLORS.len()]
}

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

    // Cycle octree debug depth: [ = previous level, ] = next level
    // [ = deeper level, ] = shallower level
    if key_code == KeyCode::BracketLeft && pressed {
        let max = OCTREE_MAX_DEPTH.load(Ordering::Relaxed);
        let old = OCTREE_DEBUG_DEPTH.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |d| {
            if d < max { Some(d + 1) } else { None }
        });
        if old.is_ok() {
            rebuild_octree_debug(state);
        }
    }

    if key_code == KeyCode::BracketRight && pressed {
        let old = OCTREE_DEBUG_DEPTH.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |d| {
            if d > 0 { Some(d - 1) } else { None }
        });
        if old.is_ok() {
            rebuild_octree_debug(state);
        }
    }

    if key_code == KeyCode::KeyT && pressed {}

    if key_code == KeyCode::KeyO && pressed {
        let mut planet = Planet::generate_planet();
        let solid_material = Material::new("shaders/planet_terrain.wgsl".to_string())
            .with_vertex_layouts(vec![PlanetVertex::layout(), PlanetInstance::layout()])
            .with_cull(CullMode::None);

        let line_material = Material::new("shaders/planet_terrain2.wgsl".to_string())
            .with_vertex_layouts(vec![PlanetVertex::layout(), PlanetInstance::layout()])
            .with_topology(engine::renderer::Topology::LineList)
            .with_cull(CullMode::None);
        //.with_depth(Some(DepthState {
        //    write_enabled: false,
        //    compare: engine::renderer::CompareFunction::Always,
        //}));

        let camera_layout = state
            .renderer
            .render_graph
            .get_node_mut::<GeometryPassNode>(0)
            .and_then(|node| node.camera_bind_group_layout)
            .expect("GeometryPassNode must be compiled before creating pipelines");

        state
            .renderer
            .renderer_api
            .create_pipeline(&solid_material, &[camera_layout]);

        state
            .renderer
            .renderer_api
            .create_pipeline(&line_material, &[camera_layout]);

        planet.load_mesh(state);
        if planet.mesh.positions.len() > 0 {
            let vertex_bytes: Vec<u8> = bytemuck::cast_slice(&planet.mesh.positions).to_vec();
            let solid_render_data = state.renderer.renderer_api.create_render_data(
                &vertex_bytes,
                &planet.mesh.indices,
                solid_material,
                &PipelineHandle(0),
            );

            let line_render_data = state.renderer.renderer_api.create_render_data(
                &vertex_bytes,
                &planet.mesh.indices,
                line_material,
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
                node.add_render_data(solid_render_data);
                node.add_render_data(line_render_data);
            }
        }
    }
}

trait PlanetExt {
    fn generate_planet() -> Self;
    fn load_mesh(&mut self, state: &mut engine::State);
    fn generate_grid(x: u32, y: u32, z: u32, size: u32) -> Vec<Vec<Vec<f32>>>;
    fn dual_contour_grid(grid: &Vec<Vec<Vec<f32>>>) -> (Vec<PlanetVertex>, Vec<u32>);
    fn create_octree(planet_radius: u32) -> OctreeNode;
    fn collect_leaf_nodes(node: &OctreeNode, out: &mut Vec<&OctreeNode>);
}

impl PlanetExt for Planet {
    fn generate_planet() -> Self {
        Planet {
            id: 0,
            name: String::new(),
            mesh: PlanetMesh::new(),
        }
    }

    fn load_mesh(&mut self, state: &mut engine::State) {
        let size: usize = PLANET_SIZE;
        let grid = Planet::generate_grid(size as u32, size as u32, size as u32, size as u32);
        let debug_pass_node: &mut DebugPassNode = state
            .renderer
            .render_graph
            .get_node_mut::<DebugPassNode>(1)
            .unwrap();

        // Only add debug cubes at surface cells (sign change between neighbors)
        for x in 0..(size - 1) {
            for y in 0..(size - 1) {
                for z in 0..(size - 1) {
                    let d = grid[x][y][z];
                    let sign_change = (d > 0.0) != (grid[x + 1][y][z] > 0.0)
                        || (d > 0.0) != (grid[x][y + 1][z] > 0.0)
                        || (d > 0.0) != (grid[x][y][z + 1] > 0.0);
                    if sign_change {
                        let center = (size as f32) / 2.0;
                        let position = cgmath::Point3::new(
                            x as f32 - center,
                            y as f32 - center,
                            z as f32 - center,
                        );
                        //debug_pass_node.add_cube(position, 1.0, [0.3, 0.3, 0.3, 1.0]);
                    }
                }
            }
        }

        // Visualize octree nodes as debug cubes
        let octree = Planet::create_octree(size as u32 / 2);
        let max_depth = octree_max_depth(&octree, 0);
        OCTREE_MAX_DEPTH.store(max_depth, Ordering::Relaxed);
        OCTREE_DEBUG_DEPTH.store(0, Ordering::Relaxed);

        let mut octree_nodes = Vec::new();
        collect_octree_nodes_at_depth(&octree, 0, 0, &mut octree_nodes);
        let color = depth_color(0);
        for (center, node_size) in &octree_nodes {
            debug_pass_node.add_wire_cube(*center, *node_size, color);
        }

        let (positions, indices) = Planet::dual_contour_grid(&grid);
        self.mesh.positions = positions;
        self.mesh.indices = indices;
    }

    fn generate_grid(x: u32, y: u32, z: u32, size: u32) -> Vec<Vec<Vec<f32>>> {
        let mut grid: Vec<Vec<Vec<f32>>> = Vec::new();
        for xi in 0..x {
            let mut plane = Vec::new();

            for yi in 0..y {
                let mut row = Vec::new();

                for zi in 0..z {
                    let center = size as f32 / 2.0;
                    let position = cgmath::Vector3::new(
                        xi as f32 - center,
                        yi as f32 - center,
                        zi as f32 - center,
                    );
                    let sdf = sdf(position);
                    row.push(sdf);
                }

                plane.push(row);
            }

            grid.push(plane);
        }

        grid
    }

    fn dual_contour_grid(grid: &Vec<Vec<Vec<f32>>>) -> (Vec<PlanetVertex>, Vec<u32>) {
        let mut vertices: Vec<PlanetVertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        let size_x = grid.len();
        let size_y = grid[0].len();
        let size_z = grid[0][0].len();

        // Store vertex index per cell
        let mut cell_vertex = vec![vec![vec![None; size_z - 1]; size_y - 1]; size_x - 1];

        // PASS 1: Generate vertices per cell

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

                    fn f(x: f32, y: f32, z: f32) -> f32 {
                        let r = (x * x + y * y + z * z).sqrt();
                        r - 1.0
                    }

                    fn normal_from_function(vert: cgmath::Vector3<f32>) -> [f32; 3] {
                        let eps = 0.001;
                        let x = vert.x;
                        let y = vert.y;
                        let z = vert.z;
                        let dx = (f(x + eps, y, z) - f(x - eps, y, z)) / (2.0 * eps);
                        let dy = (f(x, y + eps, z) - f(x, y - eps, z)) / (2.0 * eps);
                        let dz = (f(x, y, z + eps) - f(x, y, z - eps)) / (2.0 * eps);
                        let normal = vec3(dx, dy, dz).normalize();
                        [normal.x, normal.y, normal.z]
                    }

                    let normals = [
                        normal_from_function(positions[0]),
                        normal_from_function(positions[1]),
                        normal_from_function(positions[2]),
                        normal_from_function(positions[3]),
                        normal_from_function(positions[4]),
                        normal_from_function(positions[5]),
                        normal_from_function(positions[6]),
                        normal_from_function(positions[7]),
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
                    let avg_norm: [f32; 3] =
                        normal_from_function(Vector3::new(avg_pos.x, avg_pos.y, avg_pos.z));
                    vertices.push(PlanetVertex {
                        position: [avg_pos.x, avg_pos.y, avg_pos.z],
                        tex_coords: [0.0, 0.0],
                        normal: avg_norm,
                    });
                    cell_vertex[x][y][z] = Some(index);
                }
            }
        }

        // PASS 2: Build indices (edges)

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

    fn create_octree(planet_radius: u32) -> OctreeNode {
        let r = planet_radius as f32 / 2.0;
        let root_node = build_node(
            Vector3 {
                x: -r,
                y: -r,
                z: -r,
            },
            planet_radius as f32,
            1.0,
            true,
        );

        root_node
    }

    fn collect_leaf_nodes(node: &OctreeNode, out: &mut Vec<&OctreeNode>) {}
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

pub struct OctreeNode {
    min: Vector3<f32>, // corner
    size: f32,
    children: Option<[Box<OctreeNode>; 8]>,
    vertex: Option<u32>,
}

fn hash3(p: Vector3<f32>) -> f32 {
    // Convert to integers for reliable hashing
    let ix = (p.x.floor() as i32).wrapping_mul(1619);
    let iy = (p.y.floor() as i32).wrapping_mul(31337);
    let iz = (p.z.floor() as i32).wrapping_mul(6271);
    let n = ix.wrapping_add(iy).wrapping_add(iz);
    // Avalanche
    let n = n.wrapping_mul(n.wrapping_mul(n).wrapping_mul(60493).wrapping_add(19990303));
    // Map to [0, 1]
    (n as u32 as f32) / (u32::MAX as f32)
}

fn smooth_noise(p: Vector3<f32>) -> f32 {
    // Rust fract() is negative for negative numbers; floor+fract gives wrong cell
    let ix = p.x.floor();
    let iy = p.y.floor();
    let iz = p.z.floor();
    let fx = p.x - ix; // always in [0, 1)
    let fy = p.y - iy;
    let fz = p.z - iz;

    let u = vec3(
        fx * fx * (3.0 - 2.0 * fx),
        fy * fy * (3.0 - 2.0 * fy),
        fz * fz * (3.0 - 2.0 * fz),
    );

    let i = vec3(ix, iy, iz);
    let v000 = hash3(i + vec3(0.0, 0.0, 0.0));
    let v100 = hash3(i + vec3(1.0, 0.0, 0.0));
    let v010 = hash3(i + vec3(0.0, 1.0, 0.0));
    let v110 = hash3(i + vec3(1.0, 1.0, 0.0));
    let v001 = hash3(i + vec3(0.0, 0.0, 1.0));
    let v101 = hash3(i + vec3(1.0, 0.0, 1.0));
    let v011 = hash3(i + vec3(0.0, 1.0, 1.0));
    let v111 = hash3(i + vec3(1.0, 1.0, 1.0));

    let x00 = v000 + u.x * (v100 - v000);
    let x10 = v010 + u.x * (v110 - v010);
    let x01 = v001 + u.x * (v101 - v001);
    let x11 = v011 + u.x * (v111 - v011);
    let y0 = x00 + u.y * (x10 - x00);
    let y1 = x01 + u.y * (x11 - x01);
    y0 + u.z * (y1 - y0)
}

/// Fractional Brownian Motion — layered octaves of noise
fn fbm(p: Vector3<f32>, octaves: u32) -> f32 {
    let mut value = 0.0f32;
    let mut amplitude = 0.5f32;
    let mut frequency = 1.0f32;
    for _ in 0..octaves {
        value += amplitude * smooth_noise(p * frequency);
        frequency *= 2.0;
        amplitude *= 0.5;
    }
    value
}

// --- Cave SDF ---
// Returns a negative value inside a cave tunnel.
// We carve worm-like tunnels by warping a sine-based distance field.
fn cave_sdf(p: Vector3<f32>) -> f32 {
    let scale = 0.04; // spatial frequency of tunnels
    let q = p * scale;
    // Warp the lookup point so tunnels curl
    let warp = vec3(
        fbm(q + vec3(1.7, 9.2, 3.4), 3),
        fbm(q + vec3(8.3, 2.8, 5.1), 3),
        fbm(q + vec3(4.1, 6.7, 1.9), 3),
    ) * 2.0
        - vec3(1.0, 1.0, 1.0); // remap [0,1] -> [-1,1]

    let warped = q + warp * 0.6;
    let tunnel_r = fbm(warped, 4); // 0..1
    // tunnel_r near 0.5 => inside tunnel; map so ~0.5 = 0.0 boundary
    let cave_dist = (tunnel_r - 0.5).abs() - 0.08; // tube half-thickness
    cave_dist * (1.0 / scale) // scale back to world units
}

pub fn sdf(p: cgmath::Vector3<f32>) -> f32 {
    let planet_r = PLANET_SIZE as f32 / 8.0; // 32.0

    let dist_from_center = p.magnitude();
    let sphere = dist_from_center - planet_r;

    let dir = if dist_from_center > 1e-6 {
        p / dist_from_center
    } else {
        vec3(0.0, 1.0, 0.0)
    };

    // Mountains
    let noise_freq = 3.0;
    let mountain_height = planet_r * 0.5;
    let raw = fbm(dir * noise_freq, 6);
    let ridged = 1.0 - (raw * 2.0 - 1.0).abs();
    let mountain = ridged * mountain_height;
    let terrain = sphere - mountain;

    // Caves: only carve underground, fading in over a shell a few units thick
    // so the outer silhouette is never broken
    let depth_below_surface = -terrain; // positive when underground
    let fade_zone = planet_r * 0.1; // 3.2 units deep before caves fully appear
    let cave_blend = (depth_below_surface / fade_zone).clamp(0.0, 1.0);

    if cave_blend > 0.0 {
        let cave = cave_sdf(p);
        // SDF subtraction: max(terrain, -cave) carves cave volume out of terrain
        let carved = terrain.max(-cave);
        // Blend smoothly from terrain to carved based on depth
        terrain + (carved - terrain) * cave_blend
    } else {
        terrain
    }
}

const THRESHOLD: f32 = 0.1;

fn should_subdivide(node: OctreeNode, camera_pos: Vector3<f32>) -> bool {
    let center = node.min + vec3(node.size * 0.5, node.size * 0.5, node.size * 0.5);
    let dist = (center - camera_pos).magnitude();

    let error = node.size / dist;

    error > THRESHOLD
}

pub fn build_node(min: Vector3<f32>, size: f32, min_size: f32, first: bool) -> OctreeNode {
    let camera_pos = vec3(0.0, PLANET_SIZE as f32 / 4.0, 0.0);
    if !first {
        let leaf = OctreeNode {
            min,
            size,
            children: None,
            vertex: None,
        };

        if !has_surface(min, size) {
            return leaf;
        }

        if size <= min_size || !should_subdivide(leaf, camera_pos) {
            return OctreeNode {
                min,
                size,
                children: None,
                vertex: None,
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
        )),
        Box::new(build_node(
            min + vec3(child_size, 0.0, 0.0),
            child_size,
            min_size,
            false,
        )),
        Box::new(build_node(
            min + vec3(0.0, child_size, 0.0),
            child_size,
            min_size,
            false,
        )),
        Box::new(build_node(
            min + vec3(child_size, child_size, 0.0),
            child_size,
            min_size,
            false,
        )),
        Box::new(build_node(
            min + vec3(0.0, 0.0, child_size),
            child_size,
            min_size,
            false,
        )),
        Box::new(build_node(
            min + vec3(child_size, 0.0, child_size),
            child_size,
            min_size,
            false,
        )),
        Box::new(build_node(
            min + vec3(0.0, child_size, child_size),
            child_size,
            min_size,
            false,
        )),
        Box::new(build_node(
            min + vec3(child_size, child_size, child_size),
            child_size,
            min_size,
            false,
        )),
    ]);

    OctreeNode {
        min,
        size,
        children,
        vertex: None,
    }
}

fn has_surface(min: Vector3<f32>, size: f32) -> bool {
    let mut has_neg = false;
    let mut has_pos = false;

    for dx in [0.0, size] {
        for dy in [0.0, size] {
            for dz in [0.0, size] {
                let p = min + vec3(dx, dy, dz);
                let d = sdf(p);

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

fn rebuild_octree_debug(state: &mut engine::State) {
    let size = PLANET_SIZE;
    let depth = OCTREE_DEBUG_DEPTH.load(Ordering::Relaxed);

    let debug_pass_node: &mut DebugPassNode = state
        .renderer
        .render_graph
        .get_node_mut::<DebugPassNode>(1)
        .unwrap();

    debug_pass_node.clear_wire_cubes();

    let octree = Planet::create_octree(size as u32 / 2);
    let mut octree_nodes = Vec::new();
    collect_octree_nodes_at_depth(&octree, 0, depth, &mut octree_nodes);
    let color = depth_color(depth);
    for (center, node_size) in &octree_nodes {
        debug_pass_node.add_wire_cube(*center, *node_size, color);
    }
}

fn collect_octree_nodes_at_depth(
    node: &OctreeNode,
    current_depth: u32,
    target_depth: u32,
    out: &mut Vec<(Point3<f32>, f32)>,
) {
    if current_depth == target_depth {
        let half = node.size / 2.0;
        let center = Point3::new(node.min.x + half, node.min.y + half, node.min.z + half);
        out.push((center, node.size));
        return;
    }

    if let Some(children) = &node.children {
        for child in children.iter() {
            collect_octree_nodes_at_depth(child, current_depth + 1, target_depth, out);
        }
    }
}

fn octree_max_depth(node: &OctreeNode, current: u32) -> u32 {
    match &node.children {
        None => current,
        Some(children) => children
            .iter()
            .map(|c| octree_max_depth(c, current + 1))
            .max()
            .unwrap_or(current),
    }
}
