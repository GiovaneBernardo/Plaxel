//! Live view of global resources and the resources stored in the active world.

use crate::EditorContext;
use crate::panels::fields::display_type_name;
use crate::panels::reflect::{draw_reflected_value, reflected_card, reflected_field};
use crate::theme;
use egui::{RichText, Ui};

#[derive(Default)]
pub struct ResourcesState {
    pub search: String,
}

pub fn draw_resources(ui: &mut Ui, state: &mut EditorContext<'_>, resources: &mut ResourcesState) {
    theme::toolbar(ui, |ui| {
        ui.label(
            RichText::new("Live resources")
                .strong()
                .color(theme::TEXT_STRONG),
        );
        ui.separator();
        theme::search_field(ui, "resource_search", "type", &mut resources.search);
    });

    let query = resources.search.trim().to_ascii_lowercase();
    let matches = |name: &str| query.is_empty() || name.to_ascii_lowercase().contains(&query);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("resources_body")
        .show(ui, |ui| {
            theme::section(ui, "Global");
            state.global_resources.for_each_reflected_mut(|name, value| {
                if matches(name) {
                    reflected_field(ui, name, value, 0);
                }
            });

            ui.add_space(4.0);
            theme::section(ui, "World");
            let Some(scene) = state.active_scene_mut() else {
                ui.label(RichText::new("No active scene.").color(theme::TEXT_DIM));
                return;
            };

            scene.world_mut().for_each_resource_mut(|type_name, value| {
                let label = display_type_name(type_name);
                if !matches(&label) {
                    return;
                }
                match value {
                    Some(value) => reflected_card(ui, &label, false, |ui| {
                        draw_reflected_value(ui, value, 0);
                    }),
                    None => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(label).color(theme::TEXT));
                            theme::tag(ui, "opaque", theme::TEXT_DIM);
                        });
                    }
                }
            });
        });
}
