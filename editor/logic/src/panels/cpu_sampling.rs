//! ETW sampling profiler view: costs resolved from call stacks, no scope markers.

use crate::panels::profiler::{header_row, mono};
use crate::theme;
use egui::{RichText, Ui, Vec2};
use engine::profiling::cpu::{CpuCallTreeNode, CpuCaptureState, CpuProfileSnapshot};

const MAX_ROWS: usize = 300;
const MAX_TREE_DEPTH: usize = 48;

pub fn draw_cpu_sampling(ui: &mut Ui, snapshot: &CpuProfileSnapshot) {
    ui.horizontal_wrapped(|ui| {
        match snapshot.state {
            CpuCaptureState::Capturing | CpuCaptureState::Processing => {
                if ui.button("⏹ Stop capture").clicked() {
                    engine::profiling::cpu::stop_capture();
                }
            }
            _ if snapshot.supported => {
                for seconds in [1, 3, 5] {
                    if ui.button(format!("⏺ {seconds}s")).clicked()
                        && let Err(error) = engine::profiling::cpu::start_capture(
                            std::time::Duration::from_secs(seconds),
                        )
                    {
                        log::error!("Could not start CPU capture: {error}");
                    }
                }
                if snapshot.total_samples > 0 && ui.button("Clear").clicked() {
                    engine::profiling::cpu::clear_capture();
                }
                #[cfg(not(target_arch = "wasm32"))]
                if snapshot.etl_available && ui.button("Save ETL…").clicked() {
                    save_etl_capture();
                }
            }
            _ => {}
        }
        ui.separator();
        ui.label(RichText::new(&snapshot.status).color(theme::TEXT_DIM));
    });

    if snapshot.state == CpuCaptureState::Capturing {
        let requested = snapshot.requested_duration.as_secs_f32().max(0.001);
        let progress = (snapshot.elapsed.as_secs_f32() / requested).clamp(0.0, 1.0);
        theme::meter(
            ui,
            progress,
            ui.available_width().min(280.0),
            &format!(
                "{:.1} / {:.1} s",
                snapshot.elapsed.as_secs_f32(),
                requested
            ),
            theme::ACCENT,
        );
    }

    if snapshot.total_samples == 0 {
        ui.label(
            RichText::new(
                "Samples native Rust, crate, Windows and driver call stacks wherever matching \
                 symbols are available - no scope markers required.",
            )
            .color(theme::TEXT_DIM),
        );
        return;
    }

    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        theme::stat(
            ui,
            "Samples",
            snapshot.total_samples.to_string(),
            theme::TEXT_STRONG,
        );
        theme::stat(
            ui,
            "Stacks",
            snapshot.distinct_stacks.to_string(),
            theme::TEXT_STRONG,
        );
        theme::stat(
            ui,
            "Duration",
            format!("{:.2} s", snapshot.elapsed.as_secs_f64()),
            theme::TEXT_STRONG,
        );
        theme::stat(
            ui,
            "CPU / frame",
            format!(
                "{:.2} ms",
                ms_per_frame(snapshot, snapshot.total_samples)
            ),
            theme::ACCENT,
        )
        .on_hover_text(
            "Processor time per engine frame across all sampled threads; can exceed wall clock \
             time when threads run in parallel.",
        );
        theme::stat(
            ui,
            "Frames",
            snapshot.captured_frames.to_string(),
            theme::TEXT_STRONG,
        );
        theme::stat(
            ui,
            "Located",
            format!(
                "{} / {}",
                snapshot.source_location_addresses, snapshot.symbolized_addresses
            ),
            theme::TEXT_DIM,
        )
        .on_hover_text("Sampled addresses resolved to a source file and line.");
    });
    ui.add_space(4.0);

    section(ui, "Function hotspots (self CPU)", true, |ui| {
        egui::ScrollArea::both()
            .id_salt("cpu_function_hotspots")
            .max_height(320.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("cpu_function_hotspot_grid")
                    .num_columns(6)
                    .striped(true)
                    .spacing(Vec2::new(10.0, 2.0))
                    .show(ui, |ui| {
                        header_row(
                            ui,
                            &["Self", "Self ms/f", "Total", "Total ms/f", "Function", "Source"],
                        );
                        for hotspot in snapshot.functions.iter().take(MAX_ROWS) {
                            mono(
                                ui,
                                percent(hotspot.self_samples, snapshot.total_samples),
                                theme::TEXT_STRONG,
                            );
                            mono(
                                ui,
                                format!("{:.3}", ms_per_frame(snapshot, hotspot.self_samples)),
                                theme::TEXT,
                            );
                            mono(
                                ui,
                                percent(hotspot.inclusive_samples, snapshot.total_samples),
                                theme::TEXT_DIM,
                            );
                            mono(
                                ui,
                                format!("{:.3}", ms_per_frame(snapshot, hotspot.inclusive_samples)),
                                theme::TEXT_DIM,
                            );
                            theme::truncated_sized(ui, 420.0, &hotspot.function, theme::TEXT);
                            theme::truncated_sized(
                                ui,
                                260.0,
                                location(hotspot.file.as_deref(), hotspot.line, &hotspot.module),
                                theme::TEXT_DIM,
                            );
                            ui.end_row();
                        }
                    });
            });
    });

    section(ui, "Top-down call tree", false, |ui| {
        egui::ScrollArea::both()
            .id_salt("cpu_call_tree")
            .max_height(380.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for node in snapshot.call_tree.iter().take(64) {
                    draw_call_tree_node(ui, node, snapshot, 0);
                }
            });
    });

    section(ui, "Bottom-up callers", false, |ui| {
        egui::ScrollArea::both()
            .id_salt("cpu_bottom_up")
            .max_height(380.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for node in snapshot.bottom_up.iter().take(64) {
                    draw_call_tree_node(ui, node, snapshot, 0);
                }
            });
    });

    section(
        ui,
        &format!("Hot source lines ({})", snapshot.source_lines.len()),
        false,
        |ui| {
            if snapshot.source_lines.is_empty() {
                ui.label(
                    RichText::new(
                        "No source lines resolved. Build with debug information and keep the \
                         matching PDB beside the executable.",
                    )
                    .color(theme::TEXT_DIM),
                );
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt("cpu_source_lines")
                .max_height(300.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("cpu_source_line_grid")
                        .num_columns(5)
                        .striped(true)
                        .spacing(Vec2::new(10.0, 2.0))
                        .show(ui, |ui| {
                            header_row(
                                ui,
                                &["Self", "Self ms/f", "Total", "Total ms/f", "Source line"],
                            );
                            for source in snapshot.source_lines.iter().take(MAX_ROWS) {
                                mono(
                                    ui,
                                    percent(source.self_samples, snapshot.total_samples),
                                    theme::TEXT_STRONG,
                                );
                                mono(
                                    ui,
                                    format!("{:.3}", ms_per_frame(snapshot, source.self_samples)),
                                    theme::TEXT,
                                );
                                mono(
                                    ui,
                                    percent(source.inclusive_samples, snapshot.total_samples),
                                    theme::TEXT_DIM,
                                );
                                mono(
                                    ui,
                                    format!(
                                        "{:.3}",
                                        ms_per_frame(snapshot, source.inclusive_samples)
                                    ),
                                    theme::TEXT_DIM,
                                );
                                theme::truncated_sized(
                                    ui,
                                    440.0,
                                    format!("{}:{}", short_path(&source.file), source.line),
                                    theme::TEXT,
                                );
                                ui.end_row();
                            }
                        });
                });
        },
    );

    section(
        ui,
        &format!("Source file costs ({})", snapshot.source_files.len()),
        false,
        |ui| {
            egui::ScrollArea::vertical()
                .id_salt("cpu_source_files")
                .max_height(280.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("cpu_source_file_grid")
                        .num_columns(3)
                        .striped(true)
                        .spacing(Vec2::new(10.0, 2.0))
                        .show(ui, |ui| {
                            header_row(ui, &["Self", "Total", "Source file"]);
                            for source in snapshot.source_files.iter().take(MAX_ROWS) {
                                mono(
                                    ui,
                                    percent(source.self_samples, snapshot.total_samples),
                                    theme::TEXT_STRONG,
                                );
                                mono(
                                    ui,
                                    percent(source.inclusive_samples, snapshot.total_samples),
                                    theme::TEXT_DIM,
                                );
                                theme::truncated_sized(
                                    ui,
                                    440.0,
                                    short_path(&source.file),
                                    theme::TEXT,
                                );
                                ui.end_row();
                            }
                        });
                });
        },
    );

    section(ui, "Sampled threads", false, |ui| {
        for thread in &snapshot.threads {
            ui.horizontal(|ui| {
                mono(
                    ui,
                    percent(thread.samples, snapshot.total_samples),
                    theme::TEXT_STRONG,
                );
                ui.label(
                    RichText::new(format!("thread {}", thread.thread_id)).color(theme::TEXT_DIM),
                );
            });
        }
    });
}

