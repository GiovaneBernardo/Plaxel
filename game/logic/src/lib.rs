use engine::core::components::core::{CameraComponent, TransformComponent};
use engine::core::input::KeyCode;
use engine::ecs::entity::Entity;
use engine::ecs::system::SystemContext;
use engine::ecs::world::World;
#[cfg(feature = "dynamic_linking")]
#[allow(unused_imports)]
use engine_dylib;

use cgmath::{self, EuclideanSpace, Quaternion, Vector3, vec3};
use cgmath::{InnerSpace, Point3};
use engine::assets::material::Material;
use engine::core::components::physics::RapierColliderHandle;
use engine::core::physics::physics::Physics;
use engine::model::Vertex;
use engine::renderer;
use engine::renderer::Topology;
use engine::renderer::{CameraData, FrameBindings, RenderData};
use engine::renderer::{CullMode, PipelineHandle};
use engine::renderer::{DebugPassNode, GeometryPassNode};
use game_types::octree::OctreeNode;
use game_types::planet::{Planet, PlanetVertex};
use game_types::planet::{PlanetInstance, PlanetMesh};
pub use game_types::render_graph;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, mpsc};
use web_time::{Duration, Instant};

mod systems;

use game_types::game_mode::{GameMode, GameModeState};
use systems::{InputMap, player_interaction_system};

struct GameState {
    previous_leaves: HashMap<NodeKey, ChunkInfo>,
    current_leaves: HashMap<NodeKey, ChunkInfo>,
    current_meshes: HashMap<NodeKey, RenderData>,
    mesh_neighbor_signatures: HashMap<NodeKey, NeighborSignature>,
    terrain_colliders: HashMap<NodeKey, RapierColliderHandle>,
    in_flight: HashSet<NodeKey>,
    // Keys whose worker finished but produced zero vertices. Remembered so
    // the scheduler never re-spawns a worker for them on subsequent frames.
    // Pruned by retain() when the key leaves the current octree, so a fresh
    // NodeKey (different position or size) always gets a clean attempt.
    empty_chunks: HashSet<NodeKey>,
    empty_neighbor_signatures: HashMap<NodeKey, NeighborSignature>,
    solid_material: Material,
    update_octree: bool,
    debug_nodes: Vec<(Point3<f32>, f32, u32)>,
    debug_depth: u32,
    max_depth: u32,
    octree_job_in_flight: bool,
    last_requested_camera_pos: Point3<f32>,
    octree_tx: mpsc::Sender<OctreeBuildResult>,
    octree_rx: Mutex<mpsc::Receiver<OctreeBuildResult>>,
}

struct GameCamera {
    entity: Entity,
    camera: engine::camera::Camera,
    controller: engine::camera::CameraController,
    uniform: engine::camera::CameraUniform,
    velocity_sample_pos: Point3<f32>,
    velocity_sample_time: Instant,
    velocity_sample_distance: f32,
}

#[derive(Clone, Copy)]
struct ChunkInfo {
    center: Point3<f32>,
    size: f32,
}

