use std::{
    collections::VecDeque,
    fmt,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::SystemTime,
};
use tracing::{Event, Level, Subscriber, field::Field};
use tracing_log::LogTracer;
use tracing_subscriber::{
    EnvFilter, Layer, field::Visit, layer::Context, prelude::*, registry::LookupSpan,
};

const MAX_CONSOLE_ENTRIES: usize = 1_000;
const CONSOLE_LOG_PATH: &str = ".plaxel_editor_console.log";

// Engine
#[macro_export]
macro_rules! engine_info {
    ($($arg:tt)*) => {
        $crate::logging::record_console_entry(
            $crate::logging::ConsoleLevel::Info,
            "engine",
            format!($($arg)*),
        );
        $crate::tracing::info!(target: "engine", $($arg)*);
    };
}

#[macro_export]
macro_rules! engine_warn {
    ($($arg:tt)*) => {
        $crate::logging::record_console_entry(
            $crate::logging::ConsoleLevel::Warn,
            "engine",
            format!($($arg)*),
        );
        $crate::tracing::warn!(target: "engine", $($arg)*);
    };
}

#[macro_export]
macro_rules! engine_error {
    ($($arg:tt)*) => {
        $crate::logging::record_console_entry(
            $crate::logging::ConsoleLevel::Error,
            "engine",
            format!($($arg)*),
        );
        $crate::tracing::error!(target: "engine", $($arg)*);
    };
}

// Game
#[macro_export]
macro_rules! game_info {
    ($($arg:tt)*) => {
        $crate::logging::record_console_entry(
            $crate::logging::ConsoleLevel::Info,
            "game",
            format!($($arg)*),
        );
        $crate::tracing::info!(target: "game", $($arg)*);
    };
}

#[macro_export]
macro_rules! game_warn {
    ($($arg:tt)*) => {
        $crate::logging::record_console_entry(
            $crate::logging::ConsoleLevel::Warn,
            "game",
            format!($($arg)*),
        );
        $crate::tracing::warn!(target: "game", $($arg)*);
    };
}

#[macro_export]
macro_rules! game_error {
    ($($arg:tt)*) => {
        $crate::logging::record_console_entry(
            $crate::logging::ConsoleLevel::Error,
            "game",
            format!($($arg)*),
        );
        $crate::tracing::error!(target: "game", $($arg)*);
    };
}

pub fn init() {
    let filter = EnvFilter::builder()
        .with_default_directive(tracing::Level::WARN.into())
        .parse_lossy(
            "engine=info,game=info,hot_lib_reloader=trace,wgpu=error,wgpu_hal=error,naga=error",
        );

    let _ = LogTracer::init();

    #[cfg(not(target_arch = "wasm32"))]
    {
        let fmt_layer = tracing_subscriber::fmt::layer();
        let console_layer = ConsoleLayer;
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .with(console_layer)
            .try_init();
    }

    #[cfg(target_arch = "wasm32")]
    {
        let console_layer = ConsoleLayer;
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(console_layer)
            .try_init();
    }

    std::panic::set_hook(Box::new(|info| {
        let report = format_panic_report(info);
        push_console_entry(ConsoleLevel::Panic, "panic", report.clone());
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!("{report}");
    }));

    engine_info!("in-editor console log sink initialized");
}

#[derive(Clone, Debug)]
pub struct ConsoleEntry {
    pub sequence: u64,
    pub level: ConsoleLevel,
    pub target: String,
    pub message: String,
    pub time: SystemTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Panic,
}

pub fn console_entries() -> Vec<ConsoleEntry> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let file_entries = console_file_entries();
        if !file_entries.is_empty() {
            return file_entries;
        }
    }

    console_buffer()
        .lock()
        .map(|entries| entries.iter().cloned().collect())
        .unwrap_or_default()
}

pub fn clear_console_entries() {
    if let Ok(mut entries) = console_buffer().lock() {
        entries.clear();
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = fs::write(console_log_path(), "");
    }
}