fn section(ui: &mut Ui, title: &str, default_open: bool, add: impl FnOnce(&mut Ui)) {
    egui::CollapsingHeader::new(RichText::new(title).color(theme::TEXT_STRONG))
        .id_salt(title)
        .default_open(default_open)
        .show(ui, add);
}

#[cfg(not(target_arch = "wasm32"))]
fn save_etl_capture() {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("ETW trace", &["etl"])
        .set_file_name("plaxel-cpu-capture.etl")
        .save_file()
    else {
        return;
    };

    match engine::profiling::cpu::save_etl(&path) {
        Ok(()) => log::info!("Saved ETW CPU capture to {}", path.display()),
        Err(error) => log::error!("Could not save ETW CPU capture: {error}"),
    }
}

fn draw_call_tree_node(
    ui: &mut Ui,
    node: &CpuCallTreeNode,
    snapshot: &CpuProfileSnapshot,
    depth: usize,
) {
    let mut chain = vec![node];
    while chain.len() + depth < MAX_TREE_DEPTH {
        let tail = *chain.last().expect("chain is never empty");
        if tail.self_samples > 0 || tail.children.len() != 1 {
            break;
        }
        chain.push(&tail.children[0]);
    }
    let tail = *chain.last().expect("chain is never empty");

    if tail.children.is_empty() || depth + chain.len() >= MAX_TREE_DEPTH {
        draw_call_chain(ui, &chain, snapshot);
        return;
    }

    egui::CollapsingHeader::new(call_tree_label(node, snapshot))
        .id_salt((
            depth,
            node.function.as_str(),
            node.inclusive_samples,
            node.self_samples,
        ))
        .show(ui, |ui| {
            if chain.len() > 1 {
                draw_call_chain(ui, &chain[1..], snapshot);
            }
            for child in tail.children.iter().take(128) {
                draw_call_tree_node(ui, child, snapshot, depth + chain.len());
            }
        });
}

