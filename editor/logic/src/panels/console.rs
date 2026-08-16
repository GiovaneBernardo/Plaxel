//! Log view. Entries are pulled from the engine only when the log actually changed,
//! because fetching them touches the console log file.

use crate::theme;
use egui::{Color32, RichText, Ui};
use engine::logging::{ConsoleEntry, ConsoleLevel};

const LEVELS: [ConsoleLevel; 6] = [
    ConsoleLevel::Trace,
    ConsoleLevel::Debug,
    ConsoleLevel::Info,
    ConsoleLevel::Warn,
    ConsoleLevel::Error,
    ConsoleLevel::Panic,
];
/// Lower bound between two fetches, so a burst of logging cannot stall the UI.
const REFRESH_INTERVAL: f64 = 0.25;

pub struct ConsoleState {
    entries: Vec<ConsoleEntry>,
    visible: Vec<usize>,
    counts: [usize; 6],
    revision: u64,
    refreshed_at: f64,
    search: String,
    enabled_levels: [bool; 6],
    autoscroll: bool,
    filter_dirty: bool,
}

impl ConsoleState {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            visible: Vec::new(),
            counts: [0; 6],
            revision: u64::MAX,
            refreshed_at: f64::NEG_INFINITY,
            search: String::new(),
            enabled_levels: [false, true, true, true, true, true],
            autoscroll: true,
            filter_dirty: true,
        }
    }

    fn refresh(&mut self, now: f64) {
        let revision = engine::logging::console_revision();
        if revision == self.revision || now - self.refreshed_at < REFRESH_INTERVAL {
            return;
        }

        self.entries = engine::logging::console_entries();
        self.revision = revision;
        self.refreshed_at = now;
        self.counts = [0; 6];
        for entry in &self.entries {
            self.counts[level_index(entry.level)] += 1;
        }
        self.filter_dirty = true;
    }

    fn rebuild_filter(&mut self) {
        if !self.filter_dirty {
            return;
        }
        self.filter_dirty = false;

        let query = self.search.trim().to_ascii_lowercase();
        self.visible.clear();
        self.visible.extend(
            self.entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| self.enabled_levels[level_index(entry.level)])
                .filter(|(_, entry)| {
                    query.is_empty()
                        || entry.message.to_ascii_lowercase().contains(&query)
                        || entry.target.to_ascii_lowercase().contains(&query)
                })
                .map(|(index, _)| index),
        );
    }
}

pub fn draw_console(ui: &mut Ui, console: &mut ConsoleState) {
    console.refresh(ui.input(|input| input.time));

    theme::toolbar(ui, |ui| {
        for (index, level) in LEVELS.iter().enumerate() {
            let count = console.counts[index];
            let label = RichText::new(format!("{} {count}", level_label(*level)))
                .size(10.5)
                .color(if console.enabled_levels[index] {
                    level_color(*level)
                } else {
                    theme::TEXT_DIM
                });
            if ui
                .selectable_label(console.enabled_levels[index], label)
                .clicked()
            {
                console.enabled_levels[index] = !console.enabled_levels[index];
                console.filter_dirty = true;
            }
        }
        ui.separator();
        if theme::search_field(ui, "console_search", "filter", &mut console.search) {
            console.filter_dirty = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Clear").clicked() {
                engine::logging::clear_console_entries();
                console.refreshed_at = f64::NEG_INFINITY;
            }
            ui.toggle_value(&mut console.autoscroll, "Follow");
        });
    });

    console.rebuild_filter();

    if console.visible.is_empty() {
        theme::empty_state(ui, "◌", "No log entries match.");
        return;
    }

    let entries = &console.entries;
    let visible = &console.visible;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(console.autoscroll)
        .show_rows(ui, theme::ROW_HEIGHT, visible.len(), |ui, range| {
            for row in range {
                let entry = &entries[visible[row]];
                // Rows are allocated at a fixed height so the virtualized scroll
                // offset matches what is actually drawn.
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), theme::ROW_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(
                            RichText::new(format!("{:>5}", entry.sequence))
                                .monospace()
                                .size(10.5)
                                .color(theme::TEXT_DIM),
                        );
                        ui.label(
                            RichText::new(level_label(entry.level))
                                .monospace()
                                .size(10.5)
                                .color(level_color(entry.level)),
                        );
                        ui.label(
                            RichText::new(&entry.target)
                                .size(10.5)
                                .color(theme::ACCENT.gamma_multiply(0.8)),
                        );
                        theme::truncated(ui, &entry.message, level_text_color(entry.level));
                    },
                );
            }
        });
}

fn level_index(level: ConsoleLevel) -> usize {
    match level {
        ConsoleLevel::Trace => 0,
        ConsoleLevel::Debug => 1,
        ConsoleLevel::Info => 2,
        ConsoleLevel::Warn => 3,
        ConsoleLevel::Error => 4,
        ConsoleLevel::Panic => 5,
    }
}

fn level_label(level: ConsoleLevel) -> &'static str {
    match level {
        ConsoleLevel::Trace => "TRC",
        ConsoleLevel::Debug => "DBG",
        ConsoleLevel::Info => "INF",
        ConsoleLevel::Warn => "WRN",
        ConsoleLevel::Error => "ERR",
        ConsoleLevel::Panic => "PNC",
    }
}

fn level_color(level: ConsoleLevel) -> Color32 {
    match level {
        ConsoleLevel::Trace => Color32::from_rgb(120, 132, 148),
        ConsoleLevel::Debug => Color32::from_rgb(140, 165, 195),
        ConsoleLevel::Info => theme::SUCCESS,
        ConsoleLevel::Warn => theme::WARN,
        ConsoleLevel::Error | ConsoleLevel::Panic => theme::ERROR,
    }
}

fn level_text_color(level: ConsoleLevel) -> Color32 {
    match level {
        ConsoleLevel::Warn => theme::WARN,
        ConsoleLevel::Error | ConsoleLevel::Panic => theme::ERROR,
        _ => theme::TEXT,
    }
}