#[derive(Hash, Eq, PartialEq, Clone, Copy, Debug, PartialOrd, Ord)]
struct NodeKey {
    x: i32,
    y: i32,
    z: i32,
    size: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NeighborSignature(Vec<NodeKey>);

#[derive(Clone, Copy, Debug)]
struct ChunkNeighbor {
    key: NodeKey,
    center: Point3<f32>,
    size: f32,
}

#[derive(Hash, Eq, PartialEq, Clone, Copy)]
#[allow(dead_code)]
struct ChunkState {
    x: i32,
    y: i32,
    z: i32,
    size: i32,
}

struct ReadyChunk {
    key: NodeKey,
    neighbor_signature: NeighborSignature,
    vertices: Vec<PlanetVertex>,
    indices: Vec<u32>,
}

struct OctreeBuildResult {
    camera_pos: Point3<f32>,
    max_depth: u32,
    leaves: Vec<(Point3<f32>, f32, u32)>,
}

fn spawn_octree_worker(camera_pos: Point3<f32>, tx: mpsc::Sender<OctreeBuildResult>) {
    #[cfg(target_arch = "wasm32")]
    {
        let octree = Planet::create_octree(PLANET_SIZE as u32 / 2, &camera_pos);
        let max_depth = octree_max_depth(&octree, 0);
        let mut leaves = Vec::new();
        Planet::collect_leaf_nodes(&octree, 0, &mut leaves);
        let _ = tx.send(OctreeBuildResult {
            camera_pos,
            max_depth,
            leaves,
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    rayon::spawn(move || {
        let octree = Planet::create_octree(PLANET_SIZE as u32 / 2, &camera_pos);
        let max_depth = octree_max_depth(&octree, 0);
        let mut leaves = Vec::new();
        Planet::collect_leaf_nodes(&octree, 0, &mut leaves);
        let _ = tx.send(OctreeBuildResult {
            camera_pos,
            max_depth,
            leaves,
        });
    });
}

fn spawn_chunk_worker(
    center: Point3<f32>,
    size: f32,
    key: NodeKey,
    neighbors: Vec<ChunkNeighbor>,
    neighbor_signature: NeighborSignature,
    tx: mpsc::Sender<ReadyChunk>,
) {
    #[cfg(target_arch = "wasm32")]
    {
        let chunk = build_chunk(center, size, key, neighbors, neighbor_signature);
        let _ = tx.send(chunk);
    }

    #[cfg(not(target_arch = "wasm32"))]
    rayon::spawn(move || {
        let chunk = build_chunk(center, size, key, neighbors, neighbor_signature);
        let _ = tx.send(chunk);
    });
}

fn build_chunk(
    center: Point3<f32>,
    size: f32,
    key: NodeKey,
    neighbors: Vec<ChunkNeighbor>,
    neighbor_signature: NeighborSignature,
) -> ReadyChunk {
    let resolution = size / CHUNK_SIZE as f32;
    let min_corner = Point3::new(
        center.x - size * 0.5,
        center.y - size * 0.5,
        center.z - size * 0.5,
    );
    let grid = generate_grid_from_min(34, 34, 34, resolution, min_corner.to_vec());
    let (vertices, indices) = Planet::dual_contour_grid(&grid, min_corner, resolution);
    let (vertices, indices) = add_lod_boundary_skirts(vertices, indices, center, size, &neighbors);
    ReadyChunk {
        key,
        neighbor_signature,
        vertices,
        indices,
    }
}

fn chunk_neighbors(
    leaves: &HashMap<NodeKey, ChunkInfo>,
    key: NodeKey,
    info: ChunkInfo,
) -> Vec<ChunkNeighbor> {
    let half = info.size * 0.5;
    let min = vec3(
        info.center.x - half,
        info.center.y - half,
        info.center.z - half,
    );
    let max = vec3(
        info.center.x + half,
        info.center.y + half,
        info.center.z + half,
    );
    let mut neighbors = Vec::new();

    for (other_key, other) in leaves {
        if *other_key == key {
            continue;
        }

        let other_half = other.size * 0.5;
        let other_min = vec3(
            other.center.x - other_half,
            other.center.y - other_half,
            other.center.z - other_half,
        );
        let other_max = vec3(
            other.center.x + other_half,
            other.center.y + other_half,
            other.center.z + other_half,
        );
        let eps = info.size.min(other.size) * 0.001;

        let touches_x =
            nearly_equal(max.x, other_min.x, eps) || nearly_equal(min.x, other_max.x, eps);
        let touches_y =
            nearly_equal(max.y, other_min.y, eps) || nearly_equal(min.y, other_max.y, eps);
        let touches_z =
            nearly_equal(max.z, other_min.z, eps) || nearly_equal(min.z, other_max.z, eps);

        let overlaps_x = ranges_overlap(min.x, max.x, other_min.x, other_max.x, eps);
        let overlaps_y = ranges_overlap(min.y, max.y, other_min.y, other_max.y, eps);
        let overlaps_z = ranges_overlap(min.z, max.z, other_min.z, other_max.z, eps);

        if (touches_x && overlaps_y && overlaps_z)
            || (touches_y && overlaps_x && overlaps_z)
            || (touches_z && overlaps_x && overlaps_y)
        {
            neighbors.push(ChunkNeighbor {
                key: *other_key,
                center: other.center,
                size: other.size,
            });
        }
    }

    neighbors.sort_by_key(|neighbor| neighbor.key);
    neighbors
}

fn neighbor_signature(neighbors: &[ChunkNeighbor]) -> NeighborSignature {
    NeighborSignature(neighbors.iter().map(|neighbor| neighbor.key).collect())
}

fn nearly_equal(a: f32, b: f32, eps: f32) -> bool {
    (a - b).abs() <= eps
}

fn ranges_overlap(a_min: f32, a_max: f32, b_min: f32, b_max: f32, eps: f32) -> bool {
    a_min < b_max - eps && b_min < a_max - eps
}

fn generate_grid_from_min(
    nx: u32,
    ny: u32,
    nz: u32,
    resolution: f32,
    min: Vector3<f32>,
) -> Vec<Vec<Vec<f32>>> {
    let mut grid = Vec::new();
    for xi in 0..nx {
        let mut plane = Vec::new();
        for yi in 0..ny {
            let mut row = Vec::new();
            for zi in 0..nz {
                let position = vec3(
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

fn add_lod_boundary_skirts(
    mut vertices: Vec<PlanetVertex>,
    mut indices: Vec<u32>,
    center: Point3<f32>,
    size: f32,
    neighbors: &[ChunkNeighbor],
) -> (Vec<PlanetVertex>, Vec<u32>) {
    if vertices.is_empty() || indices.is_empty() {
        return (vertices, indices);
    }

    let half = size * 0.5;
    let local_min = vec3(center.x - half, center.y - half, center.z - half);
    let local_max = vec3(center.x + half, center.y + half, center.z + half);
    let local_resolution = size / CHUNK_SIZE as f32;

    let mut lod_faces = Vec::new();
    for neighbor in neighbors {
        if (neighbor.size - size).abs() <= f32::EPSILON {
            continue;
        }

        let neighbor_half = neighbor.size * 0.5;
        let neighbor_min = vec3(
            neighbor.center.x - neighbor_half,
            neighbor.center.y - neighbor_half,
            neighbor.center.z - neighbor_half,
        );
        let neighbor_max = vec3(
            neighbor.center.x + neighbor_half,
            neighbor.center.y + neighbor_half,
            neighbor.center.z + neighbor_half,
        );
        let eps = local_resolution.min(neighbor.size / CHUNK_SIZE as f32) * 0.25;

        for axis in 0..3 {
            if nearly_equal(
                axis_value(local_max, axis),
                axis_value(neighbor_min, axis),
                eps,
            ) {
                lod_faces.push((
                    axis,
                    axis_value(local_max, axis),
                    neighbor_min,
                    neighbor_max,
                ));
            } else if nearly_equal(
                axis_value(local_min, axis),
                axis_value(neighbor_max, axis),
                eps,
            ) {
                lod_faces.push((
                    axis,
                    axis_value(local_min, axis),
                    neighbor_min,
                    neighbor_max,
                ));
            }
        }
    }

    if lod_faces.is_empty() {
        return (vertices, indices);
    }

    let original_indices = indices.clone();
    let mut emitted_edges = HashSet::new();
    for tri in original_indices.chunks_exact(3) {
        for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            let edge_key = if a < b { (a, b) } else { (b, a) };
            if !emitted_edges.insert(edge_key) {
                continue;
            }

            let Some(pa) = vertices
                .get(a as usize)
                .map(|v| vec3(v.position[0], v.position[1], v.position[2]))
            else {
                continue;
            };
            let Some(pb) = vertices
                .get(b as usize)
                .map(|v| vec3(v.position[0], v.position[1], v.position[2]))
            else {
                continue;
            };

            let mut on_lod_face = false;
            for (axis, boundary, neighbor_min, neighbor_max) in &lod_faces {
                if !nearly_equal(axis_value(pa, *axis), *boundary, local_resolution * 0.75)
                    || !nearly_equal(axis_value(pb, *axis), *boundary, local_resolution * 0.75)
                {
                    continue;
                }

                let u_axis = (*axis + 1) % 3;
                let v_axis = (*axis + 2) % 3;
                let a_in_overlap = axis_value(pa, u_axis)
                    >= axis_value(*neighbor_min, u_axis) - local_resolution
                    && axis_value(pa, u_axis)
                        <= axis_value(*neighbor_max, u_axis) + local_resolution
                    && axis_value(pa, v_axis)
                        >= axis_value(*neighbor_min, v_axis) - local_resolution
                    && axis_value(pa, v_axis)
                        <= axis_value(*neighbor_max, v_axis) + local_resolution;
                let b_in_overlap = axis_value(pb, u_axis)
                    >= axis_value(*neighbor_min, u_axis) - local_resolution
                    && axis_value(pb, u_axis)
                        <= axis_value(*neighbor_max, u_axis) + local_resolution
                    && axis_value(pb, v_axis)
                        >= axis_value(*neighbor_min, v_axis) - local_resolution
                    && axis_value(pb, v_axis)
                        <= axis_value(*neighbor_max, v_axis) + local_resolution;

                if a_in_overlap && b_in_overlap {
                    on_lod_face = true;
                    break;
                }
            }

            if !on_lod_face {
                continue;
            }

            let skirt_depth = local_resolution * 3.0;
            let va = vertices[a as usize];
            let vb = vertices[b as usize];
            let skirt_a = extruded_skirt_vertex(va, skirt_depth);
            let skirt_b = extruded_skirt_vertex(vb, skirt_depth);
            let skirt_ai = vertices.len() as u32;
            vertices.push(skirt_a);
            let skirt_bi = vertices.len() as u32;
            vertices.push(skirt_b);

            indices.extend_from_slice(&[a, b, skirt_ai]);
            indices.extend_from_slice(&[skirt_ai, b, skirt_bi]);
            indices.extend_from_slice(&[a, skirt_ai, b]);
            indices.extend_from_slice(&[skirt_ai, skirt_bi, b]);
        }
    }

    (vertices, indices)
}

fn extruded_skirt_vertex(mut vertex: PlanetVertex, depth: f32) -> PlanetVertex {
    vertex.position[0] -= vertex.normal[0] * depth;
    vertex.position[1] -= vertex.normal[1] * depth;
    vertex.position[2] -= vertex.normal[2] * depth;
    vertex
}

fn axis_value(v: Vector3<f32>, axis: usize) -> f32 {
    match axis {
        0 => v.x,
        1 => v.y,
        2 => v.z,
        _ => unreachable!(),
    }
}

struct PlanetWorkerCoord {
    tx: mpsc::Sender<ReadyChunk>,
    rx: Mutex<mpsc::Receiver<ReadyChunk>>,
    scheduled: usize,
    completed: usize,
}

const UPLOAD_BUDGET: Duration = Duration::from_millis(2);

const PLANET_SIZE: usize = 65536; //* 16;
const CHUNK_SIZE: usize = 32;

#[allow(dead_code)]
static OCTREE_DEBUG_DEPTH: AtomicU32 = AtomicU32::new(0);
#[allow(dead_code)]
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
    let size = state.window.inner_size();
    let aspect = size.width as f32 / size.height.max(1) as f32;

    let mut camera = engine::camera::Camera {
        position: (0.0, PLANET_SIZE as f32, 2.0).into(),
        orientation: engine::camera::Camera::look_at(
            vec3(0.01, -1.0, 0.0).normalize(),
            vec3(0.0, 0.0, -1.0),
        ),
        aspect,
        fovy: 65.0,
        znear: 0.1,
        zfar: 15_000_000.0,
    };
    if camera.position.to_vec().magnitude() > PLANET_SIZE as f32 {
        camera.position = cgmath::point3(0.0, PLANET_SIZE as f32, 0.0);
    }

    let mut uniform = engine::camera::CameraUniform::new();
    uniform.update_view_proj(&camera);
    let initial_camera_pos = camera.position;
    state
        .global_resources
        .renderer
        .render_resources
        .insert(CameraData { uniform });

    let solid_material = Material::new("shaders/planet_terrain.wgsl".to_string())
        .with_vertex_layouts(vec![PlanetVertex::layout(), PlanetInstance::layout()])
        .with_cull(CullMode::None);

    let line_material = Material::new("shaders/planet_terrain2.wgsl".to_string())
        .with_vertex_layouts(vec![PlanetVertex::layout(), PlanetInstance::layout()])
        .with_topology(Topology::LineList)
        .with_cull(CullMode::None);

    let camera_layout = state
        .global_resources
        .renderer
        .render_graph
        .get_node_mut::<GeometryPassNode>(0)
        .and_then(|node| node.camera_bind_group_layout)
        .expect("GeometryPassNode must be compiled before creating planet pipelines");
    let textures_layout = state
        .global_resources
        .renderer
        .render_resources
        .get_labeled::<FrameBindings>("frame_bindings")
        .map(|bindings| bindings.textures_layout)
        .expect(
            "Frame material bind group layout must be initialized before creating planet pipelines",
        );

    let target_info = {
        let renderer = &state.global_resources.renderer;
        let descriptor = GeometryPassNode::pass_descriptor();
        renderer
            .renderer_api
            .target_info_for_pass(&descriptor, &renderer.render_graph.resources)
    };

    state
        .global_resources
        .renderer
        .renderer_api
        .create_pipeline(
            &solid_material,
            &[camera_layout, textures_layout],
            &target_info,
        );
    state
        .global_resources
        .renderer
        .renderer_api
        .create_pipeline(
            &line_material,
            &[camera_layout, textures_layout],
            &target_info,
        );

    let (chunk_tx, chunk_rx) = mpsc::channel();
    let (octree_tx, octree_rx) = mpsc::channel();
    let scene = state.active_scene_mut().unwrap();
    let world = scene.world_mut();

    world.insert_resource(GameModeState {
        mode: GameMode::Walking,
    });
    world.insert_resource(InputMap::default());

    let velocity_sample_pos = camera.position;
    let velocity_sample_time = Instant::now();

    let camera_entity = world.spawn();
    world.insert(
        camera_entity,
        TransformComponent {
            position: cgmath::vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            scale: cgmath::vec3(1.0, 1.0, 1.0),
            velocity: cgmath::vec3(0.0, 0.0, 0.0),
        },
    );
    world.insert(
        camera_entity,
        CameraComponent {
            speed: 1.0,
            fov: 75.0,
            far_plane: 15000.0,
            near_plane: 0.01,
        },
    );

    world.insert_resource(GameCamera {
        entity: camera_entity,
        camera,
        controller: engine::camera::CameraController::new(0.2),
        uniform,
        velocity_sample_pos,
        velocity_sample_time,
        velocity_sample_distance: 0.0,
    });

    world.insert_resource(GameState {
        previous_leaves: HashMap::new(),
        current_leaves: HashMap::new(),
        current_meshes: HashMap::new(),
        mesh_neighbor_signatures: HashMap::new(),
        terrain_colliders: HashMap::new(),
        in_flight: HashSet::new(),
        empty_chunks: HashSet::new(),
        empty_neighbor_signatures: HashMap::new(),
        solid_material,
        update_octree: true,
        debug_nodes: Vec::new(),
        debug_depth: 0,
        max_depth: 0,
        octree_job_in_flight: false,
        last_requested_camera_pos: initial_camera_pos,
        octree_tx,
        octree_rx: Mutex::new(octree_rx),
    });

    world.insert_resource(PlanetWorkerCoord {
        tx: chunk_tx,
        rx: Mutex::new(chunk_rx),
        scheduled: 0,
        completed: 0,
    });

    let update_schedule_mut = scene.update_schedule_mut();

    update_schedule_mut.add_system(Physics::create_missing_rapier_bodies_system);
    update_schedule_mut.add_system(player_interaction_system);
    update_schedule_mut.add_system(camera_update_system);
    update_schedule_mut.add_system(planet_lod_system);
    update_schedule_mut.add_system(renderer::get_render_data_system);
    update_schedule_mut.add_system(engine::core::systems::systems::engine_input_system);
}

#[unsafe(no_mangle)]
pub fn render() {}

#[unsafe(no_mangle)]
pub fn update(state: &mut engine::State) {
    let Some(scene_index) = state.active_scene_index.map(|i| i as usize) else {
        return;
    };

    let (renderer, scenes) = (&mut state.global_resources.renderer, &mut state.scenes);
    let Some(scene) = scenes.get_mut(scene_index) else {
        return;
    };

    sync_camera_to_renderer(renderer, scene.world());
    drain_planet_chunks(renderer, scene.world_mut());
    sync_planet_debug(renderer, scene.world());
    sync_physics_debug(renderer, scene.world());
    sync_planet_geometry(renderer, scene.world());
    state.frame_index += 1;
}

fn camera_update_system(ctx: &mut SystemContext, _commands: &mut engine::ecs::commands::Commands) {
    let world = &mut ctx.world;
    let Some(mut camera) = world.get_resource_mut::<GameCamera>() else {
        return;
    };

    let camera_entity = camera.entity;
    let camera_transform = world.get::<TransformComponent>(camera_entity).unwrap();
    let camera_component = world.get::<CameraComponent>(camera_entity).unwrap();

    let previous_position = camera.camera.position;
    let mut controller = std::mem::replace(
        &mut camera.controller,
        engine::camera::CameraController::new(0.2),
    );
    controller.update_camera(&mut camera.camera);
    camera.camera.position = cgmath::point3::<f32>(
        camera_transform.position.x,
        camera_transform.position.y,
        camera_transform.position.z,
    );
    camera.camera.orientation = camera_transform.rotation;
    camera.camera.fovy = camera_component.fov;

    camera.controller = controller;
    update_camera_velocity_log(&mut camera, previous_position);
    let camera_copy = engine::camera::Camera {
        position: camera.camera.position,
        orientation: camera.camera.orientation,
        aspect: camera.camera.aspect,
        fovy: camera.camera.fovy,
        znear: camera.camera.znear,
        zfar: camera.camera.zfar,
    };

    camera.uniform.update_view_proj(&camera_copy);
}

fn update_camera_velocity_log(camera: &mut GameCamera, previous_position: Point3<f32>) {
    let frame_distance = (camera.camera.position - previous_position).magnitude();
    camera.velocity_sample_distance += frame_distance;

    let now = Instant::now();
    let elapsed = now
        .duration_since(camera.velocity_sample_time)
        .as_secs_f32();

    if elapsed < 1.0 {
        return;
    }

    let meters_per_second = camera.velocity_sample_distance / elapsed;
    if meters_per_second > 0.01 {
        tracing::info!(
            target: "game",
            "editor camera speed: {:.2} m/s ({:.2} km/h), position: ({:.2}, {:.2}, {:.2})",
            meters_per_second,
            meters_per_second * 3.6,
            camera.camera.position.x,
            camera.camera.position.y,
            camera.camera.position.z,
        );
    }

    camera.velocity_sample_pos = camera.camera.position;
    camera.velocity_sample_time = now;
    camera.velocity_sample_distance = 0.0;
}

fn should_request_octree_rebuild(game_state: &GameState, camera_pos: Point3<f32>) -> bool {
    if game_state.current_leaves.is_empty() {
        return true;
    }

    let distance = (camera_pos - game_state.last_requested_camera_pos).magnitude();
    let threshold = CHUNK_SIZE as f32 * 8.0;
    distance >= threshold
}

fn planet_lod_system(ctx: &mut SystemContext, _commands: &mut engine::ecs::commands::Commands) {
    let world = &mut ctx.world;
    let camera_pos = {
        let Some(camera) = world.get_resource::<GameCamera>() else {
            return;
        };
        camera.camera.position
    };

    let tx = {
        let Some(workers) = world.get_resource::<PlanetWorkerCoord>() else {
            return;
        };
        workers.tx.clone()
    };

    let Some(mut game_state) = world.get_resource_mut::<GameState>() else {
        return;
    };

    if !game_state.update_octree {
        return;
    }

    let mut latest_octree = None;
    {
        let rx = game_state.octree_rx.lock().unwrap();
        while let Ok(result) = rx.try_recv() {
            latest_octree = Some(result);
        }
    }

    if let Some(result) = latest_octree {
        let mut current_leaves = HashMap::new();
        for (center, node_size, _) in &result.leaves {
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

        game_state.octree_job_in_flight = false;
        game_state.last_requested_camera_pos = result.camera_pos;
        game_state.max_depth = result.max_depth;
        game_state.debug_nodes = result.leaves;
        game_state.current_leaves = current_leaves;
    }

    if !game_state.octree_job_in_flight && should_request_octree_rebuild(&game_state, camera_pos) {
        game_state.octree_job_in_flight = true;
        game_state.last_requested_camera_pos = camera_pos;
        spawn_octree_worker(camera_pos, game_state.octree_tx.clone());
    }

    let current_leaves = game_state.current_leaves.clone();
    if current_leaves.is_empty() {
        return;
    }

    game_state
        .in_flight
        .retain(|key| current_leaves.contains_key(key));
    game_state
        .empty_chunks
        .retain(|key| current_leaves.contains_key(key));
    game_state
        .empty_neighbor_signatures
        .retain(|key, _| current_leaves.contains_key(key));

    let stale_keys: Vec<NodeKey> = game_state
        .current_meshes
        .keys()
        .filter(|key| !current_leaves.contains_key(key))
        .copied()
        .collect();

    for stale_key in stale_keys {
        let stale_half = stale_key.size as f32 * 0.5;
        let stale_center = Point3::new(stale_key.x as f32, stale_key.y as f32, stale_key.z as f32);
        let all_covered = current_leaves.iter().all(|(leaf_key, info)| {
            let max_distance = info.size * 0.5 + stale_half;
            let overlaps = (info.center.x - stale_center.x).abs() < max_distance
                && (info.center.y - stale_center.y).abs() < max_distance
                && (info.center.z - stale_center.z).abs() < max_distance;

            !overlaps
                || game_state.current_meshes.contains_key(leaf_key)
                || game_state.empty_chunks.contains(leaf_key)
        });

        if all_covered {
            game_state.current_meshes.remove(&stale_key);
            game_state.mesh_neighbor_signatures.remove(&stale_key);
            game_state.empty_neighbor_signatures.remove(&stale_key);
            if let Some(handle) = game_state.terrain_colliders.remove(&stale_key) {
                if let Some(mut physics) = world.get_resource_mut::<Physics>() {
                    physics.remove_collider(handle.0);
                }
            }
        }
    }

    let mut scheduled = 0usize;
    for (key, info) in &current_leaves {
        let neighbors = chunk_neighbors(&current_leaves, *key, *info);
        let neighbor_signature = neighbor_signature(&neighbors);
        let mesh_is_current = game_state
            .mesh_neighbor_signatures
            .get(key)
            .is_some_and(|signature| signature == &neighbor_signature);
        let empty_is_current = game_state
            .empty_neighbor_signatures
            .get(key)
            .is_some_and(|signature| signature == &neighbor_signature);

        if game_state.current_meshes.contains_key(key) && mesh_is_current {
            continue;
        }
        if game_state.empty_chunks.contains(key) && empty_is_current {
            continue;
        }
        if game_state.in_flight.contains(key) {
            continue;
        }

        game_state.empty_chunks.remove(key);
        game_state.empty_neighbor_signatures.remove(key);
        game_state.in_flight.insert(*key);
        spawn_chunk_worker(
            info.center,
            info.size,
            *key,
            neighbors,
            neighbor_signature,
            tx.clone(),
        );
        scheduled += 1;
    }

    game_state.previous_leaves.clear();
    for (key, info) in &current_leaves {
        game_state.previous_leaves.insert(*key, *info);
    }

    drop(game_state);
    if scheduled > 0 {
        if let Some(mut workers) = world.get_resource_mut::<PlanetWorkerCoord>() {
            workers.scheduled += scheduled;
        }
    }
}

fn sync_camera_to_renderer(renderer: &mut engine::renderer::Renderer, world: &World) {
    let Some(camera) = world.get_resource::<GameCamera>() else {
        return;
    };

    renderer.render_resources.insert(CameraData {
        uniform: camera.uniform,
    });
}

fn drain_planet_chunks(renderer: &mut engine::renderer::Renderer, world: &mut World) {
    let start = Instant::now();
    let Some(mut game_state) = world.get_resource_mut::<GameState>() else {
        return;
    };
    let Some(mut workers) = world.get_resource_mut::<PlanetWorkerCoord>() else {
        return;
    };

    let mut uploaded = 0usize;
    loop {
        if start.elapsed() >= UPLOAD_BUDGET {
            break;
        }

        let chunk = {
            let rx = workers.rx.lock().unwrap();
            match rx.try_recv() {
                Ok(chunk) => chunk,
                Err(_) => break,
            }
        };

        game_state.in_flight.remove(&chunk.key);

        if chunk.vertices.is_empty() {
            game_state.current_meshes.remove(&chunk.key);
            game_state.mesh_neighbor_signatures.remove(&chunk.key);
            game_state.empty_chunks.insert(chunk.key);
            game_state
                .empty_neighbor_signatures
                .insert(chunk.key, chunk.neighbor_signature);
            continue;
        }

        //let collider_mesh = cook_terrain_collider_mesh(&chunk.vertices, &chunk.indices);
        let vertex_bytes: Vec<u8> = bytemuck::cast_slice(&chunk.vertices).to_vec();
        let render_data = renderer.renderer_api.create_render_data(
            &vertex_bytes,
            &chunk.indices,
            game_state.solid_material.clone(),
            &PipelineHandle(0),
        );
        game_state.current_meshes.insert(chunk.key, render_data);
        game_state
            .mesh_neighbor_signatures
            .insert(chunk.key, chunk.neighbor_signature);
        game_state.empty_chunks.remove(&chunk.key);
        game_state.empty_neighbor_signatures.remove(&chunk.key);
        //if let Some((vertices, indices)) = collider_mesh {
        //    if let Some(mut physics) = world.get_resource_mut::<Physics>() {
        //        if let Some(handle) = physics.add_trimesh_collider(vertices, indices, 0.0, 0.9) {
        //            game_state
        //                .terrain_colliders
        //                .insert(chunk.key, RapierColliderHandle(handle));
        //        }
        //    }
        //}
        uploaded += 1;
    }

    workers.completed += uploaded;
}

fn sync_planet_debug(renderer: &mut engine::renderer::Renderer, world: &World) {
    let Some(game_state) = world.get_resource::<GameState>() else {
        return;
    };
    let Some(debug_pass_node) = renderer.render_graph.get_node_mut::<DebugPassNode>(1) else {
        return;
    };

    debug_pass_node.clear_wire_cubes();
    debug_pass_node.clear_cubes();

    for (center, size, depth) in &game_state.debug_nodes {
        debug_pass_node.add_wire_cube(*center, *size, depth_color(*depth));
        debug_pass_node.add_cube(
            *center + vec3(0.0, size / 2.0, 0.0),
            1.0,
            depth_color(depth + 1),
        );
    }
}

fn sync_physics_debug(renderer: &mut engine::renderer::Renderer, world: &World) {
    let Some(physics) = world.get_resource::<Physics>() else {
        return;
    };
    let Some(debug_pass_node) = renderer.render_graph.get_node_mut::<DebugPassNode>(1) else {
        return;
    };

    debug_pass_node.clear_spheres();
    for (_, body) in physics.rigid_body_set.iter() {
        let position = body.translation();
        debug_pass_node.add_sphere(Point3::new(position.x, position.y, position.z));
    }
}

fn sync_planet_geometry(renderer: &mut engine::renderer::Renderer, world: &World) {
    let Some(game_state) = world.get_resource::<GameState>() else {
        return;
    };
    let Some(geometry_node) = renderer.render_graph.get_node_mut::<GeometryPassNode>(0) else {
        return;
    };

    geometry_node.clear_render_data();
    for render_data in game_state.current_meshes.values() {
        geometry_node.add_render_data(render_data.clone());
    }
}

// TODO: This should probably be deprecated as now inputs are passed around with InputState world's resource
// Or maybe not, as it works well for debugging stuff
#[unsafe(no_mangle)]
pub fn handle_key_press(state: &mut engine::State, key_code: KeyCode, pressed: bool) {
    let Some(scene_index) = state.active_scene_index.map(|i| i as usize) else {
        return;
    };
    let Some(scene) = state.scenes.get_mut(scene_index) else {
        return;
    };
    let world = scene.world_mut();

    if let Some(mut camera) = world.get_resource_mut::<GameCamera>() {
        let mut camera_transform = world.get_mut::<TransformComponent>(camera.entity).unwrap();
        camera.controller.handle_key(key_code, pressed);

        if pressed && key_code == KeyCode::PageUp {
            camera_transform.position = cgmath::vec3(0.0, PLANET_SIZE as f32, 0.0);
        }
        if pressed && key_code == KeyCode::PageDown {
            camera_transform.position = cgmath::vec3(0.0, 0.0, 0.0);
        }
    }

    if let Some(mut game_state) = world.get_resource_mut::<GameState>() {
        if pressed && key_code == KeyCode::KeyK {
            game_state.update_octree = !game_state.update_octree;
        }

        if pressed && key_code == KeyCode::KeyL {
            if let Some(mut physics) = world.get_resource_mut::<Physics>() {
                for (_, handle) in game_state.terrain_colliders.drain() {
                    physics.remove_collider(handle.0);
                }
            }
            game_state.previous_leaves.clear();
            game_state.current_leaves.clear();
            game_state.current_meshes.clear();
            game_state.mesh_neighbor_signatures.clear();
            game_state.in_flight.clear();
            game_state.empty_chunks.clear();
            game_state.empty_neighbor_signatures.clear();
            game_state.debug_nodes.clear();
            game_state.octree_job_in_flight = false;
        }

        if pressed && key_code == KeyCode::BracketLeft {
            game_state.debug_depth = (game_state.debug_depth + 1).min(game_state.max_depth);
        }
        if pressed && key_code == KeyCode::BracketRight {
            game_state.debug_depth = game_state.debug_depth.saturating_sub(1);
        }
    }
}

fn cook_terrain_collider_mesh(
    vertices: &[PlanetVertex],
    indices: &[u32],
) -> Option<(Vec<Point3<f32>>, Vec<[u32; 3]>)> {
    if vertices.is_empty() || indices.len() < 3 {
        return None;
    }

    let collider_vertices: Vec<Point3<f32>> = vertices
        .iter()
        .map(|vertex| Point3::new(vertex.position[0], vertex.position[1], vertex.position[2]))
        .collect();
    let vertex_count = collider_vertices.len() as u32;
    let mut collider_indices = Vec::with_capacity(indices.len() / 3);

    for triangle in indices.chunks_exact(3) {
        let [a, b, c] = [triangle[0], triangle[1], triangle[2]];
        if a >= vertex_count || b >= vertex_count || c >= vertex_count {
            continue;
        }

        let pa = collider_vertices[a as usize];
        let pb = collider_vertices[b as usize];
        let pc = collider_vertices[c as usize];
        if !pa.x.is_finite()
            || !pa.y.is_finite()
            || !pa.z.is_finite()
            || !pb.x.is_finite()
            || !pb.y.is_finite()
            || !pb.z.is_finite()
            || !pc.x.is_finite()
            || !pc.y.is_finite()
            || !pc.z.is_finite()
        {
            continue;
        }

        let ab = pb - pa;
        let ac = pc - pa;
        if ab.cross(ac).magnitude2() <= f32::EPSILON {
            continue;
        }

        collider_indices.push([a, b, c]);
    }

    if collider_indices.is_empty() {
        return None;
    }

    Some((collider_vertices, collider_indices))
}

#[unsafe(no_mangle)]
pub fn handle_mouse_button(state: &mut engine::State, button: engine::MouseButton, pressed: bool) {
    let Some(scene_index) = state.active_scene_index.map(|i| i as usize) else {
        return;
    };
    let Some(scene) = state.scenes.get_mut(scene_index) else {
        return;
    };

    if button == engine::MouseButton::Right {
        if let Some(mut camera) = scene.world_mut().get_resource_mut::<GameCamera>() {
            camera.controller.handle_mouse_click(pressed);
        }
    }
}

#[unsafe(no_mangle)]
pub fn handle_mouse_motion(state: &mut engine::State, dx: f64, dy: f64) {
    let Some(scene_index) = state.active_scene_index.map(|i| i as usize) else {
        return;
    };
    let Some(scene) = state.scenes.get_mut(scene_index) else {
        return;
    };

    if let Some(mut camera) = scene.world_mut().get_resource_mut::<GameCamera>() {
        camera.controller.handle_mouse(dx as f32, dy as f32);
    }
}

#[unsafe(no_mangle)]
pub fn handle_mouse_scroll(state: &mut engine::State, delta: engine::MouseScrollDelta) {
    let Some(scene_index) = state.active_scene_index.map(|i| i as usize) else {
        return;
    };
    let Some(scene) = state.scenes.get_mut(scene_index) else {
        return;
    };

    if let Some(mut camera) = scene.world_mut().get_resource_mut::<GameCamera>() {
        camera.controller.handle_mouse_scroll(delta);
    }
}

#[unsafe(no_mangle)]
pub fn handle_resize(state: &mut engine::State, width: u32, height: u32) {
    if width == 0 || height == 0 {
        return;
    }

    let Some(scene_index) = state.active_scene_index.map(|i| i as usize) else {
        return;
    };
    let Some(scene) = state.scenes.get_mut(scene_index) else {
        return;
    };

    if let Some(mut camera) = scene.world_mut().get_resource_mut::<GameCamera>() {
        camera.camera.aspect = width as f32 / height as f32;
        let camera_copy = engine::camera::Camera {
            position: camera.camera.position,
            orientation: camera.camera.orientation,
            aspect: camera.camera.aspect,
            fovy: camera.camera.fovy,
            znear: camera.camera.znear,
            zfar: camera.camera.zfar,
        };
        camera.uniform.update_view_proj(&camera_copy);
    }
}

trait PlanetExt {
    #[allow(dead_code)]
    fn generate_planet(state: &mut engine::State, camera_pos: &cgmath::Point3<f32>) -> Self;
    #[allow(dead_code)]
    fn load_meshes(
        &mut self,
        state: &mut engine::State,
        solid_material: &Material,
        line_material: &Material,
    );
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
    fn generate_planet(state: &mut engine::State, camera_pos: &cgmath::Point3<f32>) -> Self {
        let size: usize = PLANET_SIZE;
        let _debug_pass_node: &mut DebugPassNode = state
            .global_resources
            .renderer
            .render_graph
            .get_node_mut::<DebugPassNode>(1)
            .unwrap();

        println!(
            "Amount of nodes to cover entire planet: {:?}",
            u128::from((PLANET_SIZE / CHUNK_SIZE) as u64).pow(3)
        );

        let octree = Planet::create_octree(size as u32 / 2, &camera_pos);
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
        let _debug_pass_node: &mut DebugPassNode = state
            .global_resources
            .renderer
            .render_graph
            .get_node_mut::<DebugPassNode>(1)
            .unwrap();

        let mut octree_nodes = Vec::new();
        Planet::collect_leaf_nodes(&self.octree_root, 0, &mut octree_nodes);
        println!("Leaf nodes: {}", octree_nodes.len());

        let mut meshes_count = 0;

        for (center, node_size, _node_depth) in &octree_nodes {
            let resolution = node_size / CHUNK_SIZE as f32;
            let min_corner = Point3::new(
                center.x - node_size * 0.5,
                center.y - node_size * 0.5,
                center.z - node_size * 0.5,
            );
            let (positions, indices) = Planet::dual_contour_grid(
                &generate_grid_from_min(34, 34, 34, resolution, min_corner.to_vec()),
                min_corner,
                resolution,
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

#[inline(always)]
fn hash3(p: Vector3<f32>) -> f32 {
    let ix = (p.x.floor() as i32).wrapping_mul(1619);
    let iy = (p.y.floor() as i32).wrapping_mul(31337);
    let iz = (p.z.floor() as i32).wrapping_mul(6271);
    let n = ix.wrapping_add(iy).wrapping_add(iz);
    let n = n.wrapping_mul(n.wrapping_mul(n).wrapping_mul(60493).wrapping_add(19990303));
    (n as u32 as f32) / (u32::MAX as f32)
}

#[inline(always)]
fn hash3i(x: i32, y: i32, z: i32) -> f32 {
    let n = x
        .wrapping_mul(1619)
        .wrapping_add(y.wrapping_mul(31337))
        .wrapping_add(z.wrapping_mul(6271));

    let n = n.wrapping_mul(n.wrapping_mul(n).wrapping_mul(60493).wrapping_add(19990303));

    (n as u32 as f32) * (1.0 / u32::MAX as f32)
}

#[inline(always)]
fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

#[inline(always)]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + t * (b - a)
}

#[inline(always)]
fn smooth_noise(p: Vector3<f32>) -> f32 {
    let ix = p.x.floor() as i32;
    let iy = p.y.floor() as i32;
    let iz = p.z.floor() as i32;

    let fx = p.x - ix as f32;
    let fy = p.y - iy as f32;
    let fz = p.z - iz as f32;

    let ux = smoothstep(fx);
    let uy = smoothstep(fy);
    let uz = smoothstep(fz);

    let v000 = hash3i(ix, iy, iz);
    let v100 = hash3i(ix + 1, iy, iz);
    let v010 = hash3i(ix, iy + 1, iz);
    let v110 = hash3i(ix + 1, iy + 1, iz);
    let v001 = hash3i(ix, iy, iz + 1);
    let v101 = hash3i(ix + 1, iy, iz + 1);
    let v011 = hash3i(ix, iy + 1, iz + 1);
    let v111 = hash3i(ix + 1, iy + 1, iz + 1);

    let x00 = lerp(v000, v100, ux);
    let x10 = lerp(v010, v110, ux);
    let x01 = lerp(v001, v101, ux);
    let x11 = lerp(v011, v111, ux);

    let y0 = lerp(x00, x10, uy);
    let y1 = lerp(x01, x11, uy);

    lerp(y0, y1, uz)
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

//fn cave_sdf(p: Vector3<f32>) -> f32 {
//    let scale = 0.04;
//    let q = p * scale;
//    let warp = vec3(
//        fbm(q + vec3(1.7, 9.2, 3.4), 3),
//        fbm(q + vec3(8.3, 2.8, 5.1), 3),
//        fbm(q + vec3(4.1, 6.7, 1.9), 3),
//    ) * 2.0
//        - vec3(1.0, 1.0, 1.0);
//    let warped = q + warp * 0.6;
//    let tunnel_r = fbm(warped, 4);
//    let cave_dist = (tunnel_r - 0.5).abs() - 0.08;
//    cave_dist * (1.0 / scale)
//}

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

    // let depth_below_surface = -terrain;
    // let fade_zone = planet_r * 0.1;
    // let cave_blend = (depth_below_surface / fade_zone).clamp(0.0, 1.0);
    // if cave_blend > 0.0 {
    //     let cave = cave_sdf(p);
    //     let carved = terrain.max(-cave);
    //     terrain + (carved - terrain) * cave_blend
    // } else {
    //     terrain
    // }
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
    let _is_behind_horizon = is_behind_horizon(
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

#[allow(dead_code)]
fn rebuild_octree_debug(state: &mut engine::State, camera_pos: &cgmath::Point3<f32>) {
    let size = PLANET_SIZE;
    let depth = OCTREE_DEBUG_DEPTH.load(Ordering::Relaxed);

    let debug_pass_node: &mut DebugPassNode = state
        .global_resources
        .renderer
        .render_graph
        .get_node_mut::<DebugPassNode>(1)
        .unwrap();

    debug_pass_node.clear_wire_cubes();
    debug_pass_node.clear_cubes();

    let octree = Planet::create_octree(size as u32 / 2, &camera_pos);
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
