#[cfg(feature = "dynamic_linking")]
#[allow(unused_imports)]
use engine_dylib;

use cgmath::{self, Array, EuclideanSpace, Vector3, vec3};
use cgmath::{InnerSpace, Point3};
use engine::assets;
use engine::assets::material::Material;
use engine::engine_info;
use engine::model::{ModelVertex, Vertex};
use engine::renderer::{CullMode, DepthState, PipelineHandle};
use engine::renderer::{DebugPassNode, GeometryPassNode};
use engine::renderer::{RenderData, RenderNode};
use engine::{KeyCode, model::MeshAsset};
use game_types::octree::OctreeNode;
use game_types::planet::{Planet, PlanetVertex};
use game_types::planet::{PlanetInstance, PlanetMesh};
pub use game_types::render_graph;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock, mpsc};
use std::{cmp, env};

struct GameState {
    previous_leaves: HashMap<NodeKey, ChunkInfo>,
    current_meshes: HashMap<NodeKey, RenderData>,
    in_flight: HashSet<NodeKey>,
    // Keys whose worker finished but produced zero vertices. Remembered so
    // the scheduler never re-spawns a worker for them on subsequent frames.
    // Pruned by retain() when the key leaves the current octree, so a fresh
    // NodeKey (different position or size) always gets a clean attempt.
    empty_chunks: HashSet<NodeKey>,
    solid_material: Material,
    line_material: Material,
    update_octree: bool,
}

#[derive(Clone, Copy)]
struct ChunkInfo {
    center: Point3<f32>,
    size: f32,
}

#[derive(Hash, Eq, PartialEq, Clone, Copy, Debug)]
struct NodeKey {
    x: i32,
    y: i32,
    z: i32,
    size: i32,
}

#[derive(Hash, Eq, PartialEq, Clone, Copy)]
struct ChunkState {
    x: i32,
    y: i32,
    z: i32,
    size: i32,
}

struct ReadyChunk {
    key: NodeKey,
    vertices: Vec<PlanetVertex>,
    indices: Vec<u32>,
}

fn spawn_chunk_worker(center: Point3<f32>, size: f32, key: NodeKey, tx: mpsc::Sender<ReadyChunk>) {
    rayon::spawn(move || {
        let resolution = size / CHUNK_SIZE as f32;
        let min_corner = Point3::new(
            center.x - 16.0 * resolution,
            center.y - 16.0 * resolution,
            center.z - 16.0 * resolution,
        );
        let grid = Planet::generate_grid(34, 34, 34, resolution, center);
        let (vertices, indices) = Planet::dual_contour_grid(&grid, min_corner, resolution);
        let _ = tx.send(ReadyChunk {
            key,
            vertices,
            indices,
        });
    });
}

struct PlanetWorkerCoord {
    tx: mpsc::Sender<ReadyChunk>,
    rx: mpsc::Receiver<ReadyChunk>,
    solid_material: Option<Material>,
    scheduled: usize,
    completed: usize,
}

static WORKER_COORD: OnceLock<Mutex<PlanetWorkerCoord>> = OnceLock::new();

fn worker_coord() -> &'static Mutex<PlanetWorkerCoord> {
    WORKER_COORD.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        Mutex::new(PlanetWorkerCoord {
            tx,
            rx,
            solid_material: None,
            scheduled: 0,
            completed: 0,
        })
    })
}

const UPLOAD_BUDGET: std::time::Duration = std::time::Duration::from_millis(2);

const PLANET_SIZE: usize = 65536 * 16;
const CHUNK_SIZE: usize = 32;

static OCTREE_DEBUG_DEPTH: AtomicU32 = AtomicU32::new(0);
static OCTREE_MAX_DEPTH: AtomicU32 = AtomicU32::new(0);

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

fn depth_color(depth: u32) -> [f32; 4] {
    DEPTH_COLORS[depth as usize % DEPTH_COLORS.len()]
}

