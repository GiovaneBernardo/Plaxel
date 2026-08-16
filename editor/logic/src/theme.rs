//! Palette, egui style and the small widgets every editor panel is built from.

use egui::{
    Color32, CornerRadius, FontId, Margin, Response, RichText, Stroke, TextStyle, Ui, Vec2,
};

pub const BG_DEEP: Color32 = Color32::from_rgb(11, 13, 17);
pub const BG_PANEL: Color32 = Color32::from_rgb(19, 22, 28);
pub const BG_SURFACE: Color32 = Color32::from_rgb(25, 29, 36);
pub const BG_RAISED: Color32 = Color32::from_rgb(34, 40, 49);
pub const BG_HOVER: Color32 = Color32::from_rgb(45, 53, 64);
pub const BORDER: Color32 = Color32::from_rgb(40, 46, 56);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(56, 65, 78);

pub const ACCENT: Color32 = Color32::from_rgb(88, 152, 255);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(46, 88, 154);

pub const TEXT_STRONG: Color32 = Color32::from_rgb(228, 236, 245);
pub const TEXT: Color32 = Color32::from_rgb(190, 201, 214);
pub const TEXT_DIM: Color32 = Color32::from_rgb(132, 145, 161);

pub const SUCCESS: Color32 = Color32::from_rgb(94, 210, 130);
pub const WARN: Color32 = Color32::from_rgb(228, 180, 76);
pub const ERROR: Color32 = Color32::from_rgb(240, 110, 100);

pub const TOOLBAR_HEIGHT: f32 = 38.0;
pub const STATUS_BAR_HEIGHT: f32 = 24.0;
pub const ROW_HEIGHT: f32 = 20.0;

pub fn apply_editor_style(ctx: &egui::Context) {
    ctx.set_global_style(editor_style());
}

/// The editor's egui style. Built from scratch so the dock style can be derived from
/// the exact same values without waiting for a context to exist.
pub fn editor_style() -> egui::Style {
    let mut style = egui::Style::default();

    style.spacing.item_spacing = Vec2::new(6.0, 5.0);
    style.spacing.button_padding = Vec2::new(8.0, 3.0);
    style.spacing.menu_margin = Margin::same(6);
    style.spacing.window_margin = Margin::same(8);
    style.spacing.indent = 16.0;
    style.spacing.interact_size.y = 20.0;
    style.spacing.scroll.bar_width = 9.0;
    style.spacing.scroll.floating = false;

    style
        .text_styles
        .insert(TextStyle::Heading, FontId::proportional(15.0));
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(12.5));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(12.5));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(10.5));
    style
        .text_styles
        .insert(TextStyle::Monospace, FontId::monospace(11.5));

    let mut visuals = egui::Visuals::dark();
    visuals.dark_mode = true;
    visuals.panel_fill = BG_PANEL;
    visuals.window_fill = BG_SURFACE;
    visuals.extreme_bg_color = BG_DEEP;
    visuals.faint_bg_color = Color32::from_rgb(28, 33, 40);
    visuals.code_bg_color = BG_DEEP;
    // No `override_text_color`: widget states (disabled, hovered) must keep tinting
    // their own text.
    visuals.warn_fg_color = WARN;
    visuals.error_fg_color = ERROR;
    visuals.hyperlink_color = ACCENT;
    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.menu_corner_radius = CornerRadius::same(6);
    visuals.window_stroke = Stroke::new(1.0, BORDER_STRONG);
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(120),
    };
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(110),
    };
    visuals.selection.bg_fill = ACCENT_DIM;
    visuals.selection.stroke = Stroke::new(1.0, TEXT_STRONG);
    visuals.striped = true;
    visuals.indent_has_left_vline = true;
    visuals.collapsing_header_frame = false;
    visuals.slider_trailing_fill = true;

    let radius = CornerRadius::same(5);
    visuals.widgets.noninteractive.bg_fill = BG_SURFACE;
    visuals.widgets.noninteractive.weak_bg_fill = BG_SURFACE;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.noninteractive.corner_radius = radius;

    visuals.widgets.inactive.bg_fill = BG_RAISED;
    visuals.widgets.inactive.weak_bg_fill = BG_RAISED;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.inactive.corner_radius = radius;

    visuals.widgets.hovered.bg_fill = BG_HOVER;
    visuals.widgets.hovered.weak_bg_fill = BG_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_STRONG);
    visuals.widgets.hovered.corner_radius = radius;

    visuals.widgets.active.bg_fill = ACCENT_DIM;
    visuals.widgets.active.weak_bg_fill = ACCENT_DIM;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT_STRONG);
    visuals.widgets.active.corner_radius = radius;

    visuals.widgets.open.bg_fill = BG_HOVER;
    visuals.widgets.open.weak_bg_fill = BG_HOVER;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, TEXT_STRONG);
    visuals.widgets.open.corner_radius = radius;

    style.visuals = visuals;
    style
}

