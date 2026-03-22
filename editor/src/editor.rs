use egui::ViewportId;

#[derive(Clone)]
pub struct EditorState {
    pub egui_context: egui::Context,
}

pub fn update(state: &mut engine::State, editor_state: &mut EditorState) {}
