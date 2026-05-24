use crate::{
    core::input::{InputState, KeyCode, MouseButton},
    ecs::{commands::Commands, system::SystemContext},
};

pub fn engine_input_system(ctx: &mut SystemContext, _commands: &mut Commands) {
    let Some(input) = ctx.world.get_resource::<InputState>() else {
        return;
    };

    if input.pressed.contains(&KeyCode::AltLeft)
        && input.mouse_just_pressed.contains(&MouseButton::Right)
    {}
}
