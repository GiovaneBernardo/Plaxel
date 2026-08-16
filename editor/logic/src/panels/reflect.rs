//! Generic editor for anything that implements `bevy_reflect`, used by the entity
//! inspector, the resource browser and the render graph node inspector.

use crate::panels::fields::{self, VALUE_WIDTH};
use crate::theme;
use egui::{RichText, Ui, Vec2};
use engine::reflect::{self, PartialReflect, ReflectMut};

const MAX_COLLECTION_ROWS: usize = 64;

pub fn reflected_field(
    ui: &mut Ui,
    label: impl Into<String>,
    value: &mut dyn PartialReflect,
    depth: usize,
) {
    let label = fields::pretty_field_name(&label.into());
    if is_reflected_leaf(value) {
        ui.horizontal(|ui| {
            fields::field_label(ui, &label);
            draw_reflected_value(ui, value, depth + 1);
        });
    } else {
        egui::CollapsingHeader::new(RichText::new(label).color(theme::TEXT))
            .id_salt((value.reflect_type_path(), depth))
            .default_open(depth == 0)
            .show(ui, |ui| draw_reflected_value(ui, value, depth + 1));
    }
}

pub fn is_reflected_leaf(value: &dyn PartialReflect) -> bool {
    value.try_downcast_ref::<bool>().is_some()
        || value.try_downcast_ref::<String>().is_some()
        || value.try_downcast_ref::<f32>().is_some()
        || value.try_downcast_ref::<f64>().is_some()
        || value.try_downcast_ref::<i8>().is_some()
        || value.try_downcast_ref::<i16>().is_some()
        || value.try_downcast_ref::<i32>().is_some()
        || value.try_downcast_ref::<i64>().is_some()
        || value.try_downcast_ref::<u8>().is_some()
        || value.try_downcast_ref::<u16>().is_some()
        || value.try_downcast_ref::<u32>().is_some()
        || value.try_downcast_ref::<u64>().is_some()
        || value.try_downcast_ref::<engine::math::Vec2>().is_some()
        || value.try_downcast_ref::<engine::math::Vec3>().is_some()
        || value.try_downcast_ref::<engine::math::Vec4>().is_some()
        || value.try_downcast_ref::<engine::math::DVec3>().is_some()
        || value.try_downcast_ref::<engine::math::Quat>().is_some()
        || value
            .try_downcast_ref::<engine::reflect::RuntimeCounter>()
            .is_some()
        || matches!(value.reflect_kind(), reflect::ReflectKind::Opaque)
}

