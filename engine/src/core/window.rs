use std::path::PathBuf;

use crate::core::{
    ecs::{
        event::EventReader, plugin::Plugin, resource::ResMut, schedule::CoreSchedule,
        system::GlobalsMut,
    },
    input::{InputState, KeyCode, MouseButton},
};

/// Installs the platform event resources and translates them into the
/// frame-oriented input resource used by gameplay systems.
pub struct WindowPlugin;

impl Plugin for WindowPlugin {
    fn build(&self, app: &mut crate::App) {
        app.add_event::<winit::event::WindowEvent>()
            .add_event::<WindowResized>()
            .add_event::<KeyboardInput>()
            .add_event::<MouseButtonInput>()
            .add_event::<CursorMoved>()
            .add_event::<MouseMotion>()
            .add_event::<MouseWheel>()
            .add_event::<FileDropped>()
            .add_system(CoreSchedule::First, update_input_from_window_events)
            .add_system(CoreSchedule::First, handle_engine_shortcuts)
            .add_system(CoreSchedule::Last, clear_transient_input);
    }
}

/// Sent by the winit adapter when the drawable area changes size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowResized {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyboardInput {
    pub key_code: KeyCode,
    pub pressed: bool,
    pub repeat: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseButtonInput {
    pub button: MouseButton,
    pub pressed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorMoved {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MouseMotion {
    pub delta_x: f32,
    pub delta_y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MouseWheel {
    pub horizontal: f32,
    pub vertical: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDropped {
    pub path: PathBuf,
}

/// Example consumer: platform input events become the frame-oriented
/// `InputState` resource used by gameplay systems.
pub fn update_input_from_window_events(
    mut keyboard_events: EventReader<KeyboardInput>,
    mut mouse_button_events: EventReader<MouseButtonInput>,
    mut cursor_events: EventReader<CursorMoved>,
    mut mouse_motion_events: EventReader<MouseMotion>,
    mut mouse_wheel_events: EventReader<MouseWheel>,
    mut input: ResMut<InputState>,
) {
    for event in keyboard_events.read() {
        if event.pressed {
            let newly_pressed = input.pressed.insert(event.key_code);
            if newly_pressed && !event.repeat {
                input.just_pressed.insert(event.key_code);
            }
        } else {
            input.pressed.remove(&event.key_code);
            input.just_released.insert(event.key_code);
        }
    }

    for event in mouse_button_events.read() {
        if event.pressed {
            if input.mouse_pressed.insert(event.button) {
                input.mouse_just_pressed.insert(event.button);
            }
        } else {
            input.mouse_pressed.remove(&event.button);
            input.mouse_just_released.insert(event.button);
        }
    }

    for event in cursor_events.read() {
        input.mouse_position = Some((event.x, event.y));
    }

    for event in mouse_motion_events.read() {
        input.mouse_delta.0 += event.delta_x;
        input.mouse_delta.1 += event.delta_y;
    }

    for event in mouse_wheel_events.read() {
        input.scroll += event.vertical.clamp(-1.0, 1.0);
    }
}

/// Clears values that are valid for only one frame. Held keys/buttons remain.
pub fn clear_transient_input(mut input: ResMut<InputState>) {
    input.just_pressed.clear();
    input.just_released.clear();
    input.mouse_just_pressed.clear();
    input.mouse_just_released.clear();
    input.mouse_delta = (0.0, 0.0);
    input.scroll = 0.0;
}

/// Engine-wide shortcuts that operate on platform/renderer state. Gameplay
/// input remains available through `InputState` and the typed event readers.
pub fn handle_engine_shortcuts(
    mut keyboard_events: EventReader<KeyboardInput>,
    mut globals: GlobalsMut,
) {
    for event in keyboard_events.read() {
        if !event.pressed || event.repeat {
            continue;
        }

        match event.key_code {
            KeyCode::KeyH => globals.frame_capturer.request_capture(),
            KeyCode::KeyR => globals.renderer.renderer_api.reload_shaders(),
            KeyCode::F10 => globals.renderer.renderer_api.toggle_present_mode(),
            _ => {}
        }
    }
}

/// Example event consumer that owns GPU-side resize preparation.
pub fn resize_renderer_from_events(
    mut resize_events: EventReader<WindowResized>,
    mut globals: GlobalsMut,
) {
    for event in resize_events.read() {
        if event.width == 0 || event.height == 0 {
            continue;
        }

        let renderer = &mut globals.renderer;
        renderer.resize(event.width, event.height);
        let crate::renderer::Renderer {
            render_graph,
            renderer_api,
            render_resources,
            ..
        } = renderer;
        render_graph.resize(
            renderer_api.as_mut(),
            render_resources,
            event.width,
            event.height,
        );
    }
}
