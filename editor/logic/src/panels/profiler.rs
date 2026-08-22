//! Frame profiler.
//!
//! The engine publishes a new snapshot at the rate set by
//! `engine::profiling::set_snapshot_interval` and hands it out as an `Arc`, so the
//! panel neither copies samples per frame nor flickers at the frame rate. Everything
//! derived from a snapshot is rebuilt only when the `Arc` actually changes, which also
//! makes pausing exact: a frozen `Arc` means frozen rows, including their order.

use crate::EditorContext;
use crate::theme;
use egui::{Color32, RichText, Ui, Vec2};
use engine::profiling::ProfileSnapshot;
use std::sync::Arc;

const TOP_ACTIVITIES: usize = 3;
/// Utilization is smoothed towards the newest sample so the bars read as a level
/// instead of a strobe.
const SMOOTHING: f32 = 0.35;

pub struct ProfilerState {
    paused: bool,
    frozen: Option<Arc<ProfileSnapshot>>,
    view: ProfilerView,
    #[cfg_attr(not(feature = "puffin-ui"), allow(dead_code))]
    show_puffin: bool,
    expanded_overlay: bool,
    frame_time_threshold_ms: f64,
    auto_paused_at_ms: Option<f64>,
}

impl ProfilerState {
    pub fn new() -> Self {
        Self {
            paused: false,
            frozen: None,
            view: ProfilerView::default(),
            show_puffin: false,
            expanded_overlay: false,
            frame_time_threshold_ms: 30.0,
            auto_paused_at_ms: None,
        }
    }

    /// Snapshot the panel should display: the frozen one while paused, the live one
    /// otherwise.
    fn displayed(&mut self, live: &Arc<ProfileSnapshot>) -> Arc<ProfileSnapshot> {
        if self.paused {
            self.frozen.get_or_insert_with(|| live.clone()).clone()
        } else {
            self.frozen = None;
            live.clone()
        }
    }

    fn toggle_pause(&mut self, live: &Arc<ProfileSnapshot>) {
        self.paused = !self.paused;
        self.frozen = self.paused.then(|| live.clone());
        self.auto_paused_at_ms = None;
    }

    fn catch_frame_time_pause(&mut self, live: &Arc<ProfileSnapshot>) {
        let Some(threshold_us) = live.frame_time_pause_threshold_us else {
            return;
        };

        // Retain the exact triggered Arc before releasing the engine-side capture.
        self.paused = true;
        self.frozen = Some(live.clone());
        self.auto_paused_at_ms = Some(threshold_us / 1000.0);
        engine::profiling::clear_frame_time_pause_capture();
    }
}

#[derive(Default)]
struct ProfilerView {
    source: Option<Arc<ProfileSnapshot>>,
    cores: Vec<CoreRow>,
    threads: Vec<ThreadRow>,
}

struct CoreRow {
    core: u32,
    utilization: f32,
    smoothed: f32,
    migrations: u32,
    activity: String,
}

struct ThreadRow {
    name: String,
    busy_ms: f64,
    activity: String,
}

impl ProfilerView {
    fn sync(&mut self, snapshot: &Arc<ProfileSnapshot>) {
        if self
            .source
            .as_ref()
            .is_some_and(|source| Arc::ptr_eq(source, snapshot))
        {
            return;
        }
        self.source = Some(snapshot.clone());
        self.rebuild(snapshot);
    }

