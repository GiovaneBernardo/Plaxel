use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use engine::assets::manager::{Assets, Handle, Uuid};
use engine::assets::material::Material;
use engine::assets::server::AssetServer;
use engine::core::components::core::{CameraComponent, TransformComponent};
use engine::core::components::renderer::MeshRendererComponent;
use engine::ecs::entity::Entity;
use engine::ecs::query::Query;
use engine::ecs::resource::Res;
use engine::global_resources::GlobalResources;
use engine::model::{MeshAsset, TransformInstance, Vertex};
use engine::renderer::DefaultMeshes;
use game_types::assembly::Assembly;
use game_types::octree::{DensityRange, FaceNeighbor, OctreeNode, PlanetMeshRequest};
use game_types::planet::{Planet, PlanetTerrainEdits, TerrainBrickKey};
use game_types::terrain::PlanetTerrainConfig;
use rand::Rng;

use engine::math::{Quat, Vec3, vec3};

use engine::core::input::{InputState, KeyCode, MouseButton};
use engine::ecs::commands::{Commands, PhysicalSphereParams};
use engine::ecs::system::{GlobalsMut, SystemContext};
use game_types::game_mode::{GameMode, GameModeState};

use crate::{
    GameCamera, GameState, octree,
    sdf::{TERRAIN_EDIT_CELL_COUNT, TERRAIN_EDIT_SAMPLE_COUNT, resample_terrain_edit_brick},
    systems::{
        planets::submit_requested_mesh_urgent,
        terrain::terrain_sampler::{self, PlanetTerrainSamplerContext},
    },
};

#[allow(dead_code)]
pub enum Action {
    Interact,
    OpenMenu,
    WalkForward,
    WalkLeft,
    WalkRight,
    WalkBackward,
    RollLeft,
    RollRight,
    Jump,
    Crouch,
    Sprint,
}

#[derive(Clone, Copy, plaxel_reflect::Reflect)]
pub enum InputKey {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Space,
    Tab,
    Enter,
    Escape,
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
}

impl From<InputKey> for KeyCode {
    fn from(value: InputKey) -> Self {
        match value {
            InputKey::A => Self::KeyA,
            InputKey::B => Self::KeyB,
            InputKey::C => Self::KeyC,
            InputKey::D => Self::KeyD,
            InputKey::E => Self::KeyE,
            InputKey::F => Self::KeyF,
            InputKey::G => Self::KeyG,
            InputKey::H => Self::KeyH,
            InputKey::I => Self::KeyI,
            InputKey::J => Self::KeyJ,
            InputKey::K => Self::KeyK,
            InputKey::L => Self::KeyL,
            InputKey::M => Self::KeyM,
            InputKey::N => Self::KeyN,
            InputKey::O => Self::KeyO,
            InputKey::P => Self::KeyP,
            InputKey::Q => Self::KeyQ,
            InputKey::R => Self::KeyR,
            InputKey::S => Self::KeyS,
            InputKey::T => Self::KeyT,
            InputKey::U => Self::KeyU,
            InputKey::V => Self::KeyV,
            InputKey::W => Self::KeyW,
            InputKey::X => Self::KeyX,
            InputKey::Y => Self::KeyY,
            InputKey::Z => Self::KeyZ,
            InputKey::Space => Self::Space,
            InputKey::Tab => Self::Tab,
            InputKey::Enter => Self::Enter,
            InputKey::Escape => Self::Escape,
            InputKey::ShiftLeft => Self::ShiftLeft,
            InputKey::ShiftRight => Self::ShiftRight,
            InputKey::ControlLeft => Self::ControlLeft,
            InputKey::ControlRight => Self::ControlRight,
            InputKey::ArrowUp => Self::ArrowUp,
            InputKey::ArrowDown => Self::ArrowDown,
            InputKey::ArrowLeft => Self::ArrowLeft,
            InputKey::ArrowRight => Self::ArrowRight,
        }
    }
}

