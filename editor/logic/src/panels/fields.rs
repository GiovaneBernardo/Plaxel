//! Label/value rows shared by every inspector style panel.

use crate::theme;
use egui::{RichText, Ui, Vec2};

pub const LABEL_WIDTH: f32 = 118.0;
pub const VALUE_WIDTH: f32 = 74.0;

pub fn inspector_grid<R>(
    ui: &mut Ui,
    id: impl std::hash::Hash,
    add_rows: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing(Vec2::new(10.0, 4.0))
        .striped(false)
        .show(ui, add_rows)
}

pub fn field_label(ui: &mut Ui, label: &str) {
    ui.allocate_ui_with_layout(
        Vec2::new(LABEL_WIDTH, 18.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.add(
                egui::Label::new(RichText::new(label).color(theme::TEXT_DIM))
                    .truncate()
                    .selectable(false),
            )
            .on_hover_text(label);
        },
    );
}

pub fn drag_value(ui: &mut Ui, value: &mut f32, speed: f64, prefix: &'static str) -> bool {
    let speed = speed.max(f64::from(value.abs()) * 0.01);
    ui.add_sized(
        [VALUE_WIDTH, 18.0],
        egui::DragValue::new(value)
            .speed(speed)
            .prefix(prefix)
            .max_decimals(3),
    )
    .changed()
}

pub fn scalar_row(ui: &mut Ui, label: &str, value: &mut f32, speed: f64) {
    field_label(ui, label);
    drag_value(ui, value, speed, "");
    ui.end_row();
}

pub fn readonly_row(ui: &mut Ui, label: &str, value: &str) {
    field_label(ui, label);
    theme::truncated(ui, value, theme::TEXT);
    ui.end_row();
}

pub fn text_row(ui: &mut Ui, label: &str, value: &mut String) {
    field_label(ui, label);
    ui.add_sized([220.0, 18.0], egui::TextEdit::singleline(value));
    ui.end_row();
}

pub fn u32_row(ui: &mut Ui, label: &str, value: &mut u32) {
    field_label(ui, label);
    ui.add_sized([VALUE_WIDTH, 18.0], egui::DragValue::new(value).speed(1.0));
    ui.end_row();
}

pub fn int_row(ui: &mut Ui, label: &str, value: &mut i32) {
    field_label(ui, label);
    ui.add_sized([VALUE_WIDTH, 18.0], egui::DragValue::new(value).speed(1.0));
    ui.end_row();
}

pub fn bool_row(ui: &mut Ui, label: &str, value: &mut bool) {
    field_label(ui, label);
    ui.checkbox(value, "");
    ui.end_row();
}

pub fn float_array_row<const N: usize>(ui: &mut Ui, label: &str, value: &mut [f32; N]) {
    field_label(ui, label);
    ui.horizontal(|ui| {
        for item in value {
            drag_value(ui, item, 0.01, "");
        }
    });
    ui.end_row();
}

/// `some_field_name` -> `Some Field Name`
pub fn pretty_field_name(name: &str) -> String {
    let mut result = String::new();
    for (index, part) in name.split('_').filter(|part| !part.is_empty()).enumerate() {
        if index > 0 {
            result.push(' ');
        }
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            result.push(first.to_ascii_uppercase());
            result.extend(chars);
        }
    }
    (!result.is_empty())
        .then_some(result)
        .unwrap_or_else(|| name.to_string())
}

/// `engine::core::components::TransformComponent` -> `TransformComponent`
pub fn display_type_name(type_name: &str) -> String {
    type_name
        .rsplit("::")
        .next()
        .unwrap_or(type_name)
        .trim_end_matches('>')
        .to_string()
}