pub fn record_console_entry(
    level: ConsoleLevel,
    target: impl Into<String>,
    message: impl Into<String>,
) {
    push_console_entry(level, target, message);
}

struct ConsoleLayer;

impl<S> Layer<S> for ConsoleLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        if matches!(metadata.target(), "engine" | "game") {
            return;
        }

        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);

        let message = visitor.finish();
        push_console_entry(
            ConsoleLevel::from_tracing_level(metadata.level()),
            metadata.target(),
            message,
        );
    }
}

#[derive(Default)]
struct EventVisitor {
    message: Option<String>,
    fields: Vec<String>,
}

impl EventVisitor {
    fn finish(self) -> String {
        match (self.message, self.fields.is_empty()) {
            (Some(message), true) => message,
            (Some(message), false) => format!("{} {}", message, self.fields.join(" ")),
            (None, true) => String::new(),
            (None, false) => self.fields.join(" "),
        }
    }
}

impl Visit for EventVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
        } else {
            self.fields.push(format!("{}={value:?}", field.name()));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_owned());
        } else {
            self.fields.push(format!("{}={value:?}", field.name()));
        }
    }
}

impl ConsoleLevel {
    fn from_tracing_level(level: &Level) -> Self {
        match *level {
            Level::TRACE => Self::Trace,
            Level::DEBUG => Self::Debug,
            Level::INFO => Self::Info,
            Level::WARN => Self::Warn,
            Level::ERROR => Self::Error,
        }
    }
}

fn console_buffer() -> &'static Mutex<VecDeque<ConsoleEntry>> {
    static CONSOLE_BUFFER: OnceLock<Mutex<VecDeque<ConsoleEntry>>> = OnceLock::new();
    CONSOLE_BUFFER.get_or_init(|| Mutex::new(VecDeque::with_capacity(MAX_CONSOLE_ENTRIES)))
}

fn push_console_entry(level: ConsoleLevel, target: impl Into<String>, message: impl Into<String>) {
    static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let sequence = NEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let target = target.into();
    let message = message.into();

    let Ok(mut entries) = console_buffer().lock() else {
        return;
    };

    if entries.len() >= MAX_CONSOLE_ENTRIES {
        entries.pop_front();
    }

    entries.push_back(ConsoleEntry {
        sequence,
        level,
        target: target.clone(),
        message: message.clone(),
        time: SystemTime::now(),
    });

    #[cfg(not(target_arch = "wasm32"))]
    append_console_file_entry(level, &target, &message);
}

#[cfg(not(target_arch = "wasm32"))]
fn console_log_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(CONSOLE_LOG_PATH)
}

