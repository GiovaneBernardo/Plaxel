use engine::MouseButton;
use engine::core::components::core::{CameraComponent, TransformComponent};
use engine::core::input::{InputState, KeyCode};
use engine::ecs::entity::Entity;
use engine::ecs::system::SystemContext;
use engine::ecs::world::World;

use engine::assets::material::Material;
use engine::core::components::physics::RapierColliderHandle;
use engine::core::physics::physics::Physics;
use engine::math::Vec3;
use engine::math::{Quat, vec3};
use engine::model::Vertex;
use engine::renderer::CullMode;
use engine::renderer::Topology;
use engine::renderer::{
    BindGroupDescriptor, BindGroupEntry, BindGroupHandle, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, BufferDescriptor, BufferHandle, BufferUsages, CameraData,
    FrameBindings, PipelineHandle, RenderObjectId, ShaderStages, TextureDescriptor,
    TextureDimension, TextureFormat, TextureSize, TextureUsages,
};
use engine::renderer::{DebugPassNode, GeometryPassNode};
use game_types::octree::NodeKey;
use game_types::planet::PlanetInstance;
use game_types::planet::{GpuPlanetTerrainMaterial, Planet, PlanetVertex};
pub use game_types::render_graph;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use web_time::{Duration, Instant};

pub mod octree;
pub mod sdf;
mod systems;

use game_types::game_mode::{GameMode, GameModeState};
use systems::{InputMap, player_interaction_system};

use crate::octree::depth_color;
use crate::sdf::EarthHeightmap;
use crate::systems::planets::planet_debug;

struct GameState {
    previous_leaves: HashMap<NodeKey, ChunkInfo>,
    current_leaves: HashMap<NodeKey, ChunkInfo>,
    planets_meshes: HashMap<NodeKey, RenderObjectId>,
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
    solid_pipeline: PipelineHandle,
    solid_shadow_pipeline: PipelineHandle,
    terrain_materials_buffer: BufferHandle,
    terrain_materials_bind_group: BindGroupHandle,
    update_octree: bool,
    terrain_physics_enabled: bool,
    debug_nodes: Vec<(Vec3, f32, u32)>,
    debug_depth: u32,
    max_depth: u32,
    octree_job_in_flight: bool,
    last_requested_camera_pos: Vec3,
    terrain_brush_radius: f32,
}

struct GameCamera {
    entity: Entity,
    camera: engine::camera::Camera,
    controller: engine::camera::CameraController,
    uniform: engine::camera::CameraUniform,
    velocity_sample_pos: Vec3,
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
const CHUNK_SIZE: usize = 32;
const BRICK_LOD_RADII: [f32; 7] = [160.0, 448.0, 1024.0, 2048.0, 4096.0, 8192.0, f32::MAX];
const MAX_DEBUG_BRICKS: usize = 512;
const BRICK_REBUILD_DISTANCE: f32 = 64.0;
const MAX_CHUNK_WORKER_SPAWNS_PER_FRAME: usize = 12;
const TERRAIN_PHYSICS_RADIUS: f32 = 384.0;
const MAX_TERRAIN_PHYSICS_BRICK_SIZE: f32 = 64.0;
const COARSE_PLANET_BRICK_LEVEL: u8 = 6;
const GRASS_TERRAIN_TEXTURE_INDEX: u32 = 510;
const ROCK_TERRAIN_TEXTURE_INDEX: u32 = 511;

fn load_terrain_diffuse_texture(
    renderer: &mut engine::renderer::Renderer,
    relative_path: &str,
    label: &str,
    texture_index: u32,
) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../res/terrain_textures")
        .join(relative_path);
    renderer.renderer_api.load_texture(
        &path.to_string_lossy().into_owned(),
        &TextureDescriptor {
            label: label.to_string(),
            format: TextureFormat::Rgba8Srgb,
            size: TextureSize::Custom {
                width: 1,
                height: 1,
            },
            dimension: TextureDimension::D2,
            usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
            mip_levels: 1,
            sample_count: 1,
        },
        Some(texture_index),
    );
}