    fn rebuild(&mut self, snapshot: &ProfileSnapshot) {
        let logical_cores = std::thread::available_parallelism().map_or(1, usize::from);
        let previous = self
            .cores
            .iter()
            .map(|row| row.smoothed)
            .collect::<Vec<_>>();
        self.cores.clear();
        self.threads.clear();

        let Some(frame) = snapshot.latest_frame.as_ref() else {
            return;
        };
        let frame_us = frame.total_us.max(1.0);

        let mut cores: Vec<CoreAccumulator> = (0..logical_cores)
            .map(|core| CoreAccumulator::new(core as u32))
            .collect();
        let mut threads: Vec<ThreadAccumulator> = Vec::new();

        for scope in &frame.scopes {
            if let Some(core) = scope.processor_start
                && let Some(accumulator) = cores.get_mut(core as usize)
            {
                let start = scope.start_us.clamp(0.0, frame_us);
                let end = (scope.start_us + scope.duration_us).clamp(start, frame_us);
                if scope.name != "frame.total" && end > start {
                    accumulator.intervals.push((start, end));
                }
                accumulator.add(&scope.name, scope.duration_us);
                if scope.processor_end.is_some_and(|end_core| end_core != core) {
                    accumulator.migrations += 1;
                }
            }

            let thread = match threads
                .iter_mut()
                .position(|thread| thread.id == scope.thread_id)
            {
                Some(index) => &mut threads[index],
                None => {
                    threads.push(ThreadAccumulator::new(&scope.thread_name, scope.thread_id));
                    threads.last_mut().expect("just pushed")
                }
            };
            if scope.name != "frame.total" {
                thread.busy_us += scope.duration_us;
            }
            thread.add(&scope.name, scope.duration_us);
        }

        for (index, mut accumulator) in cores.into_iter().enumerate() {
            let busy = interval_union_us(&mut accumulator.intervals).min(frame_us);
            let utilization = (busy / frame_us) as f32;
            let previous = previous.get(index).copied().unwrap_or(utilization);
            self.cores.push(CoreRow {
                core: accumulator.core,
                utilization,
                smoothed: previous + (utilization - previous) * SMOOTHING,
                migrations: accumulator.migrations,
                activity: accumulator.top_activities(),
            });
        }

        threads.sort_by(|a, b| b.busy_us.total_cmp(&a.busy_us));
        self.threads.extend(threads.into_iter().map(|thread| {
            let activity = thread.top_activities();
            ThreadRow {
                // Thread names repeat across pools, so the short id disambiguates them.
                name: format!("{} · {:04x}", thread.name, thread.id & 0xffff),
                busy_ms: thread.busy_us / 1000.0,
                activity,
            }
        }));
    }
}

/// Per core accumulator. Activities are kept in a `Vec` and sorted by
/// `(duration, name)`, so equal costs always land in the same order - a `HashMap`
/// reshuffled the labels every frame even when the samples were frozen.
struct CoreAccumulator {
    core: u32,
    intervals: Vec<(f64, f64)>,
    activities: Vec<(String, f64)>,
    migrations: u32,
}

impl CoreAccumulator {
    fn new(core: u32) -> Self {
        Self {
            core,
            intervals: Vec::new(),
            activities: Vec::new(),
            migrations: 0,
        }
    }

    fn add(&mut self, name: &str, duration_us: f64) {
        add_activity(&mut self.activities, name, duration_us);
    }

    fn top_activities(&self) -> String {
        format_activities(&self.activities, TOP_ACTIVITIES)
    }
}

struct ThreadAccumulator {
    name: String,
    id: u64,
    busy_us: f64,
    activities: Vec<(String, f64)>,
}

impl ThreadAccumulator {
    fn new(name: &str, id: u64) -> Self {
        Self {
            name: name.to_string(),
            id,
            busy_us: 0.0,
            activities: Vec::new(),
        }
    }

    fn add(&mut self, name: &str, duration_us: f64) {
        add_activity(&mut self.activities, name, duration_us);
    }

    fn top_activities(&self) -> String {
        format_activities(&self.activities, TOP_ACTIVITIES)
    }
}

fn add_activity(activities: &mut Vec<(String, f64)>, name: &str, duration_us: f64) {
    if name == "frame.total" {
        return;
    }
    match activities.iter_mut().find(|(entry, _)| entry == name) {
        Some((_, total)) => *total += duration_us,
        None => activities.push((name.to_string(), duration_us)),
    }
}

fn format_activities(activities: &[(String, f64)], count: usize) -> String {
    let mut sorted = activities.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let text = sorted
        .into_iter()
        .take(count)
        .map(|(name, duration)| format!("{} {:.2}ms", compact_scope_name(name), duration / 1000.0))
        .collect::<Vec<_>>()
        .join("  ·  ");

    if text.is_empty() {
        "idle / uninstrumented".to_string()
    } else {
        text
    }
}

fn interval_union_us(intervals: &mut [(f64, f64)]) -> f64 {
    if intervals.is_empty() {
        return 0.0;
    }
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut total = 0.0;
    let (mut start, mut end) = intervals[0];
    for &(next_start, next_end) in &intervals[1..] {
        if next_start <= end {
            end = end.max(next_end);
        } else {
            total += end - start;
            (start, end) = (next_start, next_end);
        }
    }
    total + end - start
}

fn compact_scope_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