fn draw_call_chain(ui: &mut Ui, chain: &[&CpuCallTreeNode], snapshot: &CpuProfileSnapshot) {
    if chain.len() == 1 {
        ui.label(RichText::new(call_tree_label(chain[0], snapshot)).monospace())
            .on_hover_text(call_tree_hover(chain[0]));
        return;
    }

    theme::card(ui, |ui| {
        ui.label(
            RichText::new(format!("single path · {} frames", chain.len()))
                .small()
                .color(theme::TEXT_DIM),
        );
        for node in chain {
            ui.label(RichText::new(call_tree_label(node, snapshot)).monospace())
                .on_hover_text(call_tree_hover(node));
        }
    });
}

fn call_tree_label(node: &CpuCallTreeNode, snapshot: &CpuProfileSnapshot) -> String {
    format!(
        "{} total {:.3} ms/f │ {} self {:.3} ms/f │ {} — {}",
        percent(node.inclusive_samples, snapshot.total_samples),
        ms_per_frame(snapshot, node.inclusive_samples),
        percent(node.self_samples, snapshot.total_samples),
        ms_per_frame(snapshot, node.self_samples),
        node.function,
        location(node.file.as_deref(), node.line, &node.module)
    )
}

fn call_tree_hover(node: &CpuCallTreeNode) -> String {
    match (&node.file, node.line) {
        (Some(file), Some(line)) => format!("{}\n{file}:{line}\n{}", node.function, node.module),
        (Some(file), None) => format!("{}\n{file}\n{}", node.function, node.module),
        (None, _) => format!("{}\n{}", node.function, node.module),
    }
}

fn ms_per_frame(snapshot: &CpuProfileSnapshot, samples: u64) -> f64 {
    samples as f64 * snapshot.sample_interval.as_secs_f64() * 1_000.0
        / snapshot.captured_frames.max(1) as f64
}

fn percent(samples: u64, total_samples: u64) -> String {
    format!(
        "{:5.2}%",
        samples as f64 * 100.0 / total_samples.max(1) as f64
    )
}

fn location(file: Option<&str>, line: Option<u32>, module: &str) -> String {
    match (file, line) {
        (Some(file), Some(line)) => format!("{}:{line}", short_path(file)),
        (Some(file), None) => short_path(file),
        (None, _) if !module.is_empty() => module.to_string(),
        _ => "unknown".to_string(),
    }
}

fn short_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    for marker in ["/engine/", "/editor/", "/game/", "/src/"] {
        if let Some(index) = normalized.rfind(marker) {
            return normalized[index + 1..].to_string();
        }
    }
    normalized
}