#[unsafe(no_mangle)]
pub fn register_systems(state: &mut engine::State) {
    if vec3(
        state.camera.position.x,
        state.camera.position.y,
        state.camera.position.z,
    )
    .magnitude()
        > PLANET_SIZE as f32
    {
        state.camera.position = cgmath::point3(0.0, PLANET_SIZE as f32, 0.0);
        // Look down at the planet center, slight forward tilt so +Z isn't degenerate.
        state.camera.orientation = engine::camera::Camera::look_at(
            vec3(0.01, -1.0, 0.0).normalize(),
            vec3(0.0, 0.0, -1.0),
        );
    }

    let solid_material = Material::new("shaders/planet_terrain.wgsl".to_string())
        .with_vertex_layouts(vec![PlanetVertex::layout(), PlanetInstance::layout()])
        .with_cull(CullMode::None);

    let line_material = Material::new("shaders/planet_terrain2.wgsl".to_string())
        .with_vertex_layouts(vec![PlanetVertex::layout(), PlanetInstance::layout()])
        .with_topology(engine::renderer::Topology::LineList)
        .with_cull(CullMode::None);

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

    state.game_data = Box::new(GameState {
        previous_leaves: HashMap::new(),
        current_meshes: HashMap::new(),
        in_flight: HashSet::new(),
        empty_chunks: HashSet::new(),
        solid_material,
        line_material,
        update_octree: true,
    });
}

#[unsafe(no_mangle)]
pub fn render() {}

#[unsafe(no_mangle)]
pub fn update(state: &mut engine::State) {
    for transform in &mut state.scene.transform_components {
        transform.scale = (0.01, 0.01, 0.01).into();
        transform.position -= transform.velocity;
    }

    {
        let game_state = state.game_data.downcast_mut::<GameState>().unwrap();
        if !game_state.update_octree {
            return;
        }
    }

    let size = PLANET_SIZE;
    let octree = Planet::create_octree(size as u32 / 2, &state.camera.position);
    let max_depth = octree_max_depth(&octree, 0);
    OCTREE_MAX_DEPTH.store(max_depth, Ordering::Relaxed);
    OCTREE_DEBUG_DEPTH.store(0, Ordering::Relaxed);

    let mut octree_nodes = Vec::new();
    Planet::collect_leaf_nodes(&octree, 0, &mut octree_nodes);

    let debug_pass_node: &mut DebugPassNode = state
        .renderer
        .render_graph
        .get_node_mut::<DebugPassNode>(1)
        .unwrap();

    debug_pass_node.clear_wire_cubes();
    debug_pass_node.clear_cubes();

    let mut current_leaves: HashMap<NodeKey, ChunkInfo> = HashMap::new();
    for (center, node_size, node_depth) in &octree_nodes {
        debug_pass_node.add_wire_cube(*center, *node_size, depth_color(*node_depth));
        debug_pass_node.add_cube(
            *center + vec3(0.0, node_size / 2.0, 0.0),
            1.0,
            depth_color(node_depth + 1),
        );

        current_leaves.insert(
            NodeKey {
                x: center.x as i32,
                y: center.y as i32,
                z: center.z as i32,
                size: *node_size as i32,
            },
            ChunkInfo {
                center: *center,
                size: *node_size,
            },
        );
    }

    let tx_template = {
        let game_state = state.game_data.downcast_mut::<GameState>().unwrap();
        let mut coord = worker_coord().lock().unwrap();
        if coord.solid_material.is_none() {
            coord.solid_material = Some(game_state.solid_material.clone());
        }
        coord.tx.clone()
    };

    // Step 1: prune in_flight keys that are no longer in the current octree.
    {
        let game_state = state.game_data.downcast_mut::<GameState>().unwrap();
        game_state
            .in_flight
            .retain(|key| current_leaves.contains_key(key));
    }

    // Step 2: drain finished chunks BEFORE pruning current_meshes so results
    // that still belong to the current octree survive the retain below.
    drain_planet_chunks(state);

    {
        let game_state = state.game_data.downcast_mut::<GameState>().unwrap();

        // Step 3: prune the empty-chunk cache to the current octree.
        game_state
            .empty_chunks
            .retain(|key| current_leaves.contains_key(key));

        // Defer eviction: keep a stale chunk visible until every current
        // leaf that spatially overlaps it has been processed (mesh uploaded
        // or known-empty). Avoids 1-frame holes during LOD subdivide/merge.
        // Stale chunks with no overlapping leaf (camera panned away) are
        // evicted immediately because the .all() is vacuously true.
        let stale_keys: Vec<NodeKey> = game_state
            .current_meshes
            .keys()
            .filter(|k| !current_leaves.contains_key(k))
            .copied()
            .collect();
        for stale_key in stale_keys {
            let s_half = stale_key.size as f32 * 0.5;
            let s_cx = stale_key.x as f32;
            let s_cy = stale_key.y as f32;
            let s_cz = stale_key.z as f32;
            let all_covered = current_leaves.iter().all(|(leaf_key, info)| {
                let max_d = info.size * 0.5 + s_half;
                let overlaps = (info.center.x - s_cx).abs() < max_d
                    && (info.center.y - s_cy).abs() < max_d
                    && (info.center.z - s_cz).abs() < max_d;
                if !overlaps {
                    return true;
                }
                game_state.current_meshes.contains_key(leaf_key)
                    || game_state.empty_chunks.contains(leaf_key)
            });
            if all_covered {
                game_state.current_meshes.remove(&stale_key);
            }
        }

        // Step 4: schedule workers only for keys that are truly missing.
        for (key, info) in &current_leaves {
            if game_state.current_meshes.contains_key(key)
                || game_state.in_flight.contains(key)
                || game_state.empty_chunks.contains(key)
            {
                continue;
            }

            game_state.in_flight.insert(*key);
            worker_coord().lock().unwrap().scheduled += 1;
            spawn_chunk_worker(info.center, info.size, *key, tx_template.clone());
        }

        // Snapshot for next frame's diff.
        game_state.previous_leaves.clear();
        for (key, info) in &current_leaves {
            game_state.previous_leaves.insert(*key, *info);
        }
    }

    // Rebuild the geometry pass from the authoritative current_meshes map.
    let game_state = state.game_data.downcast_mut::<GameState>().unwrap();
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
        node.clear_render_data();
        for render_data in game_state.current_meshes.values() {
            node.add_render_data(render_data.clone());
        }
    }

    state.frame_index += 1;

    let coord = worker_coord().lock().unwrap();
    println!(
        "planet chunks: {} / {} uploaded | in_flight: {} | empty: {}",
        coord.completed,
        coord.scheduled,
        state
            .game_data
            .downcast_ref::<GameState>()
            .unwrap()
            .in_flight
            .len(),
        state
            .game_data
            .downcast_ref::<GameState>()
            .unwrap()
            .empty_chunks
            .len(),
    );
}

