//! The game renders behind the dock, so this tab only claims the space and reports
//! whether the pointer is over it. It paints nothing: anything drawn here would sit
//! on top of the rendered frame.

use crate::EditorContext;
use egui::Ui;

pub fn draw_viewport_tab(ui: &mut Ui, state: &mut EditorContext<'_>) {
    let hovered = ui.rect_contains_pointer(ui.max_rect());

    if let Some(scene) = state.active_scene()
        && let Some(mut input) = scene
            .world()
            .get_resource_mut::<engine::core::input::InputState>()
    {
        input.is_mouse_over_game_view = hovered;
    }
}
