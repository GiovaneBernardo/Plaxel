//! Entity list. Rows are virtualized, so a world with a hundred thousand entities
//! costs the same to draw as one with fifty.

use crate::EditorContext;
use crate::panels::fields::display_type_name;
use crate::theme;
use egui::{RichText, Ui};
use engine::core::components::core::TransformComponent;
use engine::ecs::entity::Entity;

#[derive(Default)]
pub struct HierarchyState {
    pub search: String,
    entities: Vec<Entity>,
    components: String,
    described: Option<Entity>,
}

pub fn draw_hierarchy(
    ui: &mut Ui,
    state: &mut EditorContext<'_>,
    hierarchy: &mut HierarchyState,
    selected_entity: &mut Option<Entity>,
) {
    let alive = state
        .active_scene()
        .map(|scene| scene.world().entities().alive_count())
        .unwrap_or(0);

    theme::toolbar(ui, |ui| {
        if ui.button("➕ Entity").clicked()
            && let Some(scene) = state.active_scene_mut()
        {
            *selected_entity = Some(scene.world_mut().spawn());
        }
        if ui
            .button("Spawn 100")
            .on_hover_text("Spawn 100 entities with a default transform")
            .clicked()
            && let Some(scene) = state.active_scene_mut()
        {
            let world = scene.world_mut();
            for _ in 0..100 {
                let entity = world.spawn();
                world.insert(entity, default_transform());
            }
        }
        ui.separator();
        theme::search_field(ui, "hierarchy_search", "index", &mut hierarchy.search);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(format!("{alive}")).monospace().color(theme::TEXT_DIM));
        });
    });

    let Some(scene) = state.active_scene() else {
        theme::empty_state(ui, "◌", "No active scene.");
        return;
    };

    let world = scene.world();
    // Swapped out of the state so the cached component summary can still be updated
    // while the row list is borrowed.
    let mut entities = std::mem::take(&mut hierarchy.entities);
    entities.clear();
    if hierarchy.search.is_empty() {
        entities.extend(world.entities().iter_alive());
    } else {
        let query = hierarchy.search.trim();
        entities.extend(
            world
                .entities()
                .iter_alive()
                .filter(|entity| entity.index().to_string().contains(query)),
        );
    }

    if entities.is_empty() {
        hierarchy.entities = entities;
        theme::empty_state(ui, "◌", "No entities match.");
        return;
    }

    let row_height = theme::ROW_HEIGHT;
    let mut clicked = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, row_height, entities.len(), |ui, range| {
            for entity in &entities[range] {
                let entity = *entity;
                let selected = *selected_entity == Some(entity);
                let response = ui.add(
                    egui::Button::selectable(
                        selected,
                        RichText::new(format!("◈ {}:{}", entity.index(), entity.generation()))
                            .monospace()
                            .color(if selected {
                                theme::TEXT_STRONG
                            } else {
                                theme::TEXT
                            }),
                    )
                    // Fixed height keeps the virtualized rows aligned with the
                    // scroll offset.
                    .min_size(egui::vec2(ui.available_width(), row_height)),
                );
                if response.clicked() {
                    clicked = Some(entity);
                }
                if response.hovered() {
                    let summary = component_summary(world, entity, hierarchy);
                    response.on_hover_text(summary);
                }
            }
        });

    hierarchy.entities = entities;
    if let Some(entity) = clicked {
        *selected_entity = Some(entity);
    }
}

/// Component names are only resolved for the row under the cursor: listing them for
/// every entity would walk all storages every frame.
fn component_summary(
    world: &engine::ecs::world::World,
    entity: Entity,
    hierarchy: &mut HierarchyState,
) -> String {
    if hierarchy.described != Some(entity) {
        let mut names = Vec::new();
        world.for_each_reflected_component_mut(entity, |type_name, _| {
            names.push(display_type_name(type_name));
        });
        hierarchy.components = if names.is_empty() {
            "no reflected components".to_string()
        } else {
            names.join("\n")
        };
        hierarchy.described = Some(entity);
    }
    hierarchy.components.clone()
}

pub fn default_transform() -> TransformComponent {
    TransformComponent {
        position: engine::math::vec3(0.0, 10.0, 0.0),
        rotation: engine::math::Quat::IDENTITY,
        scale: engine::math::vec3(1.0, 1.0, 1.0),
        velocity: engine::math::vec3(0.0, 0.0, 0.0),
    }
}