pub fn draw_profiler(ui: &mut Ui, state: &mut EditorContext<'_>, profiler: &mut ProfilerState) {
    let live = state.global_resources.profiler_snapshot.clone();
    profiler.catch_frame_time_pause(&live);
    let snapshot = profiler.displayed(&live);
    profiler.view.sync(&snapshot);

    theme::toolbar(ui, |ui| {
        pause_button(ui, profiler, &live);
        frame_time_pause_controls(ui, profiler);
        if ui.button("Capture GPU frame").clicked() {
            state.global_resources.frame_capturer.request_capture();
        }
        #[cfg(feature = "puffin-ui")]
        {
            let label = if profiler.show_puffin {
                "Hide Puffin"
            } else {
                "Show Puffin"
            };
            if ui.button(label).clicked() {
                profiler.show_puffin = !profiler.show_puffin;
            }
        }
        ui.separator();
        refresh_rate_slider(ui);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            theme::tag(
                ui,
                if snapshot.tracy_enabled {
                    "tracy on"
                } else {
                    "tracy off"
                },
                if snapshot.tracy_enabled {
                    theme::SUCCESS
                } else {
                    theme::TEXT_DIM
                },
            );
            theme::tag(
                ui,
                if snapshot.puffin_enabled {
                    "puffin on"
                } else {
                    "puffin off"
                },
                if snapshot.puffin_enabled {
                    theme::SUCCESS
                } else {
                    theme::TEXT_DIM
                },
            );
        });
    });

    #[cfg(feature = "puffin-ui")]
    if profiler.show_puffin && !puffin_egui::profiler_window(ui.ctx()) {
        profiler.show_puffin = false;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("profiler_body")
        .show(ui, |ui| {
            draw_frame_summary(ui, &snapshot);
            ui.add_space(6.0);
            draw_frame_graph(ui, &snapshot);
            ui.add_space(8.0);

            collapsing(ui, "Logical CPUs", true, |ui| {
                draw_core_rows(ui, &profiler.view, 180.0);
            });
            collapsing(ui, "Threads", true, |ui| {
                draw_thread_rows(ui, &profiler.view);
            });
            collapsing(ui, "Scopes", true, |ui| {
                draw_scope_table(ui, &snapshot);
            });
            collapsing(ui, "GPU passes", true, |ui| {
                draw_gpu_passes(ui, &snapshot.gpu, true);
            });
            collapsing(ui, "Counters", false, |ui| {
                draw_counters(ui, &snapshot);
            });
            collapsing(ui, "Sampling profiler", false, |ui| {
                super::cpu_sampling::draw_cpu_sampling(ui, &snapshot.cpu);
            });
        });
}

/// Compact version of the profiler used by the floating overlay.
pub fn draw_performance_overlay(
    ui: &mut Ui,
    live: &Arc<ProfileSnapshot>,
    profiler: &mut ProfilerState,
) {
    profiler.catch_frame_time_pause(live);
    let snapshot = profiler.displayed(live);
    profiler.view.sync(&snapshot);

    ui.horizontal(|ui| {
        pause_button(ui, profiler, live);
        let expanded = profiler.expanded_overlay;
        ui.toggle_value(
            &mut profiler.expanded_overlay,
            if expanded { "Compact" } else { "Expand" },
        );
    });
    ui.horizontal(|ui| frame_time_pause_controls(ui, profiler));
    ui.horizontal(|ui| refresh_rate_slider(ui));
    ui.add_space(4.0);

    let cpu_ms = snapshot
        .latest_frame
        .as_ref()
        .map_or(0.0, |frame| frame.total_us / 1000.0);
    let gpu_ms = snapshot
        .gpu
        .latest_frame
        .as_ref()
        .map_or(0.0, |frame| frame.summed_pass_ms);
    ui.horizontal(|ui| {
        theme::stat(ui, "CPU", format!("{cpu_ms:.2} ms"), theme::ms_color(cpu_ms));
        theme::stat(ui, "GPU Σ", format!("{gpu_ms:.2} ms"), theme::ms_color(gpu_ms));
        theme::stat(
            ui,
            "FPS",
            format!("{:.0}", if cpu_ms > 0.0 { 1000.0 / cpu_ms } else { 0.0 }),
            theme::TEXT_STRONG,
        );
    });

    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("performance_overlay")
        .show(ui, |ui| {
            draw_frame_graph(ui, &snapshot);
            ui.add_space(6.0);
            draw_core_rows(ui, &profiler.view, 110.0);
            if profiler.expanded_overlay {
                ui.add_space(6.0);
                theme::section(ui, "GPU passes");
                draw_gpu_passes(ui, &snapshot.gpu, false);
            }
        });
}