pub fn dock_style() -> egui_dock::Style {
    let mut dock = egui_dock::Style::from_egui(&editor_style());

    dock.dock_area_padding = None;
    dock.main_surface_border_stroke = Stroke::NONE;
    dock.main_surface_border_rounding = CornerRadius::ZERO;

    dock.separator.width = 1.0;
    dock.separator.extra_interact_width = 5.0;
    dock.separator.color_idle = BG_DEEP;
    dock.separator.color_hovered = BORDER_STRONG;
    dock.separator.color_dragged = ACCENT;

    dock.tab_bar.bg_fill = BG_DEEP;
    dock.tab_bar.height = 26.0;
    dock.tab_bar.hline_color = BORDER;
    dock.tab_bar.corner_radius = CornerRadius::ZERO;
    dock.tab_bar.inner_margin = Margin::ZERO;

    // Square tabs sit flush against each other instead of leaving gaps between the
    // rounded corners of neighbours.
    dock.tab.spacing = 0.0;
    dock.tab.minimum_width = Some(72.0);
    dock.tab.hline_below_active_tab_name = false;
    dock.tab.tab_body.inner_margin = Margin::same(8);
    dock.tab.tab_body.bg_fill = BG_PANEL;
    dock.tab.tab_body.stroke = Stroke::NONE;
    dock.tab.tab_body.corner_radius = CornerRadius::ZERO;

    // Flush square tabs need an edge to tell neighbours apart; the dock repaints the
    // bottom edge with the tab fill, so the active tab still merges into its body.
    for (tab, fill, text, outline) in [
        (&mut dock.tab.active, BG_PANEL, TEXT_STRONG, BORDER),
        (&mut dock.tab.focused, BG_PANEL, TEXT_STRONG, BORDER),
        (&mut dock.tab.inactive, BG_DEEP, TEXT_DIM, BORDER),
        (&mut dock.tab.hovered, BG_SURFACE, TEXT_STRONG, BORDER_STRONG),
        (&mut dock.tab.active_with_kb_focus, BG_PANEL, TEXT_STRONG, BORDER),
        (&mut dock.tab.focused_with_kb_focus, BG_PANEL, TEXT_STRONG, BORDER),
        (&mut dock.tab.inactive_with_kb_focus, BG_DEEP, TEXT_DIM, BORDER),
    ] {
        tab.bg_fill = fill;
        tab.text_color = text;
        tab.outline_color = outline;
        tab.corner_radius = CornerRadius::ZERO;
    }

    dock.buttons.close_tab_color = TEXT_DIM;
    dock.buttons.close_tab_active_color = ERROR;
    dock.buttons.close_tab_bg_fill = BG_RAISED;
    dock.buttons.add_tab_color = TEXT_DIM;
    dock.buttons.add_tab_active_color = TEXT_STRONG;
    dock.buttons.add_tab_bg_fill = BG_RAISED;

    dock.overlay.selection_color = ACCENT.gamma_multiply(0.35);

    dock
}