fn drain_planet_chunks(state: &mut engine::State) {
    let start = std::time::Instant::now();
    let coord_mutex = worker_coord();

    let material = {
        let coord = coord_mutex.lock().unwrap();
        match &coord.solid_material {
            Some(m) => m.clone(),
            None => return,
        }
    };

    let mut uploaded = 0usize;
    loop {
        if start.elapsed() >= UPLOAD_BUDGET {
            break;
        }

        let chunk = {
            let coord = coord_mutex.lock().unwrap();
            match coord.rx.try_recv() {
                Ok(c) => c,
                Err(_) => break,
            }
        };

        let game_state = state.game_data.downcast_mut::<GameState>().unwrap();
        game_state.in_flight.remove(&chunk.key);

        if chunk.vertices.is_empty() {
            // Remember this key produces no geometry so the scheduler never
            // re-spawns a worker for it every frame.
            game_state.empty_chunks.insert(chunk.key); // <-- THE key fix
            continue;
        }

        let vertex_bytes: Vec<u8> = bytemuck::cast_slice(&chunk.vertices).to_vec();
        let render_data = state.renderer.renderer_api.create_render_data(
            &vertex_bytes,
            &chunk.indices,
            material.clone(),
            &PipelineHandle(0),
        );
        state
            .game_data
            .downcast_mut::<GameState>()
            .unwrap()
            .current_meshes
            .insert(chunk.key, render_data);
        uploaded += 1;
    }

    if uploaded > 0 {
        let mut coord = coord_mutex.lock().unwrap();
        coord.completed += uploaded;
        println!(
            "planet chunks: {} / {} uploaded ({} this frame, {:?})",
            coord.completed,
            coord.scheduled,
            uploaded,
            start.elapsed()
        );
    }
}