#[derive(plaxel_reflect::Reflect)]
pub struct InputMap {
    interact: InputKey,
    open_menu: InputKey,
    walk_forward: InputKey,
    walk_left: InputKey,
    walk_right: InputKey,
    walk_backward: InputKey,
    roll_left: InputKey,
    roll_right: InputKey,
    jump: InputKey,
    crouch: InputKey,
    sprint: InputKey,
}

impl Default for InputMap {
    fn default() -> Self {
        Self {
            interact: InputKey::F,
            open_menu: InputKey::Tab,
            walk_forward: InputKey::W,
            walk_left: InputKey::A,
            walk_right: InputKey::D,
            walk_backward: InputKey::S,
            roll_left: InputKey::Q,
            roll_right: InputKey::E,
            jump: InputKey::Space,
            crouch: InputKey::C,
            sprint: InputKey::ShiftLeft,
        }
    }
}

impl InputMap {
    fn key(&self, action: Action) -> KeyCode {
        match action {
            Action::Interact => self.interact,
            Action::OpenMenu => self.open_menu,
            Action::WalkForward => self.walk_forward,
            Action::WalkLeft => self.walk_left,
            Action::WalkRight => self.walk_right,
            Action::WalkBackward => self.walk_backward,
            Action::RollLeft => self.roll_left,
            Action::RollRight => self.roll_right,
            Action::Jump => self.jump,
            Action::Crouch => self.crouch,
            Action::Sprint => self.sprint,
        }
        .into()
    }

    fn just_pressed(&self, input: &InputState, action: Action) -> bool {
        input.just_pressed.contains(&self.key(action))
    }

    fn pressed(&self, input: &InputState, action: Action) -> bool {
        input.pressed.contains(&self.key(action))
    }
}

fn ray_from_mouse_position(
    camera: &engine::camera::Camera,
    mouse_position_x: f32,
    mouse_position_y: f32,
    viewport_width: f32,
    viewport_height: f32,
) -> Option<(engine::math::Vec3, Vec3)> {
    if viewport_width <= 0.0 || viewport_height <= 0.0 {
        return None;
    }

    let viewport_position_x = (mouse_position_x / viewport_width) * 2.0 - 1.0;
    let viewport_position_y = 1.0 - (mouse_position_y / viewport_height) * 2.0;
    let half_vertical_field_of_view = (camera.fovy.to_radians() * 0.5).tan();
    let half_horizontal_field_of_view = half_vertical_field_of_view * camera.aspect;

    let ray_direction = camera.forward()
        + camera.right() * viewport_position_x * half_horizontal_field_of_view
        + camera.up() * viewport_position_y * half_vertical_field_of_view;

    if ray_direction.length_squared() <= f32::EPSILON {
        return None;
    }

    Some((camera.position, ray_direction.normalize()))
}

fn trace_terrain_surface(
    ray_origin: engine::math::Vec3,
    ray_direction: Vec3,
    ray_start_distance: f32,
    ray_end_distance: f32,
    terrain: &PlanetTerrainSamplerContext<'_>,
) -> Option<(f32, engine::math::Vec3)> {
    let mut previous_distance = ray_start_distance.max(0.0);
    let previous_position = ray_origin + ray_direction * previous_distance;
    let mut previous_density = terrain_sampler::sample_final_density(terrain, previous_position);

    if previous_density.abs() < 0.5 {
        return Some((previous_distance, previous_position));
    }

    let mut current_distance = previous_distance;

    for _ in 0..256 {
        if current_distance > ray_end_distance {
            return None;
        }

        let step_distance = previous_density.abs().clamp(0.25, 64.0);
        current_distance = (current_distance + step_distance).min(ray_end_distance);

        let current_position = ray_origin + ray_direction * current_distance;
        let current_density = terrain_sampler::sample_final_density(terrain, current_position);

        if current_density.abs() < 0.5 {
            return Some((current_distance, current_position));
        }

        if previous_density.signum() != current_density.signum() {
            let mut lower_distance = previous_distance;
            let mut upper_distance = current_distance;
            let mut lower_density = previous_density;

            for _ in 0..16 {
                let middle_distance = (lower_distance + upper_distance) * 0.5;
                let middle_position = ray_origin + ray_direction * middle_distance;
                let middle_density =
                    terrain_sampler::sample_final_density(terrain, middle_position);

                if middle_density.abs() < 0.5 {
                    return Some((middle_distance, middle_position));
                }

                if lower_density.signum() == middle_density.signum() {
                    lower_distance = middle_distance;
                    lower_density = middle_density;
                } else {
                    upper_distance = middle_distance;
                }
            }

            let surface_distance = (lower_distance + upper_distance) * 0.5;
            return Some((
                surface_distance,
                ray_origin + ray_direction * surface_distance,
            ));
        }

        previous_distance = current_distance;
        previous_density = current_density;
    }

    None
}

