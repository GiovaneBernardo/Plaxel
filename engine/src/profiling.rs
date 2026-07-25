use std::{
    collections::VecDeque,
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub mod cpu;

const MAX_FRAMES: usize = 120;
const MAX_SCOPES_PER_FRAME: usize = 1024;
const MAX_COUNTERS_PER_FRAME: usize = 256;

static ENABLED: AtomicBool = AtomicBool::new(cfg!(feature = "profiling"));
static PROFILER: LazyLock<Mutex<Profiler>> = LazyLock::new(|| Mutex::new(Profiler::new()));

#[cfg(feature = "tracy")]
static TRACY_CLIENT: LazyLock<Mutex<Option<tracy_client::Client>>> =
    LazyLock::new(|| Mutex::new(None));

#[derive(Clone, Debug)]
pub struct ScopeSample {
    pub name: String,
    pub duration_us: f64,
}

#[derive(Clone, Debug)]
pub struct CounterSample {
    pub name: String,
    pub value: f64,
}

#[derive(Clone, Debug)]
pub struct FrameSample {
    pub index: u64,
    pub total_us: f64,
    pub scopes: Vec<ScopeSample>,
    pub counters: Vec<CounterSample>,
}

#[derive(Clone, Debug)]
pub struct ScopeSummary {
    pub name: String,
    pub calls: u32,
    pub total_us: f64,
    pub max_us: f64,
}

#[derive(Clone, Debug)]
pub struct ProfileSnapshot {
    pub enabled: bool,
    pub tracy_enabled: bool,
    pub puffin_enabled: bool,
    pub frames: Vec<FrameSample>,
    pub latest_frame: Option<FrameSample>,
    pub latest_scopes: Vec<ScopeSummary>,
    pub average_frame_us: f64,
    pub max_frame_us: f64,
    pub cpu: cpu::CpuProfileSnapshot,
}

impl Default for ProfileSnapshot {
    fn default() -> Self {
        Self {
            enabled: is_enabled(),
            tracy_enabled: cfg!(feature = "tracy"),
            puffin_enabled: cfg!(feature = "puffin"),
            frames: Vec::new(),
            latest_frame: None,
            latest_scopes: Vec::new(),
            average_frame_us: 0.0,
            max_frame_us: 0.0,
            cpu: cpu::snapshot(),
        }
    }
}

struct Profiler {
    frames: VecDeque<FrameSample>,
    current: Option<FrameSample>,
    frame_start: Option<Instant>,
}

pub struct Scope {
    name: String,
    start: Instant,
    enabled: bool,
}

impl Profiler {
    fn new() -> Self {
        Self {
            frames: VecDeque::with_capacity(MAX_FRAMES),
            current: None,
            frame_start: None,
        }
    }
}

impl Scope {
    pub fn new(name: &'static str) -> Self {
        Self::new_owned(name.to_string())
    }

    pub fn new_owned(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            start: Instant::now(),
            enabled: is_enabled(),
        }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        if self.enabled {
            record_scope(&self.name, self.start.elapsed());
        }
    }
}

pub fn init(enabled: bool) {
    set_enabled(enabled);

    #[cfg(feature = "puffin")]
    puffin::set_scopes_on(enabled);

    #[cfg(feature = "tracy")]
    {
        let mut client = TRACY_CLIENT.lock().unwrap();
        if enabled && client.is_none() {
            *client = Some(tracy_client::Client::start());
        } else if !enabled {
            *client = None;
        }
    }
}

pub fn sync_enabled(enabled: bool) {
    if is_enabled() != enabled {
        init(enabled);
    }
}

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn external_scopes_enabled() -> bool {
    is_enabled()
}

pub fn begin_frame(index: u64) {
    cpu::mark_frame(index);
    if !is_enabled() {
        return;
    }

    #[cfg(feature = "puffin")]
    puffin::GlobalProfiler::lock().new_frame();

    let mut profiler = PROFILER.lock().unwrap();
    profiler.current = Some(FrameSample {
        index,
        total_us: 0.0,
        scopes: Vec::with_capacity(128),
        counters: Vec::with_capacity(32),
    });
    profiler.frame_start = Some(Instant::now());
}