#[unsafe(no_mangle)]
pub fn handle_key_press(state: &mut engine::State, key_code: KeyCode, pressed: bool) {
    let game_state = state.game_data.downcast_mut::<GameState>().unwrap();

    if key_code == KeyCode::KeyU && pressed {
        for i in 0..cmp::min(state.scene.transform_components.len(), 3) {
            state.scene.transform_components[i].position.y += 0.1;
        }
    }

    if key_code == KeyCode::KeyK && pressed {
        game_state.update_octree = !game_state.update_octree;
    }

    if key_code == KeyCode::F9 && pressed {
        let camera_layout = state
            .renderer
            .render_graph
            .get_node_mut::<GeometryPassNode>(0)
            .and_then(|node| node.camera_bind_group_layout)
            .expect("GeometryPassNode must be compiled before creating pipelines");

        game_state.solid_material.pipeline_descriptor.topology =
            engine::renderer::Topology::LineList;

        state
            .renderer
            .renderer_api
            .update_pipeline(&game_state.solid_material, &[camera_layout]);
    }

    if key_code == KeyCode::F10 && pressed {
        let camera_layout = state
            .renderer
            .render_graph
            .get_node_mut::<GeometryPassNode>(0)
            .and_then(|node| node.camera_bind_group_layout)
            .expect("GeometryPassNode must be compiled before creating pipelines");

        game_state.solid_material.pipeline_descriptor.topology =
            engine::renderer::Topology::TriangleList;

        state
            .renderer
            .renderer_api
            .update_pipeline(&game_state.solid_material, &[camera_layout]);
    }

    if key_code == KeyCode::KeyL && pressed {
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
            node.clear_render_data();
        }
    }

    if key_code == KeyCode::BracketLeft && pressed {
        let max = OCTREE_MAX_DEPTH.load(Ordering::Relaxed);
        let old = OCTREE_DEBUG_DEPTH.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |d| {
            if d < max { Some(d + 1) } else { None }
        });
        if old.is_ok() {
            rebuild_octree_debug(state);
        }
    }

    if key_code == KeyCode::PageUp && pressed {
        state.camera.position = cgmath::point3(0.0, PLANET_SIZE as f32, 0.0);
    }
    if key_code == KeyCode::PageDown && pressed {
        state.camera.position = cgmath::point3(0.0, 0.0, 0.0);
    }

    if key_code == KeyCode::KeyJ && pressed {
        let debug_pass_node: &mut DebugPassNode = state
            .renderer
            .render_graph
            .get_node_mut::<DebugPassNode>(1)
            .unwrap();

        if debug_pass_node.wire_cubes.len() > 0 {
            debug_pass_node.clear_wire_cubes();
            debug_pass_node.clear_cubes();
            return;
        }

        let octree = Planet::create_octree(PLANET_SIZE as u32 / 2, &state.camera.position);
        let mut octree_nodes = Vec::new();
        Planet::collect_leaf_nodes(&octree, 0, &mut octree_nodes);
        println!("Leaf nodes: {}", octree_nodes.len());
        for (center, node_size, node_depth) in &octree_nodes {
            debug_pass_node.add_wire_cube(*center, *node_size, depth_color(*node_depth));
            debug_pass_node.add_cube(
                *center + vec3(0.0, node_size / 2.0, 0.0),
                1.0,
                depth_color(node_depth + 1),
            );
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
        schedule_planet_generation(state);
    }
}

