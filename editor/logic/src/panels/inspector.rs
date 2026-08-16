//! Component inspector for the entity selected in the hierarchy.

use crate::EditorContext;
use crate::panels::fields::display_type_name;
use crate::panels::hierarchy::default_transform;
use crate::panels::reflect::{draw_reflected_value, reflected_card};
use crate::theme;
use egui::{RichText, Ui};
use engine::ecs::entity::Entity;

pub fn draw_inspector(
    ui: &mut Ui,
    state: &mut EditorContext<'_>,
    selected_entity: &mut Option<Entity>,
) {
    let Some(entity) = *selected_entity else {
        theme::empty_state(ui, "◈", "Select an entity in the hierarchy.");
        return;
    };

    let Some(scene) = state.active_scene_mut() else {
        theme::empty_state(ui, "◌", "No active scene.");
        return;
    };
    let world = scene.world_mut();

    theme::toolbar(ui, |ui| {
        ui.label(
            RichText::new(format!("◈ Entity {}", entity.index()))
                .strong()
                .color(theme::TEXT_STRONG),
        );
        theme::tag(ui, &format!("gen {}", entity.generation()), theme::TEXT_DIM);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(RichText::new("Despawn").color(theme::ERROR))
                .clicked()
                && world.despawn(entity)
            {
                *selected_entity = None;
            }
            if ui.button("➕ Transform").clicked() {
                world.insert(entity, default_transform());
            }
        });
    });

    if selected_entity.is_none() {
        return;
    }

    let mut component_count = 0;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            world.for_each_reflected_component_mut(entity, |type_name, value| {
                component_count += 1;
                reflected_card(ui, &display_type_name(type_name), true, |ui| {
                    draw_reflected_value(ui, value, 0);
                });
            });

            if component_count == 0 {
                theme::empty_state(ui, "◌", "This entity has no reflected components.");
            }
        });
}
