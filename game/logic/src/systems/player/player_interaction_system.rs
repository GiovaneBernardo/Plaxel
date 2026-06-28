use std::any::TypeId;
use std::path::{Path, PathBuf};

use engine::assets::importer::AssetPayload;
use engine::assets::loader;
use engine::assets::manager::{Asset, Handle, UntypedHandle, Uuid};
use engine::assets::material::{Material, MaterialResource};
use engine::core::components::core::{CameraComponent, TransformComponent};
use engine::core::components::renderer::MeshRendererComponent;
use engine::global_resources::GlobalResources;
use engine::model::{MeshAsset, TransformInstance, Vertex};
use engine::renderer::{FrameBindings, GeometryPassNode};
use game_types::assembly::Assembly;
use rand::Rng;

use cgmath::{InnerSpace, Quaternion, Rotation3, Vector3, vec3};

use engine::core::input::{InputState, KeyCode, MouseButton};
use engine::ecs::commands::{Commands, PhysicalSphereParams};
use engine::ecs::system::SystemContext;
use game_types::game_mode::{GameMode, GameModeState};

use crate::GameCamera;

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
            let left_mouse_just_pressed = input.mouse_just_pressed.contains(&MouseButton::Left);
            let right_mouse_pressed = input.mouse_pressed.contains(&MouseButton::Right);
            let mouse_delta = input.mouse_delta;
            let scroll = input.scroll;

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
            drop(camera);

            {
                let Some(mut camera_transform) = world.get_mut::<TransformComponent>(camera_entity)
                else {
                    return;
                };

                let Some(mut camera_component) = world.get_mut::<CameraComponent>(camera_entity)
                else {
                    return;
                };

                if scroll.abs() > f32::EPSILON {
                    let sensitivity: f32 = 0.2;
                    let factor = (1.0f32 + sensitivity).powf(scroll);
                    camera_component.speed =
                        (camera_component.speed * factor).clamp(0.001, 1_000_000.0);
                }

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
            println!("Menu");
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
            println!("Editor");
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
            println!("Unable to build block: failed to read mesh header {mesh_path:?}: {error}");
            return None;
        }
    };
    let mesh = match loader::load_payload(&mesh_path) {
        Ok(AssetPayload::Mesh(mesh)) => mesh,
        Ok(_) => {
            println!("Unable to build block: {mesh_path:?} is not a mesh asset");
            return None;
        }
        Err(error) => {
            println!("Unable to build block: failed to load mesh {mesh_path:?}: {error}");
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
            println!(
                "Unable to build block: failed to read material header {material_path:?}: {error}"
            );
            return None;
        }
    };
    let mut material = match loader::load_payload(&material_path) {
        Ok(AssetPayload::Material(material)) => material,
        Ok(_) => {
            println!("Unable to build block: {material_path:?} is not a material asset");
            return None;
        }
        Err(error) => {
            println!("Unable to build block: failed to load material {material_path:?}: {error}");
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
        println!("Unable to build block: geometry camera bind group layout is unavailable");
        return None;
    };
    let Some(textures_layout) = globals
        .renderer
        .render_resources
        .get_labeled::<FrameBindings>("frame_bindings")
        .map(|bindings| bindings.textures_layout)
    else {
        println!("Unable to build block: frame texture bind group layout is unavailable");
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
            println!(
                "Unable to build block: material {material_path:?} references missing texture {texture_uuid}"
            );
            continue;
        };
        let Ok(AssetPayload::Texture(texture)) = loader::load_payload(&texture_path) else {
            println!("Unable to build block: failed to load texture asset {texture_path:?}");
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