fn schedule_planet_generation(state: &mut engine::State) {
    let debug_pass_node: &mut DebugPassNode = state
        .renderer
        .render_graph
        .get_node_mut::<DebugPassNode>(1)
        .unwrap();
    debug_pass_node.clear_wire_cubes();
    debug_pass_node.clear_cubes();

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
        node.clear_render_data();
    }

    let size = PLANET_SIZE;
    let octree = Planet::create_octree(size as u32 / 2, &state.camera.position);
    let max_depth = octree_max_depth(&octree, 0);
    OCTREE_MAX_DEPTH.store(max_depth, Ordering::Relaxed);
    OCTREE_DEBUG_DEPTH.store(0, Ordering::Relaxed);

    let mut octree_nodes = Vec::new();
    Planet::collect_leaf_nodes(&octree, 0, &mut octree_nodes);
    println!(
        "planet gen: scheduling {} chunks across rayon workers",
        octree_nodes.len()
    );

    let tx_template = {
        let mut coord = worker_coord().lock().unwrap();
        while coord.rx.try_recv().is_ok() {}
        coord.solid_material = Some(
            state
                .game_data
                .downcast_mut::<GameState>()
                .unwrap()
                .solid_material
                .clone(),
        );
        coord.scheduled = octree_nodes.len();
        coord.completed = 0;
        coord.tx.clone()
    };

    // Full reset — clear all tracking state since we're starting fresh.
    {
        let game_state = state.game_data.downcast_mut::<GameState>().unwrap();
        game_state.in_flight.clear();
        game_state.empty_chunks.clear();
        game_state.current_meshes.clear();
        game_state.previous_leaves.clear();
    }

    for (center, node_size, _depth) in octree_nodes {
        let key = NodeKey {
            x: center.x as i32,
            y: center.y as i32,
            z: center.z as i32,
            size: node_size as i32,
        };
        spawn_chunk_worker(center, node_size, key, tx_template.clone());
    }
}

trait PlanetExt {
    fn generate_planet(state: &mut engine::State) -> Self;
    fn load_meshes(
        &mut self,
        state: &mut engine::State,
        solid_material: &Material,
        line_material: &Material,
    );
    fn generate_grid(
        x: u32,
        y: u32,
        z: u32,
        resolution: f32,
        center: Point3<f32>,
    ) -> Vec<Vec<Vec<f32>>>;
    fn dual_contour_grid(
        grid: &Vec<Vec<Vec<f32>>>,
        offset: Point3<f32>,
        resolution: f32,
    ) -> (Vec<PlanetVertex>, Vec<u32>);
    fn create_octree(planet_radius: u32, camera_position: &cgmath::Point3<f32>) -> OctreeNode;
    fn collect_leaf_nodes(
        node: &OctreeNode,
        current_depth: u32,
        out: &mut Vec<(Point3<f32>, f32, u32)>,
    );
}

impl PlanetExt for Planet {
    fn generate_planet(state: &mut engine::State) -> Self {
        let size: usize = PLANET_SIZE;
        let debug_pass_node: &mut DebugPassNode = state
            .renderer
            .render_graph
            .get_node_mut::<DebugPassNode>(1)
            .unwrap();

        println!(
            "Amount of nodes to cover entire planet: {:?}",
            (PLANET_SIZE / CHUNK_SIZE) * (PLANET_SIZE / CHUNK_SIZE) * (PLANET_SIZE / CHUNK_SIZE)
        );

        let octree = Planet::create_octree(size as u32 / 2, &state.camera.position);
        let max_depth = octree_max_depth(&octree, 0);
        OCTREE_MAX_DEPTH.store(max_depth, Ordering::Relaxed);
        OCTREE_DEBUG_DEPTH.store(0, Ordering::Relaxed);

        Planet {
            id: 0,
            name: String::new(),
            octree_root: octree,
        }
    }

    fn load_meshes(
        &mut self,
        state: &mut engine::State,
        solid_material: &Material,
        line_material: &Material,
    ) {
        let debug_pass_node: &mut DebugPassNode = state
            .renderer
            .render_graph
            .get_node_mut::<DebugPassNode>(1)
            .unwrap();

        let mut octree_nodes = Vec::new();
        Planet::collect_leaf_nodes(&self.octree_root, 0, &mut octree_nodes);
        println!("Leaf nodes: {}", octree_nodes.len());

        let mut meshes_count = 0;

        for (center, node_size, node_depth) in &octree_nodes {
            let resolution = node_size / CHUNK_SIZE as f32;
            let min_corner = Point3::new(
                center.x - 16.0 * resolution,
                center.y - 16.0 * resolution,
                center.z - 16.0 * resolution,
            );
            let (positions, indices) = Planet::dual_contour_grid(
                &Planet::generate_grid(34, 34, 34, resolution, *center),
                min_corner,
                resolution,
            );

            let mesh = PlanetMesh { positions, indices };

            if mesh.positions.len() > 0 {
                meshes_count += 1;
                println!("Added meshes: {}", meshes_count);
                let vertex_bytes: Vec<u8> = bytemuck::cast_slice(&mesh.positions).to_vec();
                let solid_render_data = state.renderer.renderer_api.create_render_data(
                    &vertex_bytes,
                    &mesh.indices,
                    solid_material.clone(),
                    &PipelineHandle(0),
                );

                let line_render_data = state.renderer.renderer_api.create_render_data(
                    &vertex_bytes,
                    &mesh.indices,
                    line_material.clone(),
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
                }
            }
        }
        println!("Added meshes: {}", meshes_count);
    }

