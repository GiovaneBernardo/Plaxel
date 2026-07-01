use std::any::TypeId;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use engine::assets::importer::AssetPayload;
use engine::assets::loader;
use engine::assets::manager::{Asset, Handle, UntypedHandle, Uuid};
use engine::assets::material::{Material, MaterialResource};
use engine::core::components::core::{CameraComponent, TransformComponent};
use engine::core::components::renderer::MeshRendererComponent;
use engine::ecs::entity::Entity;
use engine::ecs::query::Query;
use engine::global_resources::GlobalResources;
use engine::model::{MeshAsset, TransformInstance, Vertex};
use engine::renderer::{FrameBindings, GeometryPassNode};
use game_types::assembly::Assembly;
use game_types::octree::{OctreeNode, PlanetMeshRequest};
use game_types::planet::{Planet, PlanetTerrainEdits, TerrainBrickKey};
use rand::Rng;

use cgmath::{EuclideanSpace, InnerSpace, Quaternion, Rotation3, Vector3, vec3};

use engine::core::input::{InputState, KeyCode, MouseButton};
use engine::ecs::commands::{Commands, PhysicalSphereParams};
use engine::ecs::system::SystemContext;
use game_types::game_mode::{GameMode, GameModeState};

use crate::{
    GameCamera, GameState, octree,
    sdf::{EarthHeightmap, sdf_at_center},
    systems::planets::submit_requested_mesh_urgent,
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

pub struct InputMap {
    interact: KeyCode,
    open_menu: KeyCode,
    walk_forward: KeyCode,
    walk_left: KeyCode,
    walk_right: KeyCode,
    walk_backward: KeyCode,
    roll_left: KeyCode,
    roll_right: KeyCode,
    jump: KeyCode,
    crouch: KeyCode,
    sprint: KeyCode,
}

impl Default for InputMap {
    fn default() -> Self {
        Self {
            interact: KeyCode::KeyF,
            open_menu: KeyCode::Tab,
            walk_forward: KeyCode::KeyW,
            walk_left: KeyCode::KeyA,
            walk_right: KeyCode::KeyD,
            walk_backward: KeyCode::KeyS,
            roll_left: KeyCode::KeyQ,
            roll_right: KeyCode::KeyE,
            jump: KeyCode::Space,
            crouch: KeyCode::KeyC,
            sprint: KeyCode::ShiftLeft,
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
) -> Option<(cgmath::Point3<f32>, Vector3<f32>)> {
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

    if ray_direction.magnitude2() <= f32::EPSILON {
        return None;
    }

    Some((camera.position, ray_direction.normalize()))
}

fn trace_terrain_surface(
    ray_origin: cgmath::Point3<f32>,
    ray_direction: Vector3<f32>,
    ray_start_distance: f32,
    ray_end_distance: f32,
    planet_position: Vector3<f32>,
    planet_size: u32,
    heightmap: Option<&EarthHeightmap>,
    terrain_edits: &PlanetTerrainEdits,
) -> Option<(f32, cgmath::Point3<f32>)> {
    let mut previous_distance = ray_start_distance.max(0.0);
    let previous_position = ray_origin + ray_direction * previous_distance;
    let mut previous_density = sdf_at_center(
        previous_position.to_vec(),
        planet_position,
        planet_size,
        heightmap,
        terrain_edits,
    );

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
        let current_density = sdf_at_center(
            current_position.to_vec(),
            planet_position,
            planet_size,
            heightmap,
            terrain_edits,
        );

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
                let middle_density = sdf_at_center(
                    middle_position.to_vec(),
                    planet_position,
                    planet_size,
                    heightmap,
                    terrain_edits,
                );

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

fn node_overlaps_bounds(
    node: &OctreeNode,
    bounds_min: Vector3<f32>,
    bounds_max: Vector3<f32>,
) -> bool {
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
    planet_position: Vector3<f32>,
    planet_size: u32,
    bounds_min: Vector3<f32>,
    bounds_max: Vector3<f32>,
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
                planet_size,
                bounds_min,
                bounds_max,
                requests,
            );
        }
        return;
    }

    requests.push(PlanetMeshRequest {
        planet_entity,
        planet_position,
        planet_size,
        node_min_corner: node.min,
        node_size: node.size,
    });
}

pub fn player_interaction_system(ctx: &mut SystemContext, commands: &mut Commands) {
    let world = &mut ctx.world;

    let Some(mode) = world.get_resource::<GameModeState>().map(|mode| mode.mode) else {
        return;
    };

    match mode {
        GameMode::Walking => {
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
            let alt_pressed = input.pressed.contains(&KeyCode::AltLeft)
                || input.pressed.contains(&KeyCode::AltRight);
            let inverse_deformation = input.pressed.contains(&KeyCode::ShiftLeft)
                || input.pressed.contains(&KeyCode::ShiftRight);
            let left_mouse_pressed = input.mouse_pressed.contains(&MouseButton::Left);
            let right_mouse_pressed = input.mouse_pressed.contains(&MouseButton::Right);
            let mouse_position = input.mouse_position;
            let mouse_delta = input.mouse_delta;
            let scroll = input.scroll;
            let viewport_size = ctx.globals.renderer.renderer_api.get_surface_size();
            let heightmap = world
                .get_resource::<Arc<EarthHeightmap>>()
                .map(|heightmap| Arc::clone(&*heightmap));

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

            let forward = camera.camera.forward();
            let right = camera.camera.right();
            let up = camera.camera.up();
            let camera_position = camera.camera.position;
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
                let Some(mut camera_transform) = world.get_mut::<TransformComponent>(camera_entity)
                else {
                    return;
                };

                let Some(mut camera_component) = world.get_mut::<CameraComponent>(camera_entity)
                else {
                    return;
                };

                if !alt_pressed && scroll.abs() > f32::EPSILON {
                    let sensitivity: f32 = 0.2;
                    let factor = (1.0f32 + sensitivity).powf(scroll);
                    camera_component.speed =
                        (camera_component.speed * factor).clamp(0.001, 1_000_000.0);
                }

                let camera_field_of_view = camera_component.fov;
                let mut final_speed = camera_component.speed;
                drop(camera_component);

                if sprint {
                    let distance = camera_transform.position.magnitude();
                    final_speed *= distance.sqrt() * 0.1;
                }

                let mut movement = Vector3::new(0.0, 0.0, 0.0);
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
                if movement.magnitude2() > f32::EPSILON {
                    camera_transform.position += movement.normalize() * final_speed;
                }

                if right_mouse_pressed
                    && (mouse_delta.0.abs() > f32::EPSILON || mouse_delta.1.abs() > f32::EPSILON)
                {
                    let sensitivity = 0.1;
                    let yaw = Quaternion::from_axis_angle(
                        Vector3::unit_y(),
                        cgmath::Rad(-(mouse_delta.0 * sensitivity).to_radians()),
                    );
                    let pitch = Quaternion::from_axis_angle(
                        Vector3::unit_x(),
                        cgmath::Rad(-(mouse_delta.1 * sensitivity).to_radians()),
                    );
                    camera_transform.rotation =
                        (camera_transform.rotation * yaw * pitch).normalize();
                }

                // Roll (Q/E): rotate around local forward (-Z).
                let mut roll_amount = 0.0f32;
                if roll_left {
                    roll_amount -= 1.0;
                }
                if roll_right {
                    roll_amount += 1.0;
                }
                if roll_amount != 0.0 {
                    let roll = Quaternion::from_axis_angle(
                        -Vector3::unit_z(),
                        cgmath::Rad(roll_amount * 0.02),
                    );
                    camera_transform.rotation = (camera_transform.rotation * roll).normalize();
                }

                // Deform with left click
                if left_mouse_pressed {
                    if let Some((mouse_position_x, mouse_position_y)) = mouse_position {
                        let current_camera = engine::camera::Camera {
                            position: cgmath::point3(
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
                            let mut query = Query::<(&Planet, &PlanetTerrainEdits)>::new(&world);

                            query.for_each(|entity, (planet, terrain_edits)| {
                                let Some((planet_entry_distance, planet_exit_distance)) =
                                    octree::ray_intersects(
                                        &planet.octree_root,
                                        ray_origin,
                                        ray_direction,
                                    )
                                else {
                                    return;
                                };

                                let planet_size = (planet.octree_root.size * 2.0) as u32;
                                let Some((surface_distance, surface_position)) =
                                    trace_terrain_surface(
                                        ray_origin,
                                        ray_direction,
                                        planet_entry_distance,
                                        planet_exit_distance,
                                        planet.position,
                                        planet_size,
                                        heightmap.as_deref(),
                                        terrain_edits,
                                    )
                                else {
                                    return;
                                };

                                if closest_hit.clone().is_none_or(|(_, hit_distance, _)| {
                                    surface_distance < hit_distance
                                }) {
                                    closest_hit =
                                        Some((entity, surface_distance, surface_position));
                                }
                            });

                            drop(query);

                            if let Some((hit_entity, _hit_distance, hit_pos)) = closest_hit {
                                let hit_planet = world.get::<Planet>(hit_entity).unwrap();

                                let hit_world = hit_pos.to_vec();
                                let hit_local = hit_world - hit_planet.position;

                                let brick_size = 32.0;
                                let level = 0;
                                let brush_strength = if inverse_deformation { -32.0 } else { 32.0 };
                                let brush_bounds_min =
                                    hit_world - vec3(brush_radius, brush_radius, brush_radius);
                                let brush_bounds_max =
                                    hit_world + vec3(brush_radius, brush_radius, brush_radius);
                                let planet_size = (hit_planet.octree_root.size * 2.0) as u32;
                                let mut dirty_mesh_requests = Vec::new();
                                collect_dirty_mesh_requests(
                                    &hit_planet.octree_root,
                                    hit_entity,
                                    hit_planet.position,
                                    planet_size,
                                    brush_bounds_min,
                                    brush_bounds_max,
                                    &mut dirty_mesh_requests,
                                );
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

                                if world.get::<PlanetTerrainEdits>(hit_entity).is_none() {
                                    engine::game_warn!("Planet terrain edits not found!");
                                    return;
                                }

                                let resolution = 16usize;
                                let modified_chunks = &mut world
                                    .get_mut::<PlanetTerrainEdits>(hit_entity)
                                    .unwrap()
                                    .modified_chunks;

                                for brick_x in min_brick.x..=max_brick.x {
                                    for brick_y in min_brick.y..=max_brick.y {
                                        for brick_z in min_brick.z..=max_brick.z {
                                            let key = TerrainBrickKey {
                                                x: brick_x,
                                                y: brick_y,
                                                z: brick_z,
                                                level,
                                            };
                                            let brick = modified_chunks
                                                .entry(key.clone())
                                                .or_insert_with(|| {
                                                    Arc::new(vec![
                                                        vec![
                                                            vec![0.0; resolution];
                                                            resolution
                                                        ];
                                                        resolution
                                                    ])
                                                });
                                            let brick = Arc::make_mut(brick);

                                            let brick_min = vec3(
                                                key.x as f32 * brick_size,
                                                key.y as f32 * brick_size,
                                                key.z as f32 * brick_size,
                                            );
                                            let sample_spacing = brick_size / resolution as f32;

                                            for sample_x in 0..resolution {
                                                for sample_y in 0..resolution {
                                                    for sample_z in 0..resolution {
                                                        let sample_position = brick_min
                                                            + vec3(
                                                                (sample_x as f32 + 0.5)
                                                                    * sample_spacing,
                                                                (sample_y as f32 + 0.5)
                                                                    * sample_spacing,
                                                                (sample_z as f32 + 0.5)
                                                                    * sample_spacing,
                                                            );
                                                        let distance_from_hit = (sample_position
                                                            - hit_local)
                                                            .magnitude();

                                                        if distance_from_hit > brush_radius {
                                                            continue;
                                                        }

                                                        let normalized_distance =
                                                            distance_from_hit / brush_radius;
                                                        let brush_influence =
                                                            1.0 - normalized_distance;
                                                        let smooth_influence = brush_influence
                                                            * brush_influence
                                                            * (3.0 - 2.0 * brush_influence);

                                                        brick[sample_x][sample_y][sample_z] +=
                                                            brush_strength * smooth_influence;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                engine::game_info!(
                                    "terrain edit ray origin: {:?}, direction: {:?}, Hit Pos: {:?}",
                                    ray_origin,
                                    ray_direction,
                                    hit_pos
                                );

                                dirty_mesh_requests.sort_by(|a, b| {
                                    let a_center = a.node_min_corner
                                        + vec3(a.node_size, a.node_size, a.node_size) * 0.5;
                                    let b_center = b.node_min_corner
                                        + vec3(b.node_size, b.node_size, b.node_size) * 0.5;
                                    (a_center - hit_world)
                                        .magnitude2()
                                        .total_cmp(&(b_center - hit_world).magnitude2())
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

                // Interact (spawn spheres for now)
                if interact {
                    let mut rng = rand::thread_rng();

                    commands.spawn_physical_sphere(PhysicalSphereParams {
                        mass: 50.0,
                        position: cgmath::vec3(
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
                let Some((material_uuid, mesh)) = ensure_build_block_assets(ctx.globals) else {
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
                        rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
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

        GameMode::PilotingShip => {
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

        GameMode::Menu => {
            engine::game_info!("Menu");
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

        GameMode::Editor => {
            engine::game_info!("Editor");
        }
    }
}

const BUILD_BLOCK_MATERIAL: &str = "Material.plxmat";
const BUILD_BLOCK_MESH: &str = "Cube_Finished_Cube.plxmesh";

fn ensure_build_block_assets(globals: &mut GlobalResources) -> Option<(Uuid, Handle<MeshAsset>)> {
    if let (Some(material), Some(mesh)) = (
        globals
            .asset_manager
            .get_by_name::<Material>(BUILD_BLOCK_MATERIAL),
        globals.asset_manager.handle::<MeshAsset>(BUILD_BLOCK_MESH),
    ) {
        return Some((material.uuid, mesh));
    }

    let asset_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("res")
        .join("imported");
    let material_path = asset_dir.join(BUILD_BLOCK_MATERIAL);
    let mesh_path = asset_dir.join(BUILD_BLOCK_MESH);

    let mesh_header = match loader::load_header(&mesh_path) {
        Ok(header) => header,
        Err(error) => {
            engine::game_warn!(
                "Unable to build block: failed to read mesh header {mesh_path:?}: {error}"
            );
            return None;
        }
    };
    let mesh = match loader::load_payload(&mesh_path) {
        Ok(AssetPayload::Mesh(mesh)) => mesh,
        Ok(_) => {
            engine::game_warn!("Unable to build block: {mesh_path:?} is not a mesh asset");
            return None;
        }
        Err(error) => {
            engine::game_warn!("Unable to build block: failed to load mesh {mesh_path:?}: {error}");
            return None;
        }
    };

    let mesh_handle = globals.renderer.renderer_api.upload_mesh(&mesh);
    globals
        .asset_manager
        .paths
        .insert(mesh_path.clone(), mesh.uuid);
    globals.asset_manager.headers.insert(mesh.uuid, mesh_header);
    register_asset_name::<MeshAsset>(globals, BUILD_BLOCK_MESH, mesh.uuid);
    register_asset_name::<MeshAsset>(globals, &mesh.name, mesh.uuid);
    globals.asset_manager.add_asset::<MeshAsset>(mesh.clone());

    let material_header = match loader::load_header(&material_path) {
        Ok(header) => header,
        Err(error) => {
            engine::game_warn!(
                "Unable to build block: failed to read material header {material_path:?}: {error}"
            );
            return None;
        }
    };
    let mut material = match loader::load_payload(&material_path) {
        Ok(AssetPayload::Material(material)) => material,
        Ok(_) => {
            engine::game_warn!("Unable to build block: {material_path:?} is not a material asset");
            return None;
        }
        Err(error) => {
            engine::game_warn!(
                "Unable to build block: failed to load material {material_path:?}: {error}"
            );
            return None;
        }
    };

    upload_material_textures(globals, &material_path, &material);
    material.pipeline_descriptor.vertex_layouts =
        vec![mesh.vertex_layout.clone(), TransformInstance::layout()];
    material.material_index = globals
        .renderer
        .renderer_api
        .upload_material_asset(&material, None);

    let Some(camera_layout) = globals
        .renderer
        .render_graph
        .get_node_mut::<GeometryPassNode>(0)
        .and_then(|node| node.camera_bind_group_layout)
    else {
        engine::game_warn!(
            "Unable to build block: geometry camera bind group layout is unavailable"
        );
        return None;
    };
    let Some(textures_layout) = globals
        .renderer
        .render_resources
        .get_labeled::<FrameBindings>("frame_bindings")
        .map(|bindings| bindings.textures_layout)
    else {
        engine::game_warn!("Unable to build block: frame texture bind group layout is unavailable");
        return None;
    };

    let target_info = {
        let descriptor = GeometryPassNode::pass_descriptor();
        globals
            .renderer
            .renderer_api
            .target_info_for_pass(&descriptor, &globals.renderer.render_graph.resources)
    };
    globals.renderer.renderer_api.create_pipeline(
        &material,
        &[camera_layout, textures_layout],
        &target_info,
    );

    let material_uuid = material.uuid;
    globals
        .asset_manager
        .paths
        .insert(material_path.clone(), material_uuid);
    globals
        .asset_manager
        .headers
        .insert(material_uuid, material_header);
    register_asset_name::<Material>(globals, BUILD_BLOCK_MATERIAL, material_uuid);
    globals.asset_manager.add_asset::<Material>(material);

    Some((material_uuid, mesh_handle))
}

fn upload_material_textures(
    globals: &mut GlobalResources,
    material_path: &Path,
    material: &Material,
) {
    for binding in &material.bindings {
        let MaterialResource::Texture(texture_uuid) = binding.resource else {
            continue;
        };
        if globals
            .renderer
            .renderer_api
            .is_texture_asset_uploaded(texture_uuid)
        {
            continue;
        }

        let Some(texture_path) = find_sibling_asset_by_uuid(material_path, texture_uuid, "plxtex")
        else {
            engine::game_warn!(
                "Unable to build block: material {material_path:?} references missing texture {texture_uuid}"
            );
            continue;
        };
        let Ok(AssetPayload::Texture(texture)) = loader::load_payload(&texture_path) else {
            engine::game_warn!(
                "Unable to build block: failed to load texture asset {texture_path:?}"
            );
            continue;
        };

        globals
            .renderer
            .renderer_api
            .upload_texture_asset(&texture, None);
    }
}

fn find_sibling_asset_by_uuid(asset_path: &Path, uuid: Uuid, extension: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(asset_path.parent()?).ok()?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(extension))
        {
            continue;
        }

        let Ok(header) = loader::load_header(&path) else {
            continue;
        };
        if header.uuid == uuid {
            return Some(path);
        }
    }

    None
}

fn register_asset_name<T: Asset + 'static>(globals: &mut GlobalResources, name: &str, uuid: Uuid) {
    globals.asset_manager.names.insert(
        (T::ASSET_TYPE, name.to_string()),
        UntypedHandle {
            uuid,
            asset_type: T::ASSET_TYPE,
            type_id: TypeId::of::<T>(),
        },
    );
}