pub fn draw_reflected_value(ui: &mut Ui, value: &mut dyn PartialReflect, depth: usize) -> bool {
    macro_rules! drag_number {
        ($ty:ty, $speed:expr) => {
            if let Some(number) = value.try_downcast_mut::<$ty>() {
                return ui
                    .add_sized(
                        [VALUE_WIDTH, 18.0],
                        egui::DragValue::new(number).speed($speed),
                    )
                    .changed();
            }
        };
    }

    if let Some(value) = value.try_downcast_mut::<bool>() {
        return ui.checkbox(value, "").changed();
    }
    if let Some(value) = value.try_downcast_mut::<String>() {
        return ui
            .add_sized([200.0, 18.0], egui::TextEdit::singleline(value))
            .changed();
    }
    drag_number!(f32, 0.01);
    drag_number!(f64, 0.01);
    drag_number!(i8, 1.0);
    drag_number!(i16, 1.0);
    drag_number!(i32, 1.0);
    drag_number!(i64, 1.0);
    drag_number!(u8, 1.0);
    drag_number!(u16, 1.0);
    drag_number!(u32, 1.0);
    drag_number!(u64, 1.0);

    macro_rules! vector_editor {
        ($ty:ty, $($field:ident),+ $(,)?) => {
            if let Some(vector) = value.try_downcast_mut::<$ty>() {
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    $(changed |= ui
                        .add_sized(
                            [VALUE_WIDTH, 18.0],
                            egui::DragValue::new(&mut vector.$field)
                                .speed(0.01)
                                .max_decimals(3)
                                .prefix(concat!(stringify!($field), " ")),
                        )
                        .changed();)+
                });
                return changed;
            }
        };
    }
    vector_editor!(engine::math::Vec2, x, y);
    vector_editor!(engine::math::Vec3, x, y, z);
    vector_editor!(engine::math::Vec4, x, y, z, w);
    vector_editor!(engine::math::DVec3, x, y, z);
    vector_editor!(engine::math::Quat, x, y, z, w);

    if let Some(counter) = value.try_downcast_ref::<engine::reflect::RuntimeCounter>() {
        ui.label(
            RichText::new(counter.get().to_string())
                .monospace()
                .color(theme::TEXT_STRONG),
        );
        return false;
    }

    match value.reflect_mut() {
        ReflectMut::Struct(value) => {
            for index in 0..value.field_len() {
                let name = value.name_at(index).unwrap_or("field").to_string();
                if let Some(field) = value.field_at_mut(index) {
                    reflected_field(ui, name, field, depth);
                }
            }
            true
        }
        ReflectMut::TupleStruct(value) => {
            for index in 0..value.field_len() {
                if let Some(field) = value.field_mut(index) {
                    reflected_field(ui, index.to_string(), field, depth);
                }
            }
            true
        }
        ReflectMut::Tuple(value) => {
            for index in 0..value.field_len() {
                if let Some(field) = value.field_mut(index) {
                    reflected_field(ui, index.to_string(), field, depth);
                }
            }
            true
        }
        ReflectMut::List(value) => {
            let length = value.len();
            for index in 0..length.min(MAX_COLLECTION_ROWS) {
                if let Some(item) = value.get_mut(index) {
                    reflected_field(ui, format!("[{index}]"), item, depth);
                }
            }
            overflow_note(ui, length);
            true
        }
        ReflectMut::Array(value) => {
            let length = value.len();
            for index in 0..length.min(MAX_COLLECTION_ROWS) {
                if let Some(item) = value.get_mut(index) {
                    reflected_field(ui, format!("[{index}]"), item, depth);
                }
            }
            overflow_note(ui, length);
            true
        }
        ReflectMut::Enum(value) => {
            let current_variant = value.variant_name().to_string();
            let unit_variants = value
                .get_represented_enum_info()
                .map(|info| {
                    (0..info.variant_len())
                        .filter_map(|index| info.variant_at(index))
                        .filter(|variant| matches!(variant, reflect::enums::VariantInfo::Unit(_)))
                        .map(|variant| variant.name())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if unit_variants.len() > 1 {
                egui::ComboBox::from_id_salt((value.reflect_type_path(), depth))
                    .selected_text(&current_variant)
                    .show_ui(ui, |ui| {
                        for variant in unit_variants {
                            if ui
                                .selectable_label(current_variant == variant, variant)
                                .clicked()
                            {
                                let patch = reflect::enums::DynamicEnum::new(variant, ());
                                let _ = value.try_apply(&patch);
                            }
                        }
                    });
            } else {
                ui.label(
                    RichText::new(&current_variant)
                        .monospace()
                        .color(theme::TEXT_STRONG),
                );
            }
            for index in 0..value.field_len() {
                let name = value
                    .name_at(index)
                    .map(str::to_string)
                    .unwrap_or_else(|| index.to_string());
                if let Some(field) = value.field_at_mut(index) {
                    reflected_field(ui, name, field, depth);
                }
            }
            true
        }
        ReflectMut::Map(value) => {
            let length = value.len();
            ui.label(RichText::new(format!("{length} entries")).color(theme::TEXT_DIM));
            for (index, (key, entry)) in value.iter().take(MAX_COLLECTION_ROWS).enumerate() {
                theme::truncated(ui, format!("[{index}] {key:?}: {entry:?}"), theme::TEXT);
            }
            overflow_note(ui, length);
            false
        }
        ReflectMut::Set(value) => {
            let length = value.len();
            ui.label(RichText::new(format!("{length} entries")).color(theme::TEXT_DIM));
            for (index, entry) in value.iter().take(MAX_COLLECTION_ROWS).enumerate() {
                theme::truncated(ui, format!("[{index}] {entry:?}"), theme::TEXT);
            }
            overflow_note(ui, length);
            false
        }
        ReflectMut::Opaque(value) => {
            ui.label(
                RichText::new(format!(
                    "{} (opaque)",
                    fields::display_type_name(value.reflect_type_path())
                ))
                .italics()
                .color(theme::TEXT_DIM),
            );
            false
        }
    }
}

/// Card with a component/resource name that keeps its open state per type.
pub fn reflected_card(ui: &mut Ui, title: &str, default_open: bool, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(theme::BG_SURFACE)
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(8, 6))
        .outer_margin(egui::Margin {
            bottom: 4,
            ..egui::Margin::ZERO
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::CollapsingHeader::new(RichText::new(title).strong().color(theme::TEXT_STRONG))
                .id_salt(title)
                .default_open(default_open)
                .show_unindented(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 3.0);
                    add(ui);
                });
        });
}

fn overflow_note(ui: &mut Ui, length: usize) {
    if length > MAX_COLLECTION_ROWS {
        ui.label(
            RichText::new(format!("… {} more", length - MAX_COLLECTION_ROWS))
                .small()
                .color(theme::TEXT_DIM),
        );
    }
}
