use engine::core::components::core::{CameraComponent, TransformComponent};
use rand::Rng;

use cgmath::{InnerSpace, Quaternion, Rotation3, Vector3};

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
            drop(camera);

            let Some(mut camera_transform) = world.get_mut::<TransformComponent>(camera_entity)
            else {
                return;
            };

            let Some(mut camera_component) = world.get_mut::<CameraComponent>(camera_entity) else {
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
                camera_transform.rotation = (camera_transform.rotation * yaw * pitch).normalize();
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