#[cfg(not(target_arch = "wasm32"))]
fn append_console_file_entry(level: ConsoleLevel, target: &str, message: &str) {
    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(console_log_path())
    else {
        return;
    };

    let _ = writeln!(
        file,
        "{}\t{}\t{}",
        console_level_file_label(level),
        escape_console_field(target),
        escape_console_field(message)
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn console_file_entries() -> Vec<ConsoleEntry> {
    let Ok(file) = fs::File::open(console_log_path()) else {
        return Vec::new();
    };

    let lines = BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .collect::<Vec<_>>();
    let start = lines.len().saturating_sub(MAX_CONSOLE_ENTRIES);

    lines
        .into_iter()
        .skip(start)
        .enumerate()
        .filter_map(|(index, line)| parse_console_file_entry(index as u64 + 1, &line))
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_console_file_entry(sequence: u64, line: &str) -> Option<ConsoleEntry> {
    let mut parts = line.splitn(3, '\t');
    let level = console_level_from_file_label(parts.next()?)?;
    let target = unescape_console_field(parts.next()?);
    let message = unescape_console_field(parts.next().unwrap_or_default());

    Some(ConsoleEntry {
        sequence,
        level,
        target,
        message,
        time: SystemTime::UNIX_EPOCH,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn console_level_file_label(level: ConsoleLevel) -> &'static str {
    match level {
        ConsoleLevel::Trace => "TRACE",
        ConsoleLevel::Debug => "DEBUG",
        ConsoleLevel::Info => "INFO",
        ConsoleLevel::Warn => "WARN",
        ConsoleLevel::Error => "ERROR",
        ConsoleLevel::Panic => "PANIC",
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn console_level_from_file_label(label: &str) -> Option<ConsoleLevel> {
    match label {
        "TRACE" => Some(ConsoleLevel::Trace),
        "DEBUG" => Some(ConsoleLevel::Debug),
        "INFO" => Some(ConsoleLevel::Info),
        "WARN" => Some(ConsoleLevel::Warn),
        "ERROR" => Some(ConsoleLevel::Error),
        "PANIC" => Some(ConsoleLevel::Panic),
        _ => None,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn escape_console_field(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

#[cfg(not(target_arch = "wasm32"))]
fn unescape_console_field(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }

        match chars.next() {
            Some('t') => output.push('\t'),
            Some('r') => output.push('\r'),
            Some('n') => output.push('\n'),
            Some('\\') => output.push('\\'),
            Some(other) => {
                output.push('\\');
                output.push(other);
            }
            None => output.push('\\'),
        }
    }

    output
}

#[derive(Debug)]
struct BacktraceFrame {
    symbol: String,
    location: Option<String>,
}

fn format_panic_report(info: &std::panic::PanicHookInfo<'_>) -> String {
    let backtrace = std::backtrace::Backtrace::force_capture();
    let full_backtrace = backtrace.to_string();
    let frames = parse_backtrace_frames(&full_backtrace);
    let project_frames: Vec<_> = frames
        .iter()
        .filter(|frame| is_project_frame(frame) && !is_logging_frame(frame))
        .collect();
    let use_color = should_color_panic_report();

    let mut report = String::new();
    push_colored(&mut report, "panic:", Color::Red, use_color);
    report.push(' ');
    report.push_str(&panic_message(info));
    report.push('\n');

    if let Some(location) = info.location() {
        push_colored(
            &mut report,
            "at",
            color_for_path(location.file()),
            use_color,
        );
        report.push(' ');
        push_colored(
            &mut report,
            &format!("{}:{}", location.file(), location.line()),
            color_for_path(location.file()),
            use_color,
        );
        report.push('\n');
    }

    let shown_frames: Vec<_> = if project_frames.is_empty() {
        frames.iter().take(8).collect()
    } else {
        project_frames
    };

    if !shown_frames.is_empty() {
        report.push_str("\nproject stack:\n");
        for (index, frame) in shown_frames.iter().take(12).enumerate() {
            let color = color_for_frame(frame);
            report.push_str(&format!("  {:>2}. ", index + 1));
            push_colored(&mut report, compact_symbol(&frame.symbol), color, use_color);
            if let Some(location) = frame.location.as_deref() {
                report.push_str(" (");
                push_colored(&mut report, &compact_path(location), color, use_color);
                report.push(')');
            }
            report.push('\n');
        }
    }

    let hidden_frames = frames.len().saturating_sub(shown_frames.len().min(12));
    if should_print_full_panic_backtrace() {
        report.push_str("\nfull backtrace:\n");
        report.push_str(&full_backtrace);
    } else if hidden_frames > 0 {
        report.push_str(&format!(
            "\nfull backtrace hidden: {hidden_frames} less relevant frame(s). Set PLAXEL_PANIC_BACKTRACE=full to print everything.\n"
        ));
    }

    report
}

#[derive(Clone, Copy)]
enum Color {
    Engine,
    Game,
    Editor,
    Graphics,
    Red,
}

impl Color {
    fn ansi_code(self) -> &'static str {
        match self {
            Color::Engine => "32",
            Color::Game => "33",
            Color::Editor => "34",
            Color::Graphics | Color::Red => "31",
        }
    }
}

fn push_colored(output: &mut String, text: &str, color: Color, enabled: bool) {
    if enabled {
        output.push_str("\x1b[");
        output.push_str(color.ansi_code());
        output.push_str(";2m");
        output.push_str(text);
        output.push_str("\x1b[0m");
    } else {
        output.push_str(text);
    }
}

fn panic_message(info: &std::panic::PanicHookInfo<'_>) -> String {
    if let Some(message) = info.payload().downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = info.payload().downcast_ref::<String>() {
        message.clone()
    } else {
        "<non-string panic payload>".to_owned()
    }
}

fn parse_backtrace_frames(backtrace: &str) -> Vec<BacktraceFrame> {
    let mut frames = Vec::new();

    for line in backtrace.lines() {
        if let Some(symbol) = parse_frame_symbol(line) {
            frames.push(BacktraceFrame {
                symbol: symbol.to_owned(),
                location: None,
            });
        } else if let Some(location) = parse_frame_location(line) {
            if let Some(frame) = frames.last_mut() {
                frame.location = Some(location.to_owned());
            }
        }
    }

    frames
}

fn parse_frame_symbol(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let (number, symbol) = trimmed.split_once(':')?;
    if !number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit()) {
        Some(symbol.trim())
    } else {
        None
    }
}

fn parse_frame_location(line: &str) -> Option<&str> {
    line.trim_start().strip_prefix("at ").map(str::trim)
}

fn is_project_frame(frame: &BacktraceFrame) -> bool {
    is_project_symbol(&frame.symbol)
        || frame
            .location
            .as_deref()
            .is_some_and(|location| is_project_path(location))
}

fn is_project_symbol(symbol: &str) -> bool {
    matches!(
        symbol.split("::").next(),
        Some("engine")
            | Some("engine_dylib")
            | Some("editor_runner")
            | Some("editor_logic")
            | Some("game_runner")
            | Some("game_logic")
            | Some("game_types")
    )
}

fn is_logging_frame(frame: &BacktraceFrame) -> bool {
    frame.symbol.starts_with("engine::logging::")
        || frame.symbol.starts_with("logging::")
        || frame
            .location
            .as_deref()
            .is_some_and(|location| location.contains("\\engine\\src\\logging\\mod.rs"))
        || frame
            .location
            .as_deref()
            .is_some_and(|location| location.contains("/engine/src/logging/mod.rs"))
}

fn is_project_path(path: &str) -> bool {
    [
        "\\engine\\src\\",
        "\\editor\\runner\\src\\",
        "\\editor\\logic\\src\\",
        "\\game\\runner\\src\\",
        "\\game\\logic\\src\\",
        "\\game\\types\\src\\",
        "/engine/src/",
        "/editor/runner/src/",
        "/editor/logic/src/",
        "/game/runner/src/",
        "/game/logic/src/",
        "/game/types/src/",
    ]
    .iter()
    .any(|needle| path.contains(needle))
}

fn compact_symbol(symbol: &str) -> &str {
    symbol
        .strip_prefix("plaxel::")
        .or_else(|| symbol.strip_prefix("engine::"))
        .map(|stripped| stripped.strip_prefix("engine::").unwrap_or(stripped))
        .unwrap_or(symbol)
}

fn compact_path(path: &str) -> String {
    for marker in [
        "\\engine\\",
        "\\editor\\",
        "\\game\\",
        "/engine/",
        "/editor/",
        "/game/",
    ] {
        if let Some(index) = path.find(marker) {
            return path[index + 1..].to_owned();
        }
    }

    path.to_owned()
}

fn color_for_frame(frame: &BacktraceFrame) -> Color {
    color_for_name_or_path(&frame.symbol)
        .or_else(|| frame.location.as_deref().and_then(color_for_name_or_path))
        .unwrap_or(Color::Engine)
}

fn color_for_path(path: &str) -> Color {
    color_for_name_or_path(path).unwrap_or(Color::Engine)
}

fn color_for_name_or_path(text: &str) -> Option<Color> {
    let text = text.to_ascii_lowercase();

    if text.contains("wgpu")
        || text.contains("naga")
        || text.contains("renderdoc")
        || text.contains("renderer\\backends")
        || text.contains("renderer/backends")
    {
        Some(Color::Graphics)
    } else if text.starts_with("game_")
        || text.contains("\\game\\")
        || text.contains("/game/")
        || text.contains("::game::")
    {
        Some(Color::Game)
    } else if text.starts_with("editor_")
        || text.contains("\\editor\\")
        || text.contains("/editor/")
        || text.contains("::editor::")
    {
        Some(Color::Editor)
    } else if text.starts_with("engine")
        || text.contains("\\engine\\")
        || text.contains("/engine/")
        || text.contains("::engine::")
    {
        Some(Color::Engine)
    } else {
        None
    }
}

fn should_print_full_panic_backtrace() -> bool {
    std::env::var("PLAXEL_PANIC_BACKTRACE")
        .map(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "full"))
        .unwrap_or(false)
}

fn should_color_panic_report() -> bool {
    std::env::var_os("NO_COLOR").is_none()
        && std::env::var("PLAXEL_PANIC_COLOR")
            .map(|value| !matches!(value.to_ascii_lowercase().as_str(), "0" | "false" | "off"))
            .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_backtrace_frames() {
        let frames = parse_backtrace_frames(
            r#"   0: std::backtrace_rs::backtrace::win64::trace
             at /rustc/library\std\src\backtrace.rs:85
   1: engine::renderer::core::Renderer::init_frame_bindings
             at .\engine\src\renderer\core.rs:626
   2: editor_runner::run_editor
             at .\editor\runner\src\lib.rs:115"#,
        );

        assert_eq!(frames.len(), 3);
        assert_eq!(
            frames[1].symbol,
            "engine::renderer::core::Renderer::init_frame_bindings"
        );
        assert_eq!(
            frames[1].location.as_deref(),
            Some(".\\engine\\src\\renderer\\core.rs:626")
        );
    }

    #[test]
    fn identifies_project_frames_by_symbol_or_path() {
        let project_symbol = BacktraceFrame {
            symbol: "game_logic::update".to_owned(),
            location: None,
        };
        let project_path = BacktraceFrame {
            symbol: "closure$0".to_owned(),
            location: Some("C:\\repo\\plaxel\\engine\\src\\lib.rs:142".to_owned()),
        };
        let dependency = BacktraceFrame {
            symbol: "winit::event_loop::run_app".to_owned(),
            location: Some(
                "C:\\Users\\me\\.cargo\\registry\\src\\winit\\event_loop.rs:265".to_owned(),
            ),
        };

        assert!(is_project_frame(&project_symbol));
        assert!(is_project_frame(&project_path));
        assert!(!is_project_frame(&dependency));
    }

    #[test]
    fn hides_logging_frames_from_project_stack() {
        let logging_frame = BacktraceFrame {
            symbol: "engine::logging::format_panic_report".to_owned(),
            location: Some(".\\engine\\src\\logging\\mod.rs:64".to_owned()),
        };
        let renderer_frame = BacktraceFrame {
            symbol: "engine::renderer::core::Renderer::init".to_owned(),
            location: Some(".\\engine\\src\\renderer\\core.rs:536".to_owned()),
        };

        assert!(is_logging_frame(&logging_frame));
        assert!(!is_logging_frame(&renderer_frame));
    }

    #[test]
    fn compacts_project_paths() {
        assert_eq!(
            compact_path("C:\\repo\\plaxel\\engine\\src\\renderer\\core.rs:626"),
            "engine\\src\\renderer\\core.rs:626"
        );
    }

    #[test]
    fn colors_known_frame_categories() {
        assert!(matches!(
            color_for_name_or_path("engine::renderer::core::Renderer::init"),
            Some(Color::Engine)
        ));
        assert!(matches!(
            color_for_name_or_path("game_logic::update"),
            Some(Color::Game)
        ));
        assert!(matches!(
            color_for_name_or_path("editor_runner::run_editor"),
            Some(Color::Editor)
        ));
        assert!(matches!(
            color_for_name_or_path("wgpu::backend::wgpu_core"),
            Some(Color::Graphics)
        ));
    }
}
