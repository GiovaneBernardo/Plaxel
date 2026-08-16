use crate::{
    core::input::{InputState, KeyCode, MouseButton},
    ecs::resource::Res,
};

#[inline(never)]
pub fn engine_input_system(input: Res<InputState>) {
    engine_input_system_impl(input);
}

fn engine_input_system_impl(input: Res<InputState>) {
    if input.pressed.contains(&KeyCode::AltLeft)
        && input.mouse_just_pressed.contains(&MouseButton::Right)
    {}
}
