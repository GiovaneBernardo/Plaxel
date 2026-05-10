use rand::Rng;

use cgmath::Vector3;
use engine::core::input::{InputState, KeyCode};
use engine::ecs::commands::Commands;
use engine::ecs::world::World;
use game_types::game_mode::{GameMode, GameModeState};

use crate::GameCamera;
use crate::systems::commands::{GameCommandsExt, PhysicalSphereParams};

pub enum Action {
    Interact,
    OpenMenu,
    WalkForward,
    WalkLeft,
    WalkRight,
    WalkBackward,
}

pub struct InputMap {
    interact: KeyCode,
    open_menu: KeyCode,
    walk_forward: KeyCode,
    walk_left: KeyCode,
    walk_right: KeyCode,
    walk_backward: KeyCode,
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
        }
    }
}

impl InputMap {
    fn just_pressed(&self, input: &InputState, action: Action) -> bool {
        let key = match action {
            Action::Interact => self.interact,
            Action::OpenMenu => self.open_menu,
            Action::WalkForward => self.walk_forward,
            Action::WalkLeft => self.walk_left,
            Action::WalkRight => self.walk_right,
            Action::WalkBackward => self.walk_backward,
        };

        input.just_pressed.contains(&key)
    }
}

pub fn player_interaction_system(world: &mut World, commands: &mut Commands) {
    let Some(input) = world.get_resource::<InputState>() else {
        return;
    };
    let Some(input_map) = world.get_resource::<InputMap>() else {
        return;
    };
    let Some(mode) = world.get_resource::<GameModeState>() else {
        return;
    };

    match mode.mode {
        GameMode::Walking => {
            println!("Walking");
            if input.pressed.contains(&KeyCode::KeyT) {
                println!("Pressionando T :D ");
            }
            if input_map.just_pressed(&input, Action::Interact) {
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

            if input_map.just_pressed(&input, Action::OpenMenu) {
                commands.push(|world| {
                    world.get_resource_mut::<GameModeState>().unwrap().mode = GameMode::Menu;
                });
            }
        }

        GameMode::PilotingShip => {
            if input_map.just_pressed(&input, Action::Interact) {
                // exit ship
            }
        }

        GameMode::Menu => {
            println!("Menu");
            if input_map.just_pressed(&input, Action::OpenMenu) {
                commands.push(|world| {
                    world.get_resource_mut::<GameModeState>().unwrap().mode = GameMode::Walking;
                });
            }
        }

        GameMode::Editor => {
            println!("Editor");
        }
    }
}