    fn generate_grid(
        nx: u32,
        ny: u32,
        nz: u32,
        resolution: f32,
        center: Point3<f32>,
    ) -> Vec<Vec<Vec<f32>>> {
        let half = (nx - 1) as f32 * resolution / 2.0;
        let min = cgmath::Vector3::new(center.x - half, center.y - half, center.z - half);

        let mut grid = Vec::new();
        for xi in 0..nx {
            let mut plane = Vec::new();
            for yi in 0..ny {
                let mut row = Vec::new();
                for zi in 0..nz {
                    let position = cgmath::Vector3::new(
                        min.x + xi as f32 * resolution,
                        min.y + yi as f32 * resolution,
                        min.z + zi as f32 * resolution,
                    );
                    row.push(sdf(position));
                }
                plane.push(row);
            }
            grid.push(plane);
        }
        grid
    }

    fn dual_contour_grid(
        grid: &Vec<Vec<Vec<f32>>>,
        offset: Point3<f32>,
        resolution: f32,
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
                        let dx = sdf(p + vec3(eps, 0.0, 0.0)) - sdf(p - vec3(eps, 0.0, 0.0));
                        let dy = sdf(p + vec3(0.0, eps, 0.0)) - sdf(p - vec3(0.0, eps, 0.0));
                        let dz = sdf(p + vec3(0.0, 0.0, eps)) - sdf(p - vec3(0.0, 0.0, eps));
                        let n = vec3(dx, dy, dz).normalize();
                        [n.x, n.y, n.z]
                    };

                    let avg_norm = normal_at(Vector3::new(avg_pos.x, avg_pos.y, avg_pos.z));
                    let up = vec3(0.0, 1.0, 0.0);
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