#[unsafe(no_mangle)]
pub fn initialize_game_state(state: &mut engine::State) {
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
    if camera.position.length() > PLANET_SIZE as f32 {
        camera.position = engine::math::vec3(0.0, PLANET_SIZE as f32, 0.0);
    }

    let mut uniform = engine::camera::CameraUniform::new();
    uniform.update_view_proj(&camera);
    let initial_camera_pos = camera.position;
    state
        .global_resources
        .renderer
        .render_resources
        .insert(CameraData::from_camera(&camera, uniform));

    let mut solid_material = Material::new("shaders/planet_terrain.wgsl".to_string())
        .with_vertex_layouts(vec![PlanetVertex::layout(), PlanetInstance::layout()])
        .with_cull(CullMode::None);
    solid_material.configure_pass(engine::renderer::material_passes::SHADOW, |pass| {
        pass.vertex_entry = "vs_shadow".into();
        pass.fragment_entry = None;
    });

    let line_material = Material::new("shaders/planet_terrain2.wgsl".to_string())
        .with_vertex_layouts(vec![PlanetVertex::layout(), PlanetInstance::layout()])
        .with_topology(Topology::LineList)
        .with_cull(CullMode::None);

    let camera_layout = state
        .global_resources
        .renderer
        .render_graph
        .get_node_mut::<GeometryPassNode>(engine::renderer::ids::graph_passes::GEOMETRY)
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
    let shadow_bindings = *state
        .global_resources
        .renderer
        .render_resources
        .get_labeled::<engine::renderer::ShadowBindings>("shadow_bindings")
        .expect("ShadowPassNode must be compiled before creating planet pipelines");

    load_terrain_diffuse_texture(
        &mut state.global_resources.renderer,
        "Grass001_2K-JPG_Color.jpg",
        "terrain_grass_diffuse",
        GRASS_TERRAIN_TEXTURE_INDEX,
    );
    load_terrain_diffuse_texture(
        &mut state.global_resources.renderer,
        "Rock061_2K-JPG_Color.jpg",
        "terrain_rock_diffuse",
        ROCK_TERRAIN_TEXTURE_INDEX,
    );

    // PlanetVertex material IDs address this palette directly: grass = 0, rock = 1.
    let terrain_materials = [
        GpuPlanetTerrainMaterial {
            diffuse_texture_index: GRASS_TERRAIN_TEXTURE_INDEX,
            normal_texture_index: 0,
            displacement_texture_index: 0,
            roughness_texture_index: 0,
            texture_scale: 1.0,
            displacement_scale: 0.0,
            roughness_factor: 0.9,
            flags: 0,
        },
        GpuPlanetTerrainMaterial {
            diffuse_texture_index: ROCK_TERRAIN_TEXTURE_INDEX,
            normal_texture_index: 0,
            displacement_texture_index: 0,
            roughness_texture_index: 0,
            texture_scale: 1.0,
            displacement_scale: 0.0,
            roughness_factor: 0.75,
            flags: 0,
        },
    ];
    let terrain_materials_layout = state
        .global_resources
        .renderer
        .renderer_api
        .create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: "planet_terrain_materials_layout".to_string(),
            entries: vec![BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::Fragment,
                entry_type: BindingType::StorageBuffer { read_only: true },
                count: None,
            }],
        });
    let terrain_materials_buffer =
        state
            .global_resources
            .renderer
            .renderer_api
            .create_buffer(&BufferDescriptor {
                label: "planet_terrain_materials".to_string(),
                size: std::mem::size_of_val(&terrain_materials) as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            });
    state.global_resources.renderer.renderer_api.write_buffer(
        terrain_materials_buffer,
        bytemuck::cast_slice(&terrain_materials),
    );
    let terrain_materials_bind_group = state
        .global_resources
        .renderer
        .renderer_api
        .create_bind_group(&BindGroupDescriptor {
            label: "planet_terrain_materials_bind_group".to_string(),
            layout: terrain_materials_layout,
            entries: vec![(0, BindGroupEntry::Buffer(terrain_materials_buffer))],
        });

    let geometry_target = {
        let renderer = &state.global_resources.renderer;
        let descriptor = GeometryPassNode::pass_descriptor();
        renderer
            .renderer_api
            .target_info_for_pass(&descriptor, &renderer.render_graph.resources)
    };

    let solid_pipeline = state
        .global_resources
        .renderer
        .renderer_api
        .create_pipeline(
            &solid_material,
            engine::renderer::ids::material_passes::FORWARD_OPAQUE,
            &[
                camera_layout,
                textures_layout,
                terrain_materials_layout,
                shadow_bindings.sampling_layout,
            ],
            &geometry_target,
        );
    let shadow_target = {
        let renderer = &state.global_resources.renderer;
        let descriptor = engine::renderer::ShadowPassNode::pass_descriptor();
        renderer
            .renderer_api
            .target_info_for_pass(&descriptor, &renderer.render_graph.resources)
    };
    let solid_shadow_pipeline = state
        .global_resources
        .renderer
        .renderer_api
        .create_pipeline(
            &solid_material,
            engine::renderer::material_passes::SHADOW,
            &[
                shadow_bindings.view_layout,
                textures_layout,
                terrain_materials_layout,
            ],
            &shadow_target,
        );
    state
        .global_resources
        .renderer
        .renderer_api
        .create_pipeline(
            &line_material,
            engine::renderer::ids::material_passes::FORWARD_OPAQUE,
            &[camera_layout, textures_layout],
            &geometry_target,
        );

    let scene = state.active_scene_mut().unwrap();
    let world = scene.world_mut();

    world.insert_resource(GameModeState {
        mode: GameMode::Walking,
    });
    world.insert_resource(InputMap::default());
    //load_earth_heightmap_resource(world);

    let velocity_sample_pos = camera.position;
    let velocity_sample_time = Instant::now();

    let camera_entity = world.spawn();
    world.insert(
        camera_entity,
        TransformComponent {
            position: engine::math::vec3(0.0, 8573.0, 0.0),
            rotation: Quat::IDENTITY,
            scale: engine::math::vec3(1.0, 1.0, 1.0),
            velocity: engine::math::vec3(0.0, 0.0, 0.0),
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
        planets_meshes: HashMap::new(),
        mesh_neighbor_signatures: HashMap::new(),
        terrain_colliders: HashMap::new(),
        in_flight: HashSet::new(),
        empty_chunks: HashSet::new(),
        empty_neighbor_signatures: HashMap::new(),
        solid_material,
        solid_pipeline,
        solid_shadow_pipeline,
        terrain_materials_buffer,
        terrain_materials_bind_group,
        update_octree: true,
        terrain_physics_enabled: true,
        debug_nodes: Vec::new(),
        debug_depth: 0,
        max_depth: 0,
        octree_job_in_flight: false,
        last_requested_camera_pos: initial_camera_pos,
        terrain_brush_radius: 10.0,
    });
}