fn node_overlaps_bounds(node: &OctreeNode, bounds_min: Vec3, bounds_max: Vec3) -> bool {
    let node_max = node.min + vec3(node.size, node.size, node.size);

    node.min.x <= bounds_max.x
        && node_max.x >= bounds_min.x
        && node.min.y <= bounds_max.y
        && node_max.y >= bounds_min.y
        && node.min.z <= bounds_max.z
        && node_max.z >= bounds_min.z
}

fn collect_dirty_mesh_requests(
    node: &OctreeNode,
    planet_entity: Entity,
    planet_position: Vec3,
    bounds_min: Vec3,
    bounds_max: Vec3,
    requests: &mut Vec<PlanetMeshRequest>,
) {
    if !node_overlaps_bounds(node, bounds_min, bounds_max) {
        return;
    }

    if let Some(children) = node.children.as_ref() {
        for child in children {
            collect_dirty_mesh_requests(
                child,
                planet_entity,
                planet_position,
                bounds_min,
                bounds_max,
                requests,
            );
        }
        return;
    }

    requests.push(PlanetMeshRequest {
        planet_entity,
        node_key: node.key,
        planet_position,
        node_min_corner: node.min,
        node_size: node.size,
        face_neighbors: [FaceNeighbor::SAME_OR_ABSENT; 6],
    });
}

// Keep terrain-edit temporaries out of the hotpatched walking-system frame.
#[inline(never)]
fn run_terrain_edit_phase<R>(phase: impl FnOnce() -> R) -> R {
    phase()
}

pub fn player_interaction_system(ctx: &mut SystemContext, commands: &mut Commands) {
    let Some(mode) = ctx
        .world
        .get_resource::<GameModeState>()
        .map(|mode| mode.mode)
    else {
        return;
    };

    match mode {
        GameMode::Walking => {
            let mut system = subsecond::HotFn::current(player_walking_system);
            system.call((ctx, commands));
        }
        GameMode::PilotingShip => {
            let mut system = subsecond::HotFn::current(player_piloting_ship_system);
            system.call((ctx, commands));
        }
        GameMode::Menu => {
            let mut system = subsecond::HotFn::current(player_menu_system);
            system.call((ctx, commands));
        }
        GameMode::Editor => {
            let mut system = subsecond::HotFn::current(player_editor_system);
            system.call((ctx, commands));
        }
    }
}

#[inline(never)]
fn player_walking_system(ctx: &mut SystemContext, commands: &mut Commands) {
    player_walking_system_body(ctx, commands);
}

