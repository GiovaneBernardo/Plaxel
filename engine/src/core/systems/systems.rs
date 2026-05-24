use crate::{
    core::input::{InputState, KeyCode, MouseButton},
    ecs::{commands::Commands, world::World},
};

pub fn engine_input_system(world: &mut World, _commands: &mut Commands) {
    let Some(input) = world.get_resource::<InputState>() else {
        return;
    };

    if input.pressed.contains(&KeyCode::AltLeft)
        && input.mouse_just_pressed.contains(&MouseButton::Right)
    {}
}
