pub extern crate bevy_reflect as plaxel_reflect;

use engine::core::components::core::{CameraComponent, TransformComponent};
use engine::core::input::KeyCode;
use engine::core::window::{
    KeyboardInput, MouseButtonInput, MouseMotion, MouseWheel, WindowResized,
};
use engine::ecs::entity::Entity;
use engine::ecs::event::EventReader;
use engine::ecs::plugin::Plugin;
use engine::ecs::query::Query;
use engine::ecs::resource::ResMut;
use engine::ecs::schedule::CoreSchedule;
use engine::ecs::system::SystemContext;
use engine::ecs::world::World;

use engine::core::components::physics::RapierColliderHandle;
use engine::core::physics::physics::Physics;
use engine::game_info;
use engine::math::Vec3;
use engine::math::{Quat, vec3};
use engine::renderer::CameraData;
use engine::renderer::DebugPassNode;
use game_types::octree::{NodeKey, PlanetLodSettings};
use game_types::planet::{Planet, PlanetVertex};
pub use game_types::render_graph;
use std::collections::{HashMap, HashSet};
use web_time::{Duration, Instant};

pub mod octree;
pub mod render;
pub mod sdf;
mod systems;

use game_types::game_mode::{GameMode, GameModeState};
use systems::{InputMap, player_interaction_system, preload_build_block_assets};

use crate::octree::depth_color;
use crate::sdf::EarthHeightmap;
use crate::systems::planets::planet_debug;

#[derive(plaxel_reflect::Reflect)]
#[reflect(from_reflect = false)]
struct GameState {
    /// When enabled, newly spawned planets use TerrainFieldGraph::default()
    /// immediately and never enqueue meshes from the legacy generator.
    start_with_earth_like_terrain: bool,
    #[reflect(ignore)]
    previous_leaves: HashMap<NodeKey, ChunkInfo>,
    #[reflect(ignore)]
    current_leaves: HashMap<NodeKey, ChunkInfo>,
    #[reflect(ignore)]
    mesh_neighbor_signatures: HashMap<NodeKey, NeighborSignature>,
    #[reflect(ignore)]
    terrain_colliders: HashMap<NodeKey, RapierColliderHandle>,
    #[reflect(ignore)]
    in_flight: HashSet<NodeKey>,
    // Keys whose worker finished but produced zero vertices. Remembered so
    // the scheduler never re-spawns a worker for them on subsequent frames.
    // Pruned by retain() when the key leaves the current octree, so a fresh
    // NodeKey (different position or size) always gets a clean attempt.
    #[reflect(ignore)]
    empty_chunks: HashSet<NodeKey>,
    #[reflect(ignore)]
    empty_neighbor_signatures: HashMap<NodeKey, NeighborSignature>,
    update_octree: bool,
    terrain_physics_enabled: bool,
    debug_grid_builds_only: bool,
    debug_nodes: Vec<(Vec3, f32, u32)>,
    debug_depth: u32,
    max_depth: u32,
    octree_job_in_flight: bool,
    terrain_brush_radius: f32,
}