    fn create_octree(planet_radius: u32, camera_position: &cgmath::Point3<f32>) -> OctreeNode {
        let r = planet_radius as f32 / 2.0;
        build_node(
            Vector3 {
                x: -r,
                y: -r,
                z: -r,
            },
            planet_radius as f32,
            CHUNK_SIZE as f32,
            true,
            camera_position,
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

fn hash3(p: Vector3<f32>) -> f32 {
    let ix = (p.x.floor() as i32).wrapping_mul(1619);
    let iy = (p.y.floor() as i32).wrapping_mul(31337);
    let iz = (p.z.floor() as i32).wrapping_mul(6271);
    let n = ix.wrapping_add(iy).wrapping_add(iz);
    let n = n.wrapping_mul(n.wrapping_mul(n).wrapping_mul(60493).wrapping_add(19990303));
    (n as u32 as f32) / (u32::MAX as f32)
}

fn smooth_noise(p: Vector3<f32>) -> f32 {
    let ix = p.x.floor();
    let iy = p.y.floor();
    let iz = p.z.floor();
    let fx = p.x - ix;
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

fn cave_sdf(p: Vector3<f32>) -> f32 {
    let scale = 0.04;
    let q = p * scale;
    let warp = vec3(
        fbm(q + vec3(1.7, 9.2, 3.4), 3),
        fbm(q + vec3(8.3, 2.8, 5.1), 3),
        fbm(q + vec3(4.1, 6.7, 1.9), 3),
    ) * 2.0
        - vec3(1.0, 1.0, 1.0);
    let warped = q + warp * 0.6;
    let tunnel_r = fbm(warped, 4);
    let cave_dist = (tunnel_r - 0.5).abs() - 0.08;
    cave_dist * (1.0 / scale)
}

pub fn sdf(p: cgmath::Vector3<f32>) -> f32 {
    let planet_r = PLANET_SIZE as f32 / 8.0;
    let dist_from_center = p.magnitude();
    let sphere = dist_from_center - planet_r;
    let dir = if dist_from_center > 1e-6 {
        p / dist_from_center
    } else {
        vec3(0.0, 1.0, 0.0)
    };

    let noise_freq = 15.0;
    let mountain_height = planet_r * 0.05;
    let raw = fbm(dir * noise_freq, 6);
    let ridged = 1.0 - (raw * 2.0 - 1.0).abs();
    let mountain = ridged * mountain_height;
    let terrain = sphere - mountain;
    return terrain;

    let depth_below_surface = -terrain;
    let fade_zone = planet_r * 0.1;
    let cave_blend = (depth_below_surface / fade_zone).clamp(0.0, 1.0);
    if cave_blend > 0.0 {
        let cave = cave_sdf(p);
        let carved = terrain.max(-cave);
        terrain + (carved - terrain) * cave_blend
    } else {
        terrain
    }
}

const THRESHOLD: f32 = 0.3;

fn is_behind_horizon(
    node_center: Vector3<f32>,
    camera_pos: Vector3<f32>,
    planet_center: Vector3<f32>,
) -> bool {
    let to_node = (node_center - planet_center).normalize();
    let to_camera = (camera_pos - planet_center).normalize();
    cgmath::dot(to_node, to_camera) < 0.0
}

fn should_subdivide(node: OctreeNode, camera_pos: Vector3<f32>) -> bool {
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
) -> OctreeNode {
    let has_surface = has_surface(min, size);
    let is_behind_horizon = is_behind_horizon(
        min + vec3(size * 0.5, size * 0.5, size * 0.5),
        vec3(camera_position.x, camera_position.y, camera_position.z),
        vec3(0.0, 0.0, 0.0),
    );

    if !first {
        let leaf = OctreeNode {
            min,
            size,
            children: None,
            vertex: None,
            has_surface: false,
        };

        if !has_surface && size < PLANET_SIZE as f32 / 4.0 {
            return leaf;
        }

        if size <= min_size
            || !should_subdivide(
                leaf,
                vec3(camera_position.x, camera_position.y, camera_position.z),
            )
        {
            return OctreeNode {
                min,
                size,
                children: None,
                vertex: None,
                has_surface,
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
        )),
        Box::new(build_node(
            min + vec3(child_size, 0.0, 0.0),
            child_size,
            min_size,
            false,
            camera_position,
        )),
        Box::new(build_node(
            min + vec3(0.0, child_size, 0.0),
            child_size,
            min_size,
            false,
            camera_position,
        )),
        Box::new(build_node(
            min + vec3(child_size, child_size, 0.0),
            child_size,
            min_size,
            false,
            camera_position,
        )),
        Box::new(build_node(
            min + vec3(0.0, 0.0, child_size),
            child_size,
            min_size,
            false,
            camera_position,
        )),
        Box::new(build_node(
            min + vec3(child_size, 0.0, child_size),
            child_size,
            min_size,
            false,
            camera_position,
        )),
        Box::new(build_node(
            min + vec3(0.0, child_size, child_size),
            child_size,
            min_size,
            false,
            camera_position,
        )),
        Box::new(build_node(
            min + vec3(child_size, child_size, child_size),
            child_size,
            min_size,
            false,
            camera_position,
        )),
    ]);

    OctreeNode {
        min,
        size,
        children,
        vertex: None,
        has_surface,
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
    debug_pass_node.clear_cubes();

    let octree = Planet::create_octree(size as u32 / 2, &state.camera.position);
    let mut octree_nodes = Vec::new();
    collect_octree_nodes_at_depth(&octree, 0, depth, &mut octree_nodes);
    for (center, node_size, node_depth) in &octree_nodes {
        debug_pass_node.add_wire_cube(*center, *node_size, depth_color(*node_depth));
    }
}

fn collect_octree_nodes_at_depth(
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