#[inline(never)]
fn player_walking_system_body(ctx: &mut SystemContext, commands: &mut Commands) {
    let world = &mut ctx.world;

    let Some(input) = world.get_resource::<InputState>() else {
        return;
    };
    let Some(input_map) = world.get_resource::<InputMap>() else {
        return;
    };

    let walk_forward = input_map.pressed(&input, Action::WalkForward);
    let walk_backward = input_map.pressed(&input, Action::WalkBackward);
    let walk_right = input_map.pressed(&input, Action::WalkRight);
    let walk_left = input_map.pressed(&input, Action::WalkLeft);
    let jump = input_map.pressed(&input, Action::Jump);
    let crouch = input_map.pressed(&input, Action::Crouch);
    let sprint = input_map.pressed(&input, Action::Sprint);
    let roll_left = input_map.pressed(&input, Action::RollLeft);
    let roll_right = input_map.pressed(&input, Action::RollRight);
    let interact = input_map.just_pressed(&input, Action::Interact);
    let open_menu = input_map.just_pressed(&input, Action::OpenMenu);
    let alt_pressed =
        input.pressed.contains(&KeyCode::AltLeft) || input.pressed.contains(&KeyCode::AltRight);
    let inverse_deformation =
        input.pressed.contains(&KeyCode::ShiftLeft) || input.pressed.contains(&KeyCode::ShiftRight);
    let left_mouse_pressed = input.mouse_pressed.contains(&MouseButton::Left);
    let right_mouse_pressed = input.mouse_pressed.contains(&MouseButton::Right);
    let mouse_position = input.mouse_position;
    let mouse_delta = input.mouse_delta;
    let scroll = input.scroll;
    let is_mouse_over_game_view = input.is_mouse_over_game_view;
    let viewport_size = ctx.globals.renderer.renderer_api.get_surface_size();
    //let heightmap = world
    //    .get_resource::<Arc<EarthHeightmap>>()
    //    .map(|heightmap| Arc::clone(&*heightmap));

    drop(input_map);
    drop(input);

    let camera_entity = {
        let Some(camera) = world.get_resource::<GameCamera>() else {
            return;
        };
        camera.entity
    };
    let Some(camera) = world.get_resource::<GameCamera>() else {
        return;
    };

    let mut camera_world_position = camera.world_position;
    let mut camera_position = camera_world_position.as_vec3();
    let camera_aspect = camera.camera.aspect;
    let camera_near_plane = camera.camera.znear;
    let camera_far_plane = camera.camera.zfar;
    drop(camera);

    let brush_radius = {
        let mut game_state = world.get_resource_mut::<GameState>().unwrap();
        if alt_pressed && scroll.abs() > f32::EPSILON {
            let radius_scale = 1.15f32.powf(scroll);
            game_state.terrain_brush_radius =
                (game_state.terrain_brush_radius * radius_scale).clamp(2.0, 512.0);
            engine::game_info!(
                "terrain brush radius: {:.2}",
                game_state.terrain_brush_radius
            );
        }
        game_state.terrain_brush_radius
    };

    {
        let Some(mut camera_transform) = world.get_mut::<TransformComponent>(camera_entity) else {
            return;
        };

        let Some(mut camera_component) = world.get_mut::<CameraComponent>(camera_entity) else {
            return;
        };

        if !alt_pressed && scroll.abs() > f32::EPSILON {
            let sensitivity: f32 = 0.2;
            let factor = (1.0f32 + sensitivity).powf(scroll);
            camera_component.speed = (camera_component.speed * factor).clamp(0.001, 1_000_000.0);
        }

        let camera_field_of_view = camera_component.fov;
        let mut final_speed = camera_component.speed;
        drop(camera_component);

        if sprint {
            let distance = camera_world_position.length() as f32;
            final_speed *= distance.sqrt() * 0.1;
        }

        if right_mouse_pressed
            && (mouse_delta.0.abs() > f32::EPSILON || mouse_delta.1.abs() > f32::EPSILON)
        {
            let sensitivity = 0.1;
            let yaw = Quat::from_axis_angle(Vec3::Y, -(mouse_delta.0 * sensitivity).to_radians());
            let pitch = Quat::from_axis_angle(Vec3::X, -(mouse_delta.1 * sensitivity).to_radians());
            camera_transform.rotation = (camera_transform.rotation * yaw * pitch).normalize();
        }

        let mut roll_amount = 0.0;
        if roll_left {
            roll_amount -= 1.0;
        }
        if roll_right {
            roll_amount += 1.0;
        }
        if roll_amount != 0.0 {
            let roll = Quat::from_axis_angle(-Vec3::Z, roll_amount * 0.02);
            camera_transform.rotation = (camera_transform.rotation * roll).normalize();
        }

        let forward = camera_transform.rotation * -Vec3::Z;
        let right = camera_transform.rotation * Vec3::X;
        let up = camera_transform.rotation * Vec3::Y;
        let mut movement = Vec3::ZERO;
        if walk_forward {
            movement += forward;
        }
        if walk_backward {
            movement -= forward;
        }
        if walk_right {
            movement += right;
        }
        if walk_left {
            movement -= right;
        }
        if jump {
            movement += up;
        }
        if crouch {
            movement -= up;
        }
        if movement.length_squared() > f32::EPSILON {
            camera_world_position += (movement.normalize() * final_speed).as_dvec3();
            camera_position = camera_world_position.as_vec3();
            camera_transform.position = camera_position;
            world
                .get_resource_mut::<GameCamera>()
                .unwrap()
                .world_position = camera_world_position;
        }

        let terrain_ok = run_terrain_edit_phase(|| {
            // Deform with left click
            if left_mouse_pressed && is_mouse_over_game_view {
                if let Some((mouse_position_x, mouse_position_y)) = mouse_position {
                    let current_camera = engine::camera::Camera {
                        position: engine::math::vec3(
                            camera_transform.position.x,
                            camera_transform.position.y,
                            camera_transform.position.z,
                        ),
                        orientation: camera_transform.rotation,
                        aspect: camera_aspect,
                        fovy: camera_field_of_view,
                        znear: camera_near_plane,
                        zfar: camera_far_plane,
                    };

                    if let Some((ray_origin, ray_direction)) = ray_from_mouse_position(
                        &current_camera,
                        mouse_position_x,
                        mouse_position_y,
                        viewport_size.x as f32,
                        viewport_size.y as f32,
                    ) {
                        let mut closest_hit = None;
                        let mut query = Query::<(
                            &Planet,
                            &PlanetTerrainEdits,
                            &Arc<PlanetTerrainConfig>,
                        )>::new(world);

                        query.for_each(|entity, (planet, terrain_edits, terrain_config)| {
                            let Some((planet_entry_distance, planet_exit_distance)) =
                                octree::ray_intersects(
                                    &planet.octree_root,
                                    ray_origin,
                                    ray_direction,
                                )
                            else {
                                return;
                            };

                            let terrain = PlanetTerrainSamplerContext {
                                config: terrain_config.as_ref(),
                                edits: terrain_edits,
                                planet_position: planet.position,
                            };
                            let Some((surface_distance, surface_position)) = trace_terrain_surface(
                                ray_origin,
                                ray_direction,
                                planet_entry_distance,
                                planet_exit_distance,
                                &terrain,
                            ) else {
                                return;
                            };

                            if closest_hit
                                .is_none_or(|(_, hit_distance, _)| surface_distance < hit_distance)
                            {
                                closest_hit = Some((entity, surface_distance, surface_position));
                            }
                        });

                        drop(query);

                        if let Some((hit_entity, _hit_distance, hit_pos)) = closest_hit {
                            let hit_planet = world.get::<Planet>(hit_entity).unwrap();

                            let hit_world = hit_pos;
                            let planet_position = hit_planet.position;
                            let hit_local = hit_world - planet_position;

                            let brick_size = 32.0;
                            let level = 0;
                            let brush_strength = if inverse_deformation { -32.0 } else { 32.0 };
                            let min_brick = TerrainBrickKey {
                                x: ((hit_local.x - brush_radius) / brick_size).floor() as i32,
                                y: ((hit_local.y - brush_radius) / brick_size).floor() as i32,
                                z: ((hit_local.z - brush_radius) / brick_size).floor() as i32,
                                level,
                            };
                            let max_brick = TerrainBrickKey {
                                x: ((hit_local.x + brush_radius) / brick_size).floor() as i32,
                                y: ((hit_local.y + brush_radius) / brick_size).floor() as i32,
                                z: ((hit_local.z + brush_radius) / brick_size).floor() as i32,
                                level,
                            };
                            drop(hit_planet);

                            if world.get::<PlanetTerrainEdits>(hit_entity).is_none() {
                                engine::game_warn!("Planet terrain edits not found!");
                                return false;
                            }

                            let sample_count = TERRAIN_EDIT_SAMPLE_COUNT;
                            let mut terrain_edits =
                                world.get_mut::<PlanetTerrainEdits>(hit_entity).unwrap();
                            let PlanetTerrainEdits {
                                modified_chunks,
                                modified_ranges,
                            } = &mut *terrain_edits;

                            for brick_x in min_brick.x..=max_brick.x {
                                for brick_y in min_brick.y..=max_brick.y {
                                    for brick_z in min_brick.z..=max_brick.z {
                                        let key = TerrainBrickKey {
                                            x: brick_x,
                                            y: brick_y,
                                            z: brick_z,
                                            level,
                                        };
                                        let brick =
                                            modified_chunks.entry(key).or_insert_with(|| {
                                                Arc::new(vec![
                                                    vec![
                                                        vec![0.0; sample_count];
                                                        sample_count
                                                    ];
                                                    sample_count
                                                ])
                                            });
                                        if brick.len() != sample_count
                                            || brick.iter().any(|plane| {
                                                plane.len() != sample_count
                                                    || plane
                                                        .iter()
                                                        .any(|row| row.len() != sample_count)
                                            })
                                        {
                                            *brick = Arc::new(resample_terrain_edit_brick(
                                                brick.as_ref(),
                                                sample_count,
                                            ));
                                        }
                                        let brick = Arc::make_mut(brick);

                                        let brick_min = vec3(
                                            key.x as f32 * brick_size,
                                            key.y as f32 * brick_size,
                                            key.z as f32 * brick_size,
                                        );
                                        let sample_spacing =
                                            brick_size / TERRAIN_EDIT_CELL_COUNT as f32;
                                        let mut range_min = f32::INFINITY;
                                        let mut range_max = f32::NEG_INFINITY;

                                        for sample_x in 0..sample_count {
                                            for sample_y in 0..sample_count {
                                                for sample_z in 0..sample_count {
                                                    let sample_position = brick_min
                                                        + vec3(
                                                            sample_x as f32 * sample_spacing,
                                                            sample_y as f32 * sample_spacing,
                                                            sample_z as f32 * sample_spacing,
                                                        );
                                                    let distance_from_hit =
                                                        (sample_position - hit_local).length();

                                                    if distance_from_hit <= brush_radius {
                                                        let normalized_distance =
                                                            distance_from_hit / brush_radius;
                                                        let brush_influence =
                                                            1.0 - normalized_distance;
                                                        let smooth_influence = brush_influence
                                                            * brush_influence
                                                            * (2.2 - 2.0 * brush_influence);

                                                        brick[sample_x][sample_y][sample_z] +=
                                                            brush_strength * smooth_influence;
                                                    }

                                                    let value = brick[sample_x][sample_y][sample_z];
                                                    range_min = range_min.min(value);
                                                    range_max = range_max.max(value);
                                                }
                                            }
                                        }

                                        modified_ranges
                                            .insert(key, DensityRange::new(range_min, range_max));
                                    }
                                }
                            }
                            drop(terrain_edits);

                            // Refresh the hierarchy after the edit data changes.
                            // Use whole edited-brick bounds so interpolation and
                            // leaves touching a brick boundary are included.
                            let dirty_bounds_min = planet_position
                                + vec3(
                                    min_brick.x as f32 * brick_size,
                                    min_brick.y as f32 * brick_size,
                                    min_brick.z as f32 * brick_size,
                                );
                            let dirty_bounds_max = planet_position
                                + vec3(
                                    (max_brick.x + 1) as f32 * brick_size,
                                    (max_brick.y + 1) as f32 * brick_size,
                                    (max_brick.z + 1) as f32 * brick_size,
                                );
                            let mut dirty_mesh_requests = Vec::new();
                            let mut query = Query::<(
                                &mut Planet,
                                &PlanetTerrainEdits,
                                &Arc<PlanetTerrainConfig>,
                            )>::new(world);
                            query.for_each(|entity, (planet, terrain_edits, terrain_config)| {
                                if entity != hit_entity {
                                    return;
                                }

                                octree::refresh_density_ranges_in_bounds(
                                    &mut planet.octree_root,
                                    dirty_bounds_min,
                                    dirty_bounds_max,
                                    planet.position,
                                    terrain_config.as_ref(),
                                    terrain_edits,
                                );
                                collect_dirty_mesh_requests(
                                    &planet.octree_root,
                                    hit_entity,
                                    planet.position,
                                    dirty_bounds_min,
                                    dirty_bounds_max,
                                    &mut dirty_mesh_requests,
                                );
                                // A transition polygon contains dual vertices from both
                                // sides of an LOD boundary. If deformation changes either
                                // side, remesh the opposite-resolution leaves as well.
                                let dirty_snapshot = dirty_mesh_requests.clone();
                                let mut scheduled: HashSet<(i32, i32, i32, i32)> =
                                    dirty_mesh_requests
                                        .iter()
                                        .map(|request| {
                                            (
                                                request.node_min_corner.x as i32,
                                                request.node_min_corner.y as i32,
                                                request.node_min_corner.z as i32,
                                                request.node_size as i32,
                                            )
                                        })
                                        .collect();
                                for dirty in dirty_snapshot {
                                    let mut neighbors = Vec::new();
                                    octree::collect_face_neighbor_leaves(
                                        &planet.octree_root,
                                        dirty.node_min_corner,
                                        dirty.node_size,
                                        &mut neighbors,
                                    );
                                    for neighbor in neighbors {
                                        let key = (
                                            neighbor.min.x as i32,
                                            neighbor.min.y as i32,
                                            neighbor.min.z as i32,
                                            neighbor.size as i32,
                                        );
                                        if neighbor.size == dirty.node_size
                                            || !neighbor.has_surface
                                            || !scheduled.insert(key)
                                        {
                                            continue;
                                        }
                                        dirty_mesh_requests.push(PlanetMeshRequest {
                                            planet_entity: hit_entity,
                                            node_key: neighbor.key,
                                            planet_position: planet.position,
                                            node_min_corner: neighbor.min,
                                            node_size: neighbor.size,
                                            face_neighbors: [FaceNeighbor::SAME_OR_ABSENT; 6],
                                        });
                                    }
                                }
                                for request in &mut dirty_mesh_requests {
                                    octree::annotate_mesh_request(&planet.octree_root, request);
                                }
                            });
                            drop(query);

                            dirty_mesh_requests.sort_unstable_by(|a, b| {
                                let a_center = a.node_min_corner
                                    + vec3(a.node_size, a.node_size, a.node_size) * 0.5;
                                let b_center = b.node_min_corner
                                    + vec3(b.node_size, b.node_size, b.node_size) * 0.5;
                                (a_center - hit_world)
                                    .length_squared()
                                    .total_cmp(&(b_center - hit_world).length_squared())
                            });

                            commands.push(move |ctx| {
                                for request in dirty_mesh_requests {
                                    submit_requested_mesh_urgent(ctx, request);
                                }
                            });
                        }
                    }
                }
            }
            true
        });
        if !terrain_ok {
            return;
        }

        // Interact (spawn spheres for now)
        if interact {
            let mut rng = rand::thread_rng();

            commands.spawn_physical_sphere(PhysicalSphereParams {
                mass: 50.0,
                position: engine::math::vec3(
                    rng.gen_range(-5.0..5.0),
                    rng.gen_range(0.5..5.0),
                    rng.gen_range(-5.0..5.0),
                ),
                radius: 0.5,
            });
        }
    }

    // F to build block
    if interact {
        let Some((material_uuid, mesh)) = ensure_build_block_assets(world, &ctx.globals) else {
            return;
        };

        let assembly = world.spawn();
        world.insert(
            assembly,
            Assembly {
                position: camera_position,
                blocks: Vec::new(),
            },
        );

        world.insert(
            assembly,
            TransformComponent {
                position: vec3(camera_position.x, camera_position.y, camera_position.z),
                rotation: Quat::IDENTITY,
                scale: vec3(0.1, 0.1, 0.1),
                velocity: vec3(0.0, 0.0, 0.0),
            },
        );

        world.insert(
            assembly,
            MeshRendererComponent {
                material: material_uuid,
                mesh,
            },
        );
    }

    if open_menu {
        commands.push(|ctx| {
            ctx.world.get_resource_mut::<GameModeState>().unwrap().mode = GameMode::Menu;
        });
    }
}