/// Controls how often the engine republishes the snapshot. Zero means every frame,
/// which is the "instant" end of the slider.
fn refresh_rate_slider(ui: &mut Ui) {
    let mut interval_ms = engine::profiling::snapshot_interval().as_millis() as u64;
    ui.label(RichText::new("refresh").color(theme::TEXT_DIM));
    let response = ui.add(
        egui::Slider::new(&mut interval_ms, 0..=500)
            .custom_formatter(|value, _| {
                if value < 1.0 {
                    "live".to_string()
                } else {
                    format!("{value:.0} ms")
                }
            })
            .clamping(egui::SliderClamping::Always),
    );
    if response.changed() {
        engine::profiling::set_snapshot_interval(std::time::Duration::from_millis(interval_ms));
    }
    response.on_hover_text(
        "How often the profiler data is rebuilt. `live` rebuilds every frame, which costs \
         real time but shows changes instantly.",
    );
}

fn pause_button(ui: &mut Ui, profiler: &mut ProfilerState, live: &Arc<ProfileSnapshot>) {
    let label = if profiler.paused {
        RichText::new("▶ Resume").color(theme::SUCCESS)
    } else {
        RichText::new("⏸ Pause").color(theme::TEXT)
    };
    if ui.button(label).clicked() {
        profiler.toggle_pause(live);
    }
    if profiler.paused {
        let text = profiler.auto_paused_at_ms.map_or_else(
            || "PAUSED".to_string(),
            |threshold| format!("AUTO-PAUSED >= {threshold:.1} ms"),
        );
        theme::tag(ui, &text, theme::WARN);
    }
}

fn frame_time_pause_controls(ui: &mut Ui, profiler: &mut ProfilerState) {
    let armed = engine::profiling::frame_time_pause_threshold().is_some();
    let label = if armed {
        "Cancel slow-frame pause"
    } else {
        "Pause on CPU frame >="
    };
    let response = ui.button(label).on_hover_text(
        "One-shot trigger: preserve the first CPU frame at or above the threshold and freeze \
         this profiler view. The simulation keeps running.",
    );
    if response.clicked() {
        if armed {
            engine::profiling::cancel_frame_time_pause();
        } else {
            let threshold = std::time::Duration::from_secs_f64(
                (profiler.frame_time_threshold_ms.max(0.1)) / 1000.0,
            );
            engine::profiling::arm_frame_time_pause(threshold);
            profiler.auto_paused_at_ms = None;
        }
    }
    if !armed {
        ui.add(
            egui::DragValue::new(&mut profiler.frame_time_threshold_ms)
                .range(0.1..=1000.0)
                .speed(0.5)
                .suffix(" ms"),
        );
    } else if let Some(threshold) = engine::profiling::frame_time_pause_threshold() {
        theme::tag(
            ui,
            &format!("ARMED {:.1} ms", threshold.as_secs_f64() * 1000.0),
            theme::SUCCESS,
        );
    }
}

fn collapsing(ui: &mut Ui, title: &str, default_open: bool, add: impl FnOnce(&mut Ui)) {
    egui::CollapsingHeader::new(RichText::new(title).strong().color(theme::TEXT_STRONG))
        .id_salt(title)
        .default_open(default_open)
        .show(ui, add);
}

fn draw_frame_summary(ui: &mut Ui, snapshot: &ProfileSnapshot) {
    let cpu_ms = snapshot
        .latest_frame
        .as_ref()
        .map_or(0.0, |frame| frame.total_us / 1000.0);
    let gpu_ms = snapshot
        .gpu
        .latest_frame
        .as_ref()
        .map_or(0.0, |frame| frame.summed_pass_ms);

    ui.horizontal_wrapped(|ui| {
        theme::stat(ui, "CPU frame", format!("{cpu_ms:.2} ms"), theme::ms_color(cpu_ms));
        theme::stat(ui, "GPU Σ", format!("{gpu_ms:.2} ms"), theme::ms_color(gpu_ms));
        theme::stat(
            ui,
            "FPS",
            format!("{:.0}", if cpu_ms > 0.0 { 1000.0 / cpu_ms } else { 0.0 }),
            theme::TEXT_STRONG,
        );
        theme::stat(
            ui,
            "Average",
            format!("{:.2} ms", snapshot.average_frame_us / 1000.0),
            theme::TEXT_STRONG,
        );
        theme::stat(
            ui,
            "Worst",
            format!("{:.2} ms", snapshot.max_frame_us / 1000.0),
            theme::ms_color(snapshot.max_frame_us / 1000.0),
        );
        theme::stat(
            ui,
            "Samples",
            snapshot.timings.len().to_string(),
            theme::TEXT_STRONG,
        );
    });
}

