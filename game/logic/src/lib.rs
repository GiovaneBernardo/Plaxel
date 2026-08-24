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
pub use engine::prelude::*;

use game_types::game_mode::{GameMode, GameModeState};
use systems::{InputMap, player_interaction_system, preload_build_block_assets};

use crate::octree::depth_color;
use crate::sdf::EarthHeightmap;
use crate::systems::universe::planet_debug;
use crate::systems::universe::plugin::UniversePlugin;

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
        .add_system(CoreSchedule::Startup, preload_build_block_assets)
        .add_named_legacy_system(
            CoreSchedule::Startup,
            "game.planet_init",
            systems::universe::planet_system_init,
        );

        app.add_plugin(UniversePlugin)
            .add_named_legacy_system(
                CoreSchedule::Update,
                "game.planet_update",
                systems::universe::planet_system_update,
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
            .add_system(CoreSchedule::RenderExtract, sync_camera_to_renderer)
            .add_system(CoreSchedule::Update, camera_update_system);
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct NeighborSignature(Vec<NodeKey>);

const PLANET_SIZE: usize = 65536 * 1;
/// Number of dual-contouring cells owned by each chunk along one axis.
const CHUNK_CELL_COUNT: usize = 32;
const MAX_DEBUG_BRICKS: usize = 512;

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

fn camera_update_system(
    mut camera: ResMut<GameCamera>,
    mut cameras: Query<(&CameraComponent, &TransformComponent)>,
) {
    let Some((camera_component, camera_transform)) = cameras.get(camera.entity) else {
        return;
    };

    camera.camera.position = camera.world_position.as_vec3();
    camera.camera.orientation = camera_transform.rotation;
    camera.camera.fovy = camera_component.fov;
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

fn sync_camera_to_renderer(camera: Res<GameCamera>, mut globals: GlobalsMut) {
    globals
        .renderer
        .render_resources
        .insert(CameraData::from_camera(&camera.camera, camera.uniform));
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
                systems::universe::set_grid_build_debug_enabled(game_state.debug_grid_builds_only);
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