fn player_piloting_ship_system(ctx: &mut SystemContext, _commands: &mut Commands) {
    let world = &ctx.world;
    let Some(input) = world.get_resource::<InputState>() else {
        return;
    };
    let Some(input_map) = world.get_resource::<InputMap>() else {
        return;
    };
    if input_map.just_pressed(&input, Action::Interact) {
        // exit ship
    }
}

fn player_menu_system(ctx: &mut SystemContext, commands: &mut Commands) {
    engine::game_info!("Menu");
    let world = &ctx.world;
    let Some(input) = world.get_resource::<InputState>() else {
        return;
    };
    let Some(input_map) = world.get_resource::<InputMap>() else {
        return;
    };
    if input_map.just_pressed(&input, Action::OpenMenu) {
        commands.push(|ctx| {
            ctx.world.get_resource_mut::<GameModeState>().unwrap().mode = GameMode::Walking;
        });
    }
}

fn player_editor_system(_ctx: &mut SystemContext, _commands: &mut Commands) {
    engine::game_info!("Editor");
}

const BUILD_BLOCK_MATERIAL: &str = "Material.plxmat";
const BUILD_BLOCK_MESH: &str = "Cube_Finished_Cube.plxmesh";

fn build_asset_dir() -> std::path::PathBuf {
    let asset_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("res")
        .join("imported");
    asset_dir
}