fn draw_frame_graph(ui: &mut Ui, snapshot: &ProfileSnapshot) {
    let height = 76.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, egui::CornerRadius::same(6), theme::BG_DEEP);

    if snapshot.timings.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "no frames recorded",
            egui::FontId::proportional(11.0),
            theme::TEXT_DIM,
        );
        return;
    }

    let peak_ms = snapshot
        .timings
        .iter()
        .map(|timing| timing.total_us / 1000.0)
        .fold(20.0_f64, f64::max);

    for (budget, color) in [(16.7, theme::SUCCESS), (33.3, theme::WARN)] {
        if budget > peak_ms {
            continue;
        }
        let y = rect.bottom() - (budget / peak_ms) as f32 * rect.height();
        painter.hline(
            rect.x_range(),
            y,
            egui::Stroke::new(1.0, color.gamma_multiply(0.35)),
        );
    }

    let count = snapshot.timings.len();
    let bar_width = rect.width() / count as f32;
    for (index, timing) in snapshot.timings.iter().enumerate() {
        let ms = timing.total_us / 1000.0;
        let height = ((ms / peak_ms) as f32 * rect.height()).clamp(1.0, rect.height());
        let x0 = rect.left() + index as f32 * bar_width;
        let bar = egui::Rect::from_min_max(
            egui::pos2(x0, rect.bottom() - height),
            egui::pos2((x0 + bar_width - 1.0).max(x0 + 1.0), rect.bottom()),
        );
        painter.rect_filled(bar, 0.0, theme::ms_color(ms));
    }

    // GPU cost as a line on top of the CPU bars.
    if snapshot.gpu.timings.len() > 1 {
        let gpu = &snapshot.gpu.timings;
        let step = rect.width() / (gpu.len() - 1).max(1) as f32;
        let points = gpu
            .iter()
            .enumerate()
            .map(|(index, timing)| {
                let y = rect.bottom()
                    - ((timing.summed_pass_ms / peak_ms) as f32).clamp(0.0, 1.0) * rect.height();
                egui::pos2(rect.left() + index as f32 * step, y)
            })
            .collect::<Vec<_>>();
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(1.5, theme::ACCENT),
        ));
    }

    painter.text(
        rect.left_top() + Vec2::new(6.0, 4.0),
        egui::Align2::LEFT_TOP,
        format!("peak {peak_ms:.1} ms"),
        egui::FontId::proportional(10.0),
        theme::TEXT_DIM,
    );
    response.on_hover_text("Bars: CPU frame time · Line: summed GPU pass time");
}

fn draw_core_rows(ui: &mut Ui, view: &ProfilerView, bar_width: f32) {
    if view.cores.is_empty() {
        ui.label(RichText::new("No CPU frame recorded yet.").color(theme::TEXT_DIM));
        return;
    }

    for row in &view.cores {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(
                RichText::new(format!("{:02}", row.core))
                    .monospace()
                    .size(10.5)
                    .color(theme::TEXT_DIM),
            );
            let response = theme::meter(
                ui,
                row.smoothed,
                bar_width,
                &format!("{:.0}%", row.utilization * 100.0),
                theme::load_color(row.smoothed),
            );
            if row.migrations > 0 {
                response.on_hover_text(format!(
                    "{} scope(s) migrated to another logical CPU",
                    row.migrations
                ));
            }
            theme::truncated(ui, &row.activity, theme::TEXT_DIM);
        });
    }
}

fn draw_thread_rows(ui: &mut Ui, view: &ProfilerView) {
    if view.threads.is_empty() {
        ui.label(RichText::new("No thread samples yet.").color(theme::TEXT_DIM));
        return;
    }

    for row in &view.threads {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            theme::truncated_sized(ui, 150.0, &row.name, theme::TEXT);
            ui.label(
                RichText::new(format!("{:>7.2} ms", row.busy_ms))
                    .monospace()
                    .size(10.5)
                    .color(theme::ms_color(row.busy_ms)),
            );
            theme::truncated(ui, &row.activity, theme::TEXT_DIM);
        });
    }
}