/// Strip at the top of a panel holding its actions. Keeps every panel aligned.
pub fn toolbar<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let inner = egui::Frame::new()
        .fill(BG_SURFACE)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(22.0);
                ui.set_min_width(ui.available_width());
                add(ui)
            })
            .inner
        })
        .inner;
    ui.add_space(6.0);
    inner
}

/// Small dim heading used to separate blocks inside a panel.
pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(2.0);
    ui.label(
        RichText::new(title.to_uppercase())
            .size(10.0)
            .strong()
            .color(TEXT_DIM),
    );
    ui.add_space(2.0);
}

/// Bordered container for a logical group of fields.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(BG_SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::same(8))
        .show(ui, add)
        .inner
}

/// Label + value pill used for metrics.
pub fn stat(ui: &mut Ui, label: &str, value: impl Into<String>, color: Color32) -> Response {
    egui::Frame::new()
        .fill(BG_SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(label).size(9.5).color(TEXT_DIM));
                ui.label(
                    RichText::new(value.into())
                        .monospace()
                        .strong()
                        .color(color),
                );
            });
        })
        .response
}

/// Compact colored badge, e.g. log levels or state flags.
pub fn tag(ui: &mut Ui, text: &str, color: Color32) -> Response {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.18))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(5, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(10.0).strong().color(color));
        })
        .response
}

pub fn search_field(ui: &mut Ui, id: &str, hint: &str, query: &mut String) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(query)
                .id_salt(id)
                .hint_text(hint)
                .desired_width(150.0),
        );
        changed = response.changed();
        if !query.is_empty() && ui.small_button("✖").clicked() {
            query.clear();
            changed = true;
        }
    });
    changed
}

pub fn empty_state(ui: &mut Ui, icon: &str, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(28.0);
        ui.label(RichText::new(icon).size(26.0).color(BORDER_STRONG));
        ui.add_space(4.0);
        ui.label(RichText::new(text).color(TEXT_DIM));
    });
}

/// Single line label that shrinks instead of wrapping, with the full text on hover.
pub fn truncated(ui: &mut Ui, text: impl Into<String>, color: Color32) -> Response {
    let text = text.into();
    ui.add(egui::Label::new(RichText::new(&text).color(color)).truncate())
        .on_hover_text(text)
}

/// Fixed width truncated label. Table columns keep their width even when the text
/// behind them changes every refresh.
pub fn truncated_sized(ui: &mut Ui, width: f32, text: impl Into<String>, color: Color32) {
    let text = text.into();
    ui.allocate_ui_with_layout(
        Vec2::new(width, 16.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.add(egui::Label::new(RichText::new(&text).color(color)).truncate())
                .on_hover_text(text);
        },
    );
}

/// Horizontal meter painted directly, so a whole grid of them stays cheap.
pub fn meter(ui: &mut Ui, fraction: f32, width: f32, text: &str, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 14.0), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let painter = ui.painter();
    let radius = CornerRadius::same(3);
    painter.rect_filled(rect, radius, BG_DEEP);

    let fraction = fraction.clamp(0.0, 1.0);
    if fraction > 0.0 {
        let mut filled = rect;
        filled.set_width((rect.width() * fraction).max(2.0));
        painter.rect_filled(filled, radius, color);
    }

    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        FontId::monospace(10.0),
        TEXT_STRONG,
    );
    response
}

/// Color ramp for load style values: calm when low, hot when saturated.
pub fn load_color(fraction: f32) -> Color32 {
    match fraction {
        f if f >= 0.90 => ERROR,
        f if f >= 0.65 => WARN,
        f if f >= 0.25 => ACCENT,
        _ => Color32::from_rgb(60, 96, 148),
    }
}

pub fn ms_color(ms: f64) -> Color32 {
    match ms {
        value if value > 33.3 => ERROR,
        value if value > 16.7 => WARN,
        _ => SUCCESS,
    }
}