pub fn preload_build_block_assets(server: Res<AssetServer>) {
    let asset_dir = build_asset_dir();
    let material = asset_dir.join(BUILD_BLOCK_MATERIAL);
    let mesh = asset_dir.join(BUILD_BLOCK_MESH);
    if material.exists() && mesh.exists() {
        server.load::<Material>(material);
        server.load::<MeshAsset>(mesh);
    }
}

fn ensure_build_block_assets(
    world: &engine::ecs::world::World,
    globals: &GlobalResources,
) -> Option<(Uuid, Handle<MeshAsset>)> {
    let asset_dir = build_asset_dir();
    let server = world.get_resource::<AssetServer>()?;
    let material_handle = server.load::<Material>(asset_dir.join(BUILD_BLOCK_MATERIAL));
    let mesh_handle = server.load::<MeshAsset>(asset_dir.join(BUILD_BLOCK_MESH));
    drop(server);

    let vertex_layout = world
        .get_resource::<Assets<MeshAsset>>()?
        .get(mesh_handle)?
        .vertex_layout
        .clone();
    let mut materials = world.get_resource_mut::<Assets<Material>>()?;
    let material = materials.get_mut(material_handle)?;
    material.set_vertex_layouts(vec![vertex_layout, TransformInstance::layout()]);
    Some((material.uuid, globals.renderer.default_meshes().cube))
}