pub struct GamePlugin;
impl Plugin for GamePlugin {
    fn build(&self, app: &mut engine::App) {
        app.add_named_legacy_system(
            CoreSchedule::Startup,
            "game.initialize_state",
            initialize_game_state,
        )
        .add_named_legacy_system(
            CoreSchedule::Startup,
            "game.terrain_producer_init",
            render::producers::planet_terrain_producer::planet_terrain_producer_init,
        )
        .add_named_legacy_system(
            CoreSchedule::Startup,
            "game.planet_init",
            systems::planets::planet_system_init,
        )
        .add_named_legacy_system(
            CoreSchedule::Startup,
            "game.universe_init",
            systems::planets::universe_system::universe_system_init,
        )
        .add_system(CoreSchedule::Startup, preload_build_block_assets)
        .add_system(CoreSchedule::Startup, report_schedules_started)
        .add_named_legacy_system(
            CoreSchedule::Update,
            "game.planet_update",
            systems::planets::planet_system_update,
        )
        .add_named_legacy_system(
            CoreSchedule::Update,
            "game.create_missing_rapier_bodies",
            Physics::create_missing_rapier_bodies_system,
        )
        .add_named_legacy_system(
            CoreSchedule::Update,
            "game.player_interaction",
            player_interaction_system,
        )
        .add_system(CoreSchedule::Update, handle_key_press)
        .add_system(CoreSchedule::Update, handle_mouse_button)
        .add_system(CoreSchedule::Update, handle_mouse_motion)
        .add_system(CoreSchedule::Update, handle_mouse_scroll)
        .add_system(CoreSchedule::Update, handle_resize)
        .add_named_legacy_system(
            CoreSchedule::Update,
            "game.camera_update",
            camera_update_system,
        )
        .add_named_legacy_system(
            CoreSchedule::Update,
            "game.terrain_producer_update",
            render::producers::planet_terrain_producer::planet_terrain_producer_update,
        )
        .add_system(
            CoreSchedule::Update,
            engine::core::systems::systems::engine_input_system,
        )
        .add_system(CoreSchedule::Update, report_window_resize)
        .add_named_legacy_system(
            CoreSchedule::RenderExtract,
            "game.extract_render_state",
            extract_game_render_state,
        );
    }
}

fn report_schedules_started() {
    game_info!("Startup and frame schedules are running");
}

/// Small example of consuming a winit event after the App adapter has placed
/// it in the ECS world. Each system gets its own independent reader cursor.
fn report_window_resize(mut events: EventReader<WindowResized>) {
    for event in events.read() {
        game_info!("Window resized to {}x{}", event.width, event.height);
    }
}

