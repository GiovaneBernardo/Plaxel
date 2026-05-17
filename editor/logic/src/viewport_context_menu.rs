use egui::Response;

use crate::egui_node::EguiRenderNode;

#[unsafe(no_mangle)]
pub fn viewport_context_menu(
    viewport_menu_pos: &mut egui::Pos2,
    viewport_menu_open: &mut bool,
    state: &mut engine::State,
    ctx: &egui::Context,
) -> Response {
    egui::Area::new(egui::Id::new("viewport_context_menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(*viewport_menu_pos)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                if ui.button("Spawn cube").clicked() {
                    *viewport_menu_open = false;
                }
            });
        })
        .response
}
