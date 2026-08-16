//! Render graph browser: the pass chain across the top, the selected pass below it.

use crate::EditorContext;
use crate::panels::fields::{inspector_grid, readonly_row};
use crate::panels::reflect::draw_reflected_value;
use crate::theme;
use egui::{RichText, Ui, Vec2};
use engine::renderer::ids::GraphPassId;

struct RenderNodeSummary {
    index: GraphPassId,
    name: &'static str,
    enabled: bool,
    input_textures: Vec<&'static str>,
    output_textures: Vec<String>,
    color_attachments: Vec<&'static str>,
    depth_attachment: Option<&'static str>,
}

pub fn draw_render_graph(
    ui: &mut Ui,
    state: &mut EditorContext<'_>,
    selected_render_node: &mut Option<GraphPassId>,
) {
    let summaries = render_node_summaries(state);

    theme::toolbar(ui, |ui| {
        ui.label(
            RichText::new("Render graph")
                .strong()
                .color(theme::TEXT_STRONG),
        );
        theme::tag(ui, &format!("{} passes", summaries.len()), theme::ACCENT);
        if !state.global_resources.renderer.render_graph.compiled {
            theme::tag(ui, "not compiled", theme::WARN);
        }
    });

    if summaries.is_empty() {
        theme::empty_state(ui, "◌", "No render nodes.");
        return;
    }

    if selected_render_node
        .is_none_or(|selected| !summaries.iter().any(|summary| summary.index == selected))
    {
        *selected_render_node = summaries.first().map(|summary| summary.index);
    }

    // Execution order reads left to right, like the graph itself.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
        for (position, summary) in summaries.iter().enumerate() {
            if position > 0 {
                ui.label(RichText::new("→").color(theme::TEXT_DIM));
            }
            let selected = *selected_render_node == Some(summary.index);
            let dot = if summary.enabled { "◉" } else { "○" };
            let color = match (summary.enabled, selected) {
                (true, true) => theme::TEXT_STRONG,
                (true, false) => theme::TEXT,
                (false, _) => theme::TEXT_DIM,
            };
            let response = ui
                .selectable_label(
                    selected,
                    RichText::new(format!("{dot} {}", summary.name)).color(color),
                )
                .on_hover_text(format!("pass id {:#x}", summary.index.0));
            if response.clicked() {
                *selected_render_node = Some(summary.index);
            }
        }
    });

    ui.add_space(6.0);
    if let Some(index) = *selected_render_node {
        draw_render_node_inspector(ui, state, index, &summaries);
    }
}

fn render_node_summaries(state: &EditorContext<'_>) -> Vec<RenderNodeSummary> {
    let graph = &state.global_resources.renderer.render_graph;
    graph
        .nodes
        .iter()
        .map(|(index, node)| {
            let descriptor = node.describe_pass();
            RenderNodeSummary {
                index: *index,
                name: descriptor.name,
                enabled: graph.is_node_enabled(*index),
                input_textures: descriptor.input_textures,
                output_textures: descriptor
                    .output_textures
                    .into_iter()
                    .map(output_texture_label)
                    .collect(),
                color_attachments: descriptor
                    .color_attachments
                    .iter()
                    .map(|attachment| attachment.name)
                    .collect(),
                depth_attachment: descriptor
                    .depth_attachment
                    .map(|attachment| attachment.name),
            }
        })
        .collect()
}

fn draw_render_node_inspector(
    ui: &mut Ui,
    state: &mut EditorContext<'_>,
    index: GraphPassId,
    summaries: &[RenderNodeSummary],
) {
    let Some(summary) = summaries.iter().find(|summary| summary.index == index) else {
        return;
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("render_node_inspector")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(summary.name)
                        .strong()
                        .color(theme::TEXT_STRONG),
                );
                let can_disable = summary.index != crate::EGUI_NODE_INDEX;
                let mut enabled = summary.enabled;
                let checkbox = ui.add_enabled(
                    can_disable,
                    egui::Checkbox::new(&mut enabled, "enabled"),
                );
                if checkbox.changed() {
                    state
                        .global_resources
                        .renderer
                        .render_graph
                        .set_node_enabled(summary.index, enabled);
                }
                checkbox.on_disabled_hover_text("The editor UI pass stays enabled.");
            });

            // Attachments are short lists, so two columns of pairs use the width
            // instead of leaving half the panel empty.
            let column_width = ((ui.available_width() - 12.0) * 0.5).max(160.0);
            ui.horizontal_top(|ui| {
                ui.allocate_ui(Vec2::new(column_width, 0.0), |ui| {
                    theme::card(ui, |ui| {
                        ui.set_min_width(column_width - 20.0);
                        inspector_grid(ui, format!("render_node_io_{index}"), |ui| {
                            readonly_row(
                                ui,
                                "Inputs",
                                &comma_list(summary.input_textures.iter().copied()),
                            );
                            readonly_row(
                                ui,
                                "Outputs",
                                &comma_list(summary.output_textures.iter().map(String::as_str)),
                            );
                        });
                    });
                });
                ui.allocate_ui(Vec2::new(column_width, 0.0), |ui| {
                    theme::card(ui, |ui| {
                        ui.set_min_width(column_width - 20.0);
                        inspector_grid(ui, format!("render_node_attachments_{index}"), |ui| {
                            readonly_row(
                                ui,
                                "Color",
                                &comma_list(summary.color_attachments.iter().copied()),
                            );
                            readonly_row(ui, "Depth", summary.depth_attachment.unwrap_or("none"));
                        });
                    });
                });
            });

            ui.add_space(4.0);
            theme::section(ui, "Uniforms");
            let graph = &mut state.global_resources.renderer.render_graph;
            let Some((_, node)) = graph
                .nodes
                .iter_mut()
                .find(|(node_index, _)| *node_index == index)
            else {
                return;
            };

            match node.reflect_mut() {
                Some(value) => {
                    draw_reflected_value(ui, value, 0);
                }
                None => {
                    ui.label(
                        RichText::new("This pass exposes no editable uniforms yet.")
                            .color(theme::TEXT_DIM),
                    );
                }
            }
        });
}

fn output_texture_label(output: engine::renderer::OutputTexture) -> String {
    match output {
        engine::renderer::OutputTexture::Create(slot) => format!("create {}", slot.name),
        engine::renderer::OutputTexture::WriteTo(name) => format!("write {name}"),
    }
}

fn comma_list<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    let text = items.into_iter().collect::<Vec<_>>().join(", ");
    if text.is_empty() {
        "none".to_string()
    } else {
        text
    }
}