fn handle_resize(mut events: EventReader<WindowResized>, camera: Option<ResMut<GameCamera>>) {
    let Some(mut camera) = camera else {
        return;
    };

    for event in events.read() {
        if event.width == 0 || event.height == 0 {
            continue;
        }

        camera.camera.aspect = event.width as f32 / event.height as f32;
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

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GpuTerrainFrame {
    /// Projection multiplied by view rotation. Translation is applied after
    /// integer anchor subtraction in the terrain vertex shader.
    view_projection_rotation: [[f32; 4]; 4],
    camera_anchor_planet: [i32; 3],
    position_unit: f32,
    camera_remainder_planet: [f32; 3],
    _padding: f32,
    planet_world_position: [f32; 3],
    _planet_padding: f32,
}

impl GpuTerrainFrame {
    fn new(
        view_projection_rotation: engine::math::Mat4,
        camera_world_position: engine::math::DVec3,
        planet_world_position: Vec3,
    ) -> Self {
        let camera_position_planet = camera_world_position - planet_world_position.as_dvec3();
        let camera_anchor_planet = camera_position_planet.floor().as_ivec3();
        let camera_remainder = (camera_position_planet - camera_anchor_planet.as_dvec3()).as_vec3();

        Self {
            view_projection_rotation: view_projection_rotation.to_cols_array_2d(),
            camera_anchor_planet: camera_anchor_planet.to_array(),
            position_unit: 1.0,
            camera_remainder_planet: camera_remainder.to_array(),
            _padding: 0.0,
            planet_world_position: planet_world_position.to_array(),
            _planet_padding: 0.0,
        }
    }
}

#[derive(plaxel_reflect::Reflect)]
#[reflect(from_reflect = false)]
struct GameCamera {
    entity: Entity,
    #[reflect(ignore)]
    camera: engine::camera::Camera,
    world_position: engine::math::DVec3,
    #[reflect(ignore)]
    controller: engine::camera::CameraController,
    #[reflect(ignore)]
    uniform: engine::camera::CameraUniform,
    previous_world_position: engine::math::DVec3,
    velocity_sample_pos: Vec3,
    #[reflect(ignore)]
    velocity_sample_time: Instant,
    velocity_sample_distance: f32,
}

#[derive(Clone, Copy)]
struct ChunkInfo {
    center: Vec3,
    size: f32,
}

#[derive(Hash, Eq, PartialEq, Clone, Copy, Debug, PartialOrd, Ord)]
struct BrickCoord {
    level: u8,
    x: i32,
    y: i32,
    z: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NeighborSignature(Vec<NodeKey>);

#[derive(Clone, Copy, Debug)]
struct ChunkNeighbor {
    key: NodeKey,
    center: Vec3,
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

#[derive(Clone, Copy)]
struct ChunkBounds {
    key: NodeKey,
    info: ChunkInfo,
    min: [i32; 3],
    max: [i32; 3],
}

const UPLOAD_BUDGET: Duration = Duration::from_millis(2);

const PLANET_SIZE: usize = 65536 * 1;
/// Number of dual-contouring cells owned by each chunk along one axis.
const CHUNK_CELL_COUNT: usize = 32;
const BRICK_LOD_RADII: [f32; 7] = [160.0, 448.0, 1024.0, 2048.0, 4096.0, 8192.0, f32::MAX];
const MAX_DEBUG_BRICKS: usize = 512;
const BRICK_REBUILD_DISTANCE: f32 = 64.0;
const MAX_CHUNK_WORKER_SPAWNS_PER_FRAME: usize = 12;
const TERRAIN_PHYSICS_RADIUS: f32 = 384.0;
const MAX_TERRAIN_PHYSICS_BRICK_SIZE: f32 = 64.0;
const COARSE_PLANET_BRICK_LEVEL: u8 = 6;

fn initialize_game_state(
    ctx: &mut SystemContext<'_>,
    _commands: &mut engine::ecs::commands::Commands,
) {
    let size = ctx.globals.window.inner_size();
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
    if camera.position.length() > PLANET_SIZE as f32 {
        camera.position = vec3(0.0, PLANET_SIZE as f32, 0.0);
    }

    let mut uniform = engine::camera::CameraUniform::new();
    uniform.update_view_proj(&camera);
    ctx.globals
        .renderer
        .render_resources
        .insert(CameraData::from_camera(&camera, uniform));

    let world = &mut ctx.world;
    world.insert_resource(GameModeState {
        mode: GameMode::Walking,
    });
    world.insert_resource(InputMap::default());
    world.insert_resource(PlanetLodSettings::default());

    let velocity_sample_pos = camera.position;
    let velocity_sample_time = Instant::now();

    let camera_entity = world.spawn();
    world.insert(
        camera_entity,
        TransformComponent {
            position: vec3(0.0, 8573.0, 0.0),
            rotation: Quat::IDENTITY,
            scale: vec3(1.0, 1.0, 1.0),
            velocity: vec3(0.0, 0.0, 0.0),
        },
    );
    world.insert(
        camera_entity,
        CameraComponent {
            speed: 1.0,
            fov: 75.0,
            far_plane: 15000.0,
            near_plane: 0.001,
        },
    );

    world.insert_resource(GameCamera {
        entity: camera_entity,
        world_position: velocity_sample_pos.as_dvec3(),
        camera,
        controller: engine::camera::CameraController::new(0.2),
        uniform,
        previous_world_position: velocity_sample_pos.as_dvec3(),
        velocity_sample_pos,
        velocity_sample_time,
        velocity_sample_distance: 0.0,
    });

    world.insert_resource(GameState {
        start_with_earth_like_terrain: true,
        previous_leaves: HashMap::new(),
        current_leaves: HashMap::new(),
        mesh_neighbor_signatures: HashMap::new(),
        terrain_colliders: HashMap::new(),
        in_flight: HashSet::new(),
        empty_chunks: HashSet::new(),
        empty_neighbor_signatures: HashMap::new(),
        update_octree: true,
        terrain_physics_enabled: true,
        debug_grid_builds_only: false,
        debug_nodes: Vec::new(),
        debug_depth: 0,
        max_depth: 0,
        octree_job_in_flight: false,
        terrain_brush_radius: 10.0,
    });
}

fn extract_game_render_state(
    ctx: &mut SystemContext<'_>,
    _commands: &mut engine::ecs::commands::Commands,
) {
    let renderer = &mut ctx.globals.renderer;
    let world = &*ctx.world;

    sync_camera_to_renderer(renderer, world);
    sync_planet_debug(renderer, world);
    sync_planet_octree_debug(renderer, world);
    sync_physics_debug(renderer, world);
}

// #[unsafe(no_mangle)]
// pub fn initialize_game_state(state: &mut engine::State) {
//     let size = state.window.inner_size();
//     let aspect = size.width as f32 / size.height.max(1) as f32;

//     let mut camera = engine::camera::Camera {
//         position: (0.0, PLANET_SIZE as f32, 2.0).into(),
//         orientation: engine::camera::Camera::look_at(
//             vec3(0.01, -1.0, 0.0).normalize(),
//             vec3(0.0, 0.0, -1.0),
//         ),
//         aspect,
//         fovy: 65.0,
//         znear: 0.1,
//         zfar: 15_000_000.0,
//     };
//     if camera.position.length() > PLANET_SIZE as f32 {
//         camera.position = engine::math::vec3(0.0, PLANET_SIZE as f32, 0.0);
//     }

//     let mut uniform = engine::camera::CameraUniform::new();
//     uniform.update_view_proj(&camera);
//     state
//         .global_resources
//         .renderer
//         .render_resources
//         .insert(CameraData::from_camera(&camera, uniform));

//     let scene = state.active_scene_mut().unwrap();
//     let world = scene.world_mut();

//     world.insert_resource(GameModeState {
//         mode: GameMode::Walking,
//     });
//     world.insert_resource(InputMap::default());
//     world.insert_resource(PlanetLodSettings::default());
//     //load_earth_heightmap_resource(world);

//     let velocity_sample_pos = camera.position;
//     let velocity_sample_time = Instant::now();

//     let camera_entity = world.spawn();
//     world.insert(
//         camera_entity,
//         TransformComponent {
//             position: engine::math::vec3(0.0, 8573.0, 0.0),
//             rotation: Quat::IDENTITY,
//             scale: engine::math::vec3(1.0, 1.0, 1.0),
//             velocity: engine::math::vec3(0.0, 0.0, 0.0),
//         },
//     );
//     world.insert(
//         camera_entity,
//         CameraComponent {
//             speed: 1.0,
//             fov: 75.0,
//             far_plane: 15000.0,
//             near_plane: 0.001,
//         },
//     );

//     world.insert_resource(GameCamera {
//         entity: camera_entity,
//         world_position: velocity_sample_pos.as_dvec3(),
//         camera,
//         controller: engine::camera::CameraController::new(0.2),
//         uniform,
//         previous_world_position: velocity_sample_pos.as_dvec3(),
//         velocity_sample_pos,
//         velocity_sample_time,
//         velocity_sample_distance: 0.0,
//     });

//     world.insert_resource(GameState {
//         start_with_earth_like_terrain: true,
//         previous_leaves: HashMap::new(),
//         current_leaves: HashMap::new(),
//         mesh_neighbor_signatures: HashMap::new(),
//         terrain_colliders: HashMap::new(),
//         in_flight: HashSet::new(),
//         empty_chunks: HashSet::new(),
//         empty_neighbor_signatures: HashMap::new(),
//         update_octree: true,
//         terrain_physics_enabled: true,
//         debug_nodes: Vec::new(),
//         debug_depth: 0,
//         max_depth: 0,
//         octree_job_in_flight: false,
//         terrain_brush_radius: 10.0,
//     });
// }

#[cfg(test)]
mod plugin_tests {
    use super::*;

    #[test]
    fn game_schedules_initialize_before_startup_resources_are_created() {
        let mut app = engine::App::new();
        app.add_plugin(engine::PlaxelDefaultPlugin)
            .add_plugin(GamePlugin);

        app.initialize_schedules();

        assert!(
            app.schedules
                .schedules
                .get(&CoreSchedule::Startup)
                .is_some_and(|schedule| schedule.system_accesses().count() >= 5)
        );
    }
}

// #[unsafe(no_mangle)]
// pub fn register_systems(state: &mut engine::State) {
//     initialize_game_state(state);
//     register_static_schedule_systems(state);
// }

// fn register_static_schedule_systems(state: &mut engine::State) {
//     let Some(scene) = state.active_scene_mut() else {
//         return;
//     };

//     let init_schedule_mut = scene.init_schedule_mut();
//     init_schedule_mut.add_legacy_system(
//         render::producers::planet_terrain_producer::planet_terrain_producer_init,
//     );
//     init_schedule_mut.add_legacy_system(hot_planet_system_init);
//     init_schedule_mut.add_legacy_system(systems::planets::universe_system::universe_system_init);

//     let update_schedule_mut = scene.update_schedule_mut();
//     update_schedule_mut.add_legacy_system(hot_planet_system_update);
//     update_schedule_mut.add_legacy_system(Physics::create_missing_rapier_bodies_system);
//     update_schedule_mut.add_static_legacy_system(hot_player_interaction_system);
//     update_schedule_mut.add_legacy_system(hot_camera_update_system);
//     update_schedule_mut.add_legacy_system(
//         render::producers::planet_terrain_producer::planet_terrain_producer_update,
//     );
//     update_schedule_mut.add_system(engine::core::systems::systems::engine_input_system);
// }

// #[unsafe(no_mangle)]
// #[inline(never)]
// pub fn hot_planet_system_init(
//     ctx: &mut engine::ecs::system::SystemContext,
//     commands: &mut engine::ecs::commands::Commands,
// ) {
//     systems::planets::planet_system_init(ctx, commands);
// }

// #[unsafe(no_mangle)]
// #[inline(never)]
// pub fn hot_planet_system_update(
//     ctx: &mut engine::ecs::system::SystemContext,
//     commands: &mut engine::ecs::commands::Commands,
// ) {
//     systems::planets::planet_system_update(ctx, commands);
// }

// #[unsafe(no_mangle)]
// #[inline(never)]
// pub fn hot_player_interaction_system(
//     ctx: &mut engine::ecs::system::SystemContext,
//     commands: &mut engine::ecs::commands::Commands,
// ) {
//     player_interaction_system(ctx, commands);
// }

// #[unsafe(no_mangle)]
// #[inline(never)]
// pub fn hot_camera_update_system(
//     ctx: &mut engine::ecs::system::SystemContext,
//     commands: &mut engine::ecs::commands::Commands,
// ) {
//     camera_update_system(ctx, commands);
// }

// #[unsafe(no_mangle)]
// pub fn render() {}

// #[unsafe(no_mangle)]
// pub fn update(state: &mut engine::State) {
//     engine::profile_scope!("game.update");
//     let mut update = subsecond::HotFn::current(update_impl);
//     update.call((state,));
// }

// fn update_impl(state: &mut engine::State) {
//     let Some(scene_index) = state.active_scene_index.map(|i| i as usize) else {
//         return;
//     };

//     // avoid simultaneous mutable borrows of state.global_resources
//     let scenes = &mut state.scenes;
//     let Some(scene) = scenes.get_mut(scene_index) else {
//         return;
//     };

//     // borrow renderer after using scenes to avoid overlapping mutable borrows
//     let renderer = &mut state.global_resources.renderer;
//     {
//         engine::profile_scope!("game.update.sync_camera");
//         sync_camera_to_renderer(renderer, scene.world());
//     }
//     {
//         engine::profile_scope!("game.update.sync_planet_debug");
//         sync_planet_debug(renderer, scene.world());
//     }
//     {
//         engine::profile_scope!("game.update.sync_octree_debug");
//         sync_planet_octree_debug(renderer, scene.world());
//     }
//     {
//         engine::profile_scope!("game.update.sync_physics_debug");
//         sync_physics_debug(renderer, scene.world());
//     }
//     state.frame_index += 1;
// }

fn camera_update_system(ctx: &mut SystemContext, _commands: &mut engine::ecs::commands::Commands) {
    let world = &mut ctx.world;
    let Some(mut camera) = world.get_resource_mut::<GameCamera>() else {
        return;
    };

    let camera_entity = camera.entity;
    let camera_transform = world.get::<TransformComponent>(camera_entity).unwrap();
    let camera_component = world.get::<CameraComponent>(camera_entity).unwrap();

    let previous_position = camera.previous_world_position;
    camera.camera.position = camera.world_position.as_vec3();
    camera.camera.orientation = camera_transform.rotation;
    camera.camera.fovy = camera_component.fov;

    update_camera_velocity_log(&mut camera, previous_position);
    camera.previous_world_position = camera.world_position;
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

fn update_camera_velocity_log(camera: &mut GameCamera, previous_position: engine::math::DVec3) {
    let frame_distance = (camera.world_position - previous_position).length();
    camera.velocity_sample_distance += frame_distance as f32;

    let now = Instant::now();
    let elapsed = now
        .duration_since(camera.velocity_sample_time)
        .as_secs_f32();

    if elapsed < 1.0 {
        return;
    }

    camera.velocity_sample_pos = camera.camera.position;
    camera.velocity_sample_time = now;
    camera.velocity_sample_distance = 0.0;
}

fn sync_camera_to_renderer(renderer: &mut engine::renderer::Renderer, world: &World) {
    let Some(camera) = world.get_resource::<GameCamera>() else {
        return;
    };

    renderer
        .render_resources
        .insert(CameraData::from_camera(&camera.camera, camera.uniform));
}

fn load_earth_heightmap_resource(world: &mut World) {
    let path = "res/heightmaps/earth5400x2700.jpg";
    let Ok(image) = image::open(path) else {
        engine::game_warn!("failed to load earth heightmap from {path}");
        return;
    };

    let grayscale = image.to_luma8();
    let width = grayscale.width();
    let height = grayscale.height();
    let samples: Vec<f32> = grayscale
        .pixels()
        .map(|pixel| pixel[0] as f32 / u8::MAX as f32)
        .collect();

    let (min_height, max_height) = samples
        .iter()
        .copied()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), value| {
            (min.min(value), max.max(value))
        });

    world.insert_resource(EarthHeightmap {
        width,
        height,
        samples,
        min_height,
        max_height,
    });
}

fn sync_planet_debug(renderer: &mut engine::renderer::Renderer, world: &World) {
    let Some(game_state) = world.get_resource::<GameState>() else {
        return;
    };
    let Some(debug_pass_node) = renderer
        .render_graph
        .get_node_mut::<DebugPassNode>(engine::renderer::ids::graph_passes::DEBUG)
    else {
        return;
    };

    debug_pass_node.clear_wire_cubes();
    debug_pass_node.clear_cubes();
    if game_state.debug_grid_builds_only {
        return;
    }

    for (center, size, depth) in game_state.debug_nodes.iter().take(MAX_DEBUG_BRICKS) {
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
    let Some(debug_pass_node) = renderer
        .render_graph
        .get_node_mut::<DebugPassNode>(engine::renderer::ids::graph_passes::DEBUG)
    else {
        return;
    };

    debug_pass_node.clear_spheres();
    for (_, body) in physics.rigid_body_set.iter() {
        let position = body.translation();
        debug_pass_node.add_sphere(Vec3::new(position.x, position.y, position.z));
    }
}

fn sync_planet_octree_debug(renderer: &mut engine::renderer::Renderer, world: &World) {
    let Some(game_state) = world.get_resource::<GameState>() else {
        return;
    };
    if game_state.debug_grid_builds_only {
        let nodes = systems::planets::recent_grid_build_debug_nodes();
        let Some(debug_pass_node) = renderer
            .render_graph
            .get_node_mut::<DebugPassNode>(engine::renderer::ids::graph_passes::DEBUG)
        else {
            return;
        };
        for node in nodes.iter().take(MAX_DEBUG_BRICKS) {
            debug_pass_node.add_wire_cube(node.center, node.size, depth_color(node.depth));
            debug_pass_node.add_cube(
                node.center + vec3(0.0, node.size / 2.0, 0.0),
                1.0,
                depth_color(node.depth + 1),
            );
        }
        return;
    }

    let mut query = engine::ecs::query::Query::<(&Planet,)>::new(world);
    if game_state.terrain_physics_enabled {
        return;
    }
    query.for_each(|_, (planet,)| {
        planet_debug::sync_planet_debug(renderer, world, planet);
    });
}

fn cook_terrain_collider_mesh(
    vertices: &[PlanetVertex],
    indices: &[u32],
) -> Option<(Vec<Vec3>, Vec<[u32; 3]>)> {
    if vertices.is_empty() || indices.len() < 3 {
        return None;
    }

    let collider_vertices: Vec<Vec3> = vertices
        .iter()
        .map(|vertex| Vec3::new(vertex.position[0], vertex.position[1], vertex.position[2]))
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
        if ab.cross(ac).length_squared() <= f32::EPSILON {
            continue;
        }

        collider_indices.push([a, b, c]);
    }

    if collider_indices.is_empty() {
        return None;
    }

    Some((collider_vertices, collider_indices))
}

fn handle_key_press(
    mut events: EventReader<KeyboardInput>,
    camera: Option<ResMut<GameCamera>>,
    game_state: Option<ResMut<GameState>>,
    physics: Option<ResMut<Physics>>,
    mut transforms: Query<(&mut TransformComponent,)>,
) {
    let mut camera = camera;
    let mut game_state = game_state;
    let mut physics = physics;

    for event in events.read() {
        if let Some(camera) = camera.as_mut() {
            camera.controller.handle_key(event.key_code, event.pressed);

            if event.pressed && !event.repeat {
                let new_position = match event.key_code {
                    KeyCode::PageUp => Some(vec3(0.0, PLANET_SIZE as f32, 0.0)),
                    KeyCode::PageDown => Some(Vec3::ZERO),
                    _ => None,
                };

                if let Some(new_position) = new_position {
                    let camera_entity = camera.entity;
                    transforms.for_each(|entity, (transform,)| {
                        if entity == camera_entity {
                            transform.position = new_position;
                        }
                    });
                    camera.world_position = new_position.as_dvec3();
                }
            }
        }

        if !event.pressed || event.repeat {
            continue;
        }

        let Some(game_state) = game_state.as_mut() else {
            continue;
        };

        match event.key_code {
            KeyCode::KeyK => {
                game_state.update_octree = !game_state.update_octree;
            }
            KeyCode::KeyP => {
                game_state.terrain_physics_enabled = !game_state.terrain_physics_enabled;
                if !game_state.terrain_physics_enabled {
                    if let Some(physics) = physics.as_mut() {
                        for (_, handle) in game_state.terrain_colliders.drain() {
                            physics.remove_collider(handle.0);
                        }
                    }
                }
            }
            KeyCode::KeyG => {
                game_state.debug_grid_builds_only = !game_state.debug_grid_builds_only;
                systems::planets::set_grid_build_debug_enabled(game_state.debug_grid_builds_only);
                game_info!(
                    "Grid-build octree debug: {}",
                    if game_state.debug_grid_builds_only {
                        "on"
                    } else {
                        "off"
                    }
                );
            }
            KeyCode::KeyL => {
                if let Some(physics) = physics.as_mut() {
                    for (_, handle) in game_state.terrain_colliders.drain() {
                        physics.remove_collider(handle.0);
                    }
                }
                game_state.previous_leaves.clear();
                game_state.current_leaves.clear();
                game_state.mesh_neighbor_signatures.clear();
                game_state.in_flight.clear();
                game_state.empty_chunks.clear();
                game_state.empty_neighbor_signatures.clear();
                game_state.debug_nodes.clear();
                game_state.octree_job_in_flight = false;
            }
            KeyCode::BracketLeft => {
                game_state.debug_depth = (game_state.debug_depth + 1).min(game_state.max_depth);
            }
            KeyCode::BracketRight => {
                game_state.debug_depth = game_state.debug_depth.saturating_sub(1);
            }
            _ => {}
        }
    }
}

fn handle_mouse_button(
    mut events: EventReader<MouseButtonInput>,
    camera: Option<ResMut<GameCamera>>,
) {
    let Some(mut camera) = camera else {
        return;
    };

    for event in events.read() {
        if event.button == engine::core::input::MouseButton::Right {
            camera.controller.handle_mouse_click(event.pressed);
        }
    }
}

fn handle_mouse_motion(mut events: EventReader<MouseMotion>, camera: Option<ResMut<GameCamera>>) {
    let Some(mut camera) = camera else {
        return;
    };

    for event in events.read() {
        camera.controller.handle_mouse(event.delta_x, event.delta_y);
    }
}

fn handle_mouse_scroll(mut events: EventReader<MouseWheel>, camera: Option<ResMut<GameCamera>>) {
    let Some(mut camera) = camera else {
        return;
    };

    for event in events.read() {
        camera.controller.handle_scroll(event.vertical);
    }
}