fn draw_scope_table(ui: &mut Ui, snapshot: &ProfileSnapshot) {
    if snapshot.latest_scopes.is_empty() {
        ui.label(RichText::new("No scopes recorded.").color(theme::TEXT_DIM));
        return;
    }

    let frame_us = snapshot
        .latest_frame
        .as_ref()
        .map_or(1.0, |frame| frame.total_us.max(1.0));

    egui::Grid::new("profiler_scopes")
        .num_columns(5)
        .spacing(Vec2::new(10.0, 2.0))
        .striped(true)
        .show(ui, |ui| {
            header_row(ui, &["Scope", "Share", "Calls", "Total", "Max"]);
            for scope in snapshot.latest_scopes.iter().take(40) {
                theme::truncated_sized(ui, 220.0, &scope.name, theme::TEXT);
                let share = (scope.total_us / frame_us) as f32;
                theme::meter(
                    ui,
                    share,
                    90.0,
                    &format!("{:.0}%", share * 100.0),
                    theme::load_color(share),
                );
                mono(ui, scope.calls.to_string(), theme::TEXT_DIM);
                mono(ui, format!("{:.2} ms", scope.total_us / 1000.0), theme::TEXT);
                mono(
                    ui,
                    format!("{:.2} ms", scope.max_us / 1000.0),
                    theme::TEXT_DIM,
                );
                ui.end_row();
            }
        });
}

fn draw_counters(ui: &mut Ui, snapshot: &ProfileSnapshot) {
    let Some(frame) = snapshot.latest_frame.as_ref() else {
        return;
    };
    if frame.counters.is_empty() {
        ui.label(RichText::new("No counters recorded.").color(theme::TEXT_DIM));
        return;
    }

    egui::Grid::new("profiler_counters")
        .num_columns(2)
        .spacing(Vec2::new(10.0, 2.0))
        .striped(true)
        .show(ui, |ui| {
            for counter in &frame.counters {
                theme::truncated_sized(ui, 220.0, &counter.name, theme::TEXT);
                mono(ui, format!("{:.0}", counter.value), theme::TEXT_STRONG);
                ui.end_row();
            }
        });
}

pub fn draw_gpu_passes(
    ui: &mut Ui,
    snapshot: &engine::profiling::gpu::GpuProfileSnapshot,
    detailed: bool,
) {
    let Some(frame) = snapshot.latest_frame.as_ref() else {
        ui.label(
            RichText::new(if snapshot.timestamp_supported {
                "Waiting for asynchronous GPU query results."
            } else {
                "GPU timestamps are unsupported on this device."
            })
            .color(theme::TEXT_DIM),
        );
        return;
    };

    ui.label(
        RichText::new(format!(
            "frame {} · {:.3} ms summed",
            frame.index, frame.summed_pass_ms
        ))
        .small()
        .color(theme::TEXT_DIM),
    );

    let columns = if detailed { 7 } else { 2 };
    egui::Grid::new(if detailed {
        "gpu_passes_detailed"
    } else {
        "gpu_passes_compact"
    })
    .num_columns(columns)
    .spacing(Vec2::new(10.0, 2.0))
    .striped(true)
    .show(ui, |ui| {
        if detailed {
            header_row(
                ui,
                &[
                    "Pass",
                    "GPU ms",
                    "Vertex",
                    "Clip in",
                    "Prims out",
                    "Fragment",
                    "Compute",
                ],
            );
        } else {
            header_row(ui, &["Pass", "GPU ms"]);
        }

        for pass in &frame.passes {
            theme::truncated_sized(ui, 170.0, compact_scope_name(&pass.name), theme::TEXT);
            match pass.duration_ms {
                Some(duration) => mono(
                    ui,
                    format!("{duration:.3}"),
                    theme::ms_color(duration),
                ),
                None => mono(ui, "—".to_string(), theme::TEXT_DIM),
            }
            if detailed {
                let statistics = pass.statistics.unwrap_or_default();
                for value in [
                    statistics.vertex_shader_invocations,
                    statistics.clipper_invocations,
                    statistics.clipper_primitives_out,
                    statistics.fragment_shader_invocations,
                    statistics.compute_shader_invocations,
                ] {
                    mono(ui, value.to_string(), theme::TEXT_DIM);
                }
            }
            ui.end_row();
        }
    });
}

pub fn header_row(ui: &mut Ui, labels: &[&str]) {
    for label in labels {
        ui.label(
            RichText::new(*label)
                .size(10.0)
                .strong()
                .color(theme::TEXT_DIM),
        );
    }
    ui.end_row();
}

pub fn mono(ui: &mut Ui, text: impl Into<String>, color: Color32) {
    ui.label(RichText::new(text.into()).monospace().color(color));
}