#[unsafe(no_mangle)]
pub fn register_systems(state: &mut engine::State) {
    initialize_game_state(state);
    register_static_schedule_systems(state);
}

fn register_static_schedule_systems(state: &mut engine::State) {
    let Some(scene) = state.active_scene_mut() else {
        return;
    };

    let init_schedule_mut = scene.init_schedule_mut();
    init_schedule_mut.add_system(hot_planet_system_init);
    init_schedule_mut.add_system(systems::planets::universe_system::universe_system_init);

    let update_schedule_mut = scene.update_schedule_mut();
    update_schedule_mut.add_system(hot_planet_system_update);
    update_schedule_mut.add_system(Physics::create_missing_rapier_bodies_system);
    update_schedule_mut.add_static_system(hot_player_interaction_system);
    update_schedule_mut.add_system(hot_camera_update_system);
    update_schedule_mut.add_system(engine::core::systems::systems::engine_input_system);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn hot_planet_system_init(
    ctx: &mut engine::ecs::system::SystemContext,
    commands: &mut engine::ecs::commands::Commands,
) {
    systems::planets::planet_system_init(ctx, commands);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn hot_planet_system_update(
    ctx: &mut engine::ecs::system::SystemContext,
    commands: &mut engine::ecs::commands::Commands,
) {
    systems::planets::planet_system_update(ctx, commands);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn hot_player_interaction_system(
    ctx: &mut engine::ecs::system::SystemContext,
    commands: &mut engine::ecs::commands::Commands,
) {
    player_interaction_system(ctx, commands);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn hot_camera_update_system(
    ctx: &mut engine::ecs::system::SystemContext,
    commands: &mut engine::ecs::commands::Commands,
) {
    camera_update_system(ctx, commands);
}

#[unsafe(no_mangle)]
pub fn render() {}

#[unsafe(no_mangle)]
pub fn update(state: &mut engine::State) {
    let mut update = subsecond::HotFn::current(update_impl);
    update.call((state,));
}

fn update_impl(state: &mut engine::State) {
    let Some(scene_index) = state.active_scene_index.map(|i| i as usize) else {
        return;
    };

    // avoid simultaneous mutable borrows of state.global_resources
    let scenes = &mut state.scenes;
    let Some(scene) = scenes.get_mut(scene_index) else {
        return;
    };

    // borrow renderer after using scenes to avoid overlapping mutable borrows
    let renderer = &mut state.global_resources.renderer;
    sync_camera_to_renderer(renderer, scene.world());
    sync_planet_debug(renderer, scene.world());
    sync_planet_octree_debug(renderer, scene.world());
    sync_physics_debug(renderer, scene.world());
    state.frame_index += 1;
}

fn camera_update_system(ctx: &mut SystemContext, _commands: &mut engine::ecs::commands::Commands) {
    let world = &mut ctx.world;
    let camera_input = camera_input_from_world(world);
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
    apply_camera_input(&mut controller, camera_input);
    controller.update_camera(&mut camera.camera);
    camera.camera.position = engine::math::vec3(
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

#[derive(Clone, Copy, Default)]
struct CameraInput {
    forward: bool,
    backward: bool,
    left: bool,
    right: bool,
    up: bool,
    down: bool,
    shift: bool,
    roll_left: bool,
    roll_right: bool,
    right_mouse: bool,
    mouse_delta: (f32, f32),
    scroll: f32,
}

fn camera_input_from_world(world: &World) -> CameraInput {
    let Some(input) = world.get_resource::<InputState>() else {
        return CameraInput::default();
    };

    CameraInput {
        forward: input.pressed.contains(&KeyCode::KeyW)
            || input.pressed.contains(&KeyCode::ArrowUp),
        backward: input.pressed.contains(&KeyCode::KeyS)
            || input.pressed.contains(&KeyCode::ArrowDown),
        left: input.pressed.contains(&KeyCode::KeyA) || input.pressed.contains(&KeyCode::ArrowLeft),
        right: input.pressed.contains(&KeyCode::KeyD)
            || input.pressed.contains(&KeyCode::ArrowRight),
        up: input.pressed.contains(&KeyCode::Space) || input.pressed.contains(&KeyCode::PageUp),
        down: input.pressed.contains(&KeyCode::KeyC) || input.pressed.contains(&KeyCode::PageDown),
        shift: input.pressed.contains(&KeyCode::ShiftLeft),
        roll_left: input.pressed.contains(&KeyCode::KeyQ),
        roll_right: input.pressed.contains(&KeyCode::KeyE),
        right_mouse: input.mouse_pressed.contains(&MouseButton::Right),
        mouse_delta: input.mouse_delta,
        scroll: input.scroll,
    }
}

fn apply_camera_input(controller: &mut engine::camera::CameraController, input: CameraInput) {
    controller.handle_key(KeyCode::KeyW, input.forward);
    controller.handle_key(KeyCode::KeyS, input.backward);
    controller.handle_key(KeyCode::KeyA, input.left);
    controller.handle_key(KeyCode::KeyD, input.right);
    controller.handle_key(KeyCode::Space, input.up);
    controller.handle_key(KeyCode::KeyC, input.down);
    controller.handle_key(KeyCode::ShiftLeft, input.shift);
    controller.handle_key(KeyCode::KeyQ, input.roll_left);
    controller.handle_key(KeyCode::KeyE, input.roll_right);
    controller.handle_mouse_click(input.right_mouse);

    if input.mouse_delta.0 != 0.0 || input.mouse_delta.1 != 0.0 {
        controller.handle_mouse(input.mouse_delta.0, input.mouse_delta.1);
    }

    if input.scroll != 0.0 {
        controller.handle_mouse_scroll(engine::MouseScrollDelta::LineDelta(0.0, input.scroll));
    }
}

fn update_camera_velocity_log(camera: &mut GameCamera, previous_position: Vec3) {
    let frame_distance = (camera.camera.position - previous_position).length();
    camera.velocity_sample_distance += frame_distance;

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

    world.insert_resource(Arc::new(EarthHeightmap {
        width,
        height,
        samples,
        min_height,
        max_height,
    }));
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
    let mut query = engine::ecs::query::Query::<(&Planet,)>::new(world);
    if world
        .get_resource::<GameState>()
        .unwrap()
        .terrain_physics_enabled
    {
        return;
    }
    query.for_each(|_, (planet,)| {
        planet_debug::sync_planet_debug(renderer, world, planet);
    });
}

// TODO: This should probably be deprecated as now inputs are passed around with InputState world's resource
// Or maybe not, as it works well for debugging stuff
#[unsafe(no_mangle)]
pub fn handle_key_press(state: &mut engine::State, key_code: KeyCode, pressed: bool) {
    let mut handler = subsecond::HotFn::current(handle_key_press_impl);
    handler.call((state, key_code, pressed));
}

fn handle_key_press_impl(state: &mut engine::State, key_code: KeyCode, pressed: bool) {
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
            camera_transform.position = engine::math::vec3(0.0, PLANET_SIZE as f32, 0.0);
        }
        if pressed && key_code == KeyCode::PageDown {
            camera_transform.position = engine::math::vec3(0.0, 0.0, 0.0);
        }
    }

    if let Some(mut game_state) = world.get_resource_mut::<GameState>() {
        if pressed && key_code == KeyCode::KeyK {
            game_state.update_octree = !game_state.update_octree;
        }

        if pressed && key_code == KeyCode::KeyP {
            game_state.terrain_physics_enabled = !game_state.terrain_physics_enabled;
            if !game_state.terrain_physics_enabled {
                if let Some(mut physics) = world.get_resource_mut::<Physics>() {
                    for (_, handle) in game_state.terrain_colliders.drain() {
                        physics.remove_collider(handle.0);
                    }
                }
            }
        }

        if pressed && key_code == KeyCode::KeyL {
            if let Some(mut physics) = world.get_resource_mut::<Physics>() {
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

#[unsafe(no_mangle)]
pub fn handle_mouse_button(state: &mut engine::State, button: engine::MouseButton, pressed: bool) {
    let mut handler = subsecond::HotFn::current(handle_mouse_button_impl);
    handler.call((state, button, pressed));
}

fn handle_mouse_button_impl(state: &mut engine::State, button: engine::MouseButton, pressed: bool) {
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
    let mut handler = subsecond::HotFn::current(handle_mouse_motion_impl);
    handler.call((state, dx, dy));
}

fn handle_mouse_motion_impl(state: &mut engine::State, dx: f64, dy: f64) {
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
    let mut handler = subsecond::HotFn::current(handle_mouse_scroll_impl);
    handler.call((state, delta));
}

fn handle_mouse_scroll_impl(state: &mut engine::State, delta: engine::MouseScrollDelta) {
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
    let mut handler = subsecond::HotFn::current(handle_resize_impl);
    handler.call((state, width, height));
}

fn handle_resize_impl(state: &mut engine::State, width: u32, height: u32) {
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