pub fn end_frame() {
    if !is_enabled() {
        return;
    }

    let mut profiler = PROFILER.lock().unwrap();
    let total_us = profiler
        .frame_start
        .take()
        .map(|start| duration_us(start.elapsed()))
        .unwrap_or_default();

    if let Some(mut frame) = profiler.current.take() {
        frame.total_us = total_us;
        profiler.frames.push_back(frame);
        while profiler.frames.len() > MAX_FRAMES {
            profiler.frames.pop_front();
        }
    }

    #[cfg(feature = "tracy")]
    if let Some(client) = tracy_client::Client::running() {
        client.frame_mark();
    }
}

pub fn record_counter(name: &'static str, value: f64) {
    if !is_enabled() {
        return;
    }

    let mut profiler = PROFILER.lock().unwrap();
    if let Some(frame) = profiler.current.as_mut() {
        if frame.counters.len() < MAX_COUNTERS_PER_FRAME {
            frame.counters.push(CounterSample {
                name: name.to_string(),
                value,
            });
        }
    }
}

pub fn snapshot() -> ProfileSnapshot {
    cpu::poll();
    let profiler = PROFILER.lock().unwrap();
    let frames = profiler.frames.iter().cloned().collect::<Vec<_>>();
    let latest_frame = frames.last().cloned();
    let latest_scopes = latest_frame
        .as_ref()
        .map(|frame| summarize_scopes(&frame.scopes))
        .unwrap_or_default();

    let average_frame_us = if frames.is_empty() {
        0.0
    } else {
        frames.iter().map(|frame| frame.total_us).sum::<f64>() / frames.len() as f64
    };
    let max_frame_us = frames
        .iter()
        .map(|frame| frame.total_us)
        .fold(0.0_f64, f64::max);

    ProfileSnapshot {
        enabled: is_enabled(),
        tracy_enabled: cfg!(feature = "tracy"),
        puffin_enabled: cfg!(feature = "puffin"),
        frames,
        latest_frame,
        latest_scopes,
        average_frame_us,
        max_frame_us,
        cpu: cpu::snapshot(),
    }
}

fn record_scope(name: &str, duration: Duration) {
    let mut profiler = PROFILER.lock().unwrap();
    if let Some(frame) = profiler.current.as_mut() {
        if frame.scopes.len() < MAX_SCOPES_PER_FRAME {
            frame.scopes.push(ScopeSample {
                name: name.to_string(),
                duration_us: duration_us(duration),
            });
        }
    }
}

fn summarize_scopes(scopes: &[ScopeSample]) -> Vec<ScopeSummary> {
    let mut summaries = Vec::<ScopeSummary>::new();
    for scope in scopes {
        if let Some(summary) = summaries.iter_mut().find(|item| item.name == scope.name) {
            summary.calls += 1;
            summary.total_us += scope.duration_us;
            summary.max_us = summary.max_us.max(scope.duration_us);
        } else {
            summaries.push(ScopeSummary {
                name: scope.name.clone(),
                calls: 1,
                total_us: scope.duration_us,
                max_us: scope.duration_us,
            });
        }
    }

    summaries.sort_by(|a, b| {
        b.total_us
            .partial_cmp(&a.total_us)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    summaries
}

fn duration_us(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000_000.0
}

#[macro_export]
macro_rules! profile_scope {
    ($name:expr) => {
        #[cfg(feature = "puffin")]
        puffin::profile_scope!($name);

        #[cfg(feature = "tracy")]
        let _plaxel_tracy_span = if $crate::profiling::external_scopes_enabled() {
            tracy_client::Client::running()
                .map(|client| client.span_alloc(Some($name), module_path!(), file!(), line!(), 0))
        } else {
            None
        };

        let _plaxel_profile_scope = $crate::profiling::Scope::new($name);
    };
}

#[macro_export]
macro_rules! profile_counter {
    ($name:expr, $value:expr) => {
        $crate::profiling::record_counter($name, $value as f64);
    };
}
