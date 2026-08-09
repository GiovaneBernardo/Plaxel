use std::{
    cell::Cell,
    collections::{HashMap, VecDeque},
    hash::{DefaultHasher, Hash, Hasher},
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

pub mod cpu;
pub mod gpu;

const MAX_FRAMES: usize = 120;
// Terrain generation can fan out across many chunk jobs in one frame. Keep
// enough samples to retain the nested phase scopes instead of silently making
// late-running workers look uninstrumented.
const MAX_SCOPES_PER_FRAME: usize = 4096;
const MAX_COUNTERS_PER_FRAME: usize = 256;

static ENABLED: AtomicBool = AtomicBool::new(cfg!(feature = "profiling"));
static PROFILER: LazyLock<Mutex<Profiler>> = LazyLock::new(|| Mutex::new(Profiler::new()));
static CLOCK_START: LazyLock<Instant> = LazyLock::new(Instant::now);
static NEXT_SCOPE_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static SCOPE_DEPTH: Cell<u16> = const { Cell::new(0) };
}

#[cfg(feature = "tracy")]
static TRACY_CLIENT: LazyLock<Mutex<Option<tracy_client::Client>>> =
    LazyLock::new(|| Mutex::new(None));

#[derive(Clone, Debug)]
pub struct ScopeSample {
    pub name: String,
    pub duration_us: f64,
    pub start_us: f64,
    pub depth: u16,
    pub thread_id: u64,
    pub thread_name: String,
    pub processor_start: Option<u32>,
    pub processor_end: Option<u32>,
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
    pub gpu: gpu::GpuProfileSnapshot,
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
            gpu: gpu::snapshot(),
        }
    }
}

struct Profiler {
    frames: VecDeque<FrameSample>,
    current: Option<FrameSample>,
    frame_start: Option<Instant>,
    frame_start_clock_us: u64,
    active_scopes: HashMap<u64, ActiveScope>,
}

pub struct Scope {
    id: u64,
    enabled: bool,
}

#[derive(Clone)]
struct ActiveScope {
    name: String,
    start_clock_us: u64,
    depth: u16,
    thread_id: u64,
    thread_name: String,
    processor_start: Option<u32>,
}

impl Profiler {
    fn new() -> Self {
        Self {
            frames: VecDeque::with_capacity(MAX_FRAMES),
            current: None,
            frame_start: None,
            frame_start_clock_us: 0,
            active_scopes: HashMap::new(),
        }
    }
}

impl Scope {
    pub fn new(name: &'static str) -> Self {
        Self::new_owned(name)
    }

    pub fn new_owned(name: impl Into<String>) -> Self {
        let enabled = is_enabled();
        if !enabled {
            return Self {
                id: 0,
                enabled: false,
            };
        }

        let start_clock_us = micros_since_clock(Instant::now());
        let depth = if enabled {
            SCOPE_DEPTH.with(|depth| {
                let current = depth.get();
                depth.set(current.saturating_add(1));
                current
            })
        } else {
            0
        };
        let thread = std::thread::current();
        let id = NEXT_SCOPE_ID.fetch_add(1, Ordering::Relaxed);
        PROFILER.lock().unwrap().active_scopes.insert(
            id,
            ActiveScope {
                name: name.into(),
                start_clock_us,
                depth,
                thread_id: thread_id(&thread),
                thread_name: thread.name().unwrap_or("unnamed").to_string(),
                processor_start: current_processor(),
            },
        );
        Self { id, enabled }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        if self.enabled {
            SCOPE_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
            let end_clock_us = micros_since_clock(Instant::now());
            let mut profiler = PROFILER.lock().unwrap();
            if let Some(active) = profiler.active_scopes.remove(&self.id) {
                let frame_start = profiler.frame_start_clock_us;
                if let Some(frame) = profiler.current.as_mut()
                    && frame.scopes.len() < MAX_SCOPES_PER_FRAME
                    && end_clock_us > frame_start
                {
                    let start_clock_us = active.start_clock_us.max(frame_start);
                    frame.scopes.push(scope_sample(
                        &active,
                        start_clock_us,
                        end_clock_us,
                        frame_start,
                        current_processor(),
                    ));
                }
            }
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

    let now = Instant::now();
    let mut profiler = PROFILER.lock().unwrap();
    profiler.frame_start_clock_us = micros_since_clock(now);
    profiler.current = Some(FrameSample {
        index,
        total_us: 0.0,
        scopes: Vec::with_capacity(128),
        counters: Vec::with_capacity(32),
    });
    profiler.frame_start = Some(now);
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
        let frame_end_clock_us = micros_since_clock(Instant::now());
        for active in profiler.active_scopes.values() {
            if frame.scopes.len() >= MAX_SCOPES_PER_FRAME {
                break;
            }
            let start_clock_us = active.start_clock_us.max(profiler.frame_start_clock_us);
            if frame_end_clock_us > start_clock_us {
                frame.scopes.push(scope_sample(
                    active,
                    start_clock_us,
                    frame_end_clock_us,
                    profiler.frame_start_clock_us,
                    None,
                ));
            }
        }
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
        gpu: gpu::snapshot(),
    }
}

fn micros_since_clock(instant: Instant) -> u64 {
    instant
        .saturating_duration_since(*CLOCK_START)
        .as_micros()
        .min(u128::from(u64::MAX)) as u64
}

fn scope_sample(
    active: &ActiveScope,
    start_clock_us: u64,
    end_clock_us: u64,
    frame_start_clock_us: u64,
    processor_end: Option<u32>,
) -> ScopeSample {
    ScopeSample {
        name: active.name.clone(),
        duration_us: end_clock_us.saturating_sub(start_clock_us) as f64,
        start_us: start_clock_us.saturating_sub(frame_start_clock_us) as f64,
        depth: active.depth,
        thread_id: active.thread_id,
        thread_name: active.thread_name.clone(),
        processor_start: active.processor_start,
        processor_end,
    }
}

fn thread_id(thread: &std::thread::Thread) -> u64 {
    let mut hasher = DefaultHasher::new();
    thread.id().hash(&mut hasher);
    hasher.finish()
}

#[cfg(windows)]
fn current_processor() -> Option<u32> {
    Some(unsafe { windows_sys::Win32::System::Threading::GetCurrentProcessorNumber() })
}

#[cfg(not(windows))]
fn current_processor() -> Option<u32> {
    None
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

/// Profiles a scope whose display name is only known at runtime.
///
/// `$category` remains stable for profilers that identify a scope by call site, while
/// `$name` is used as the detailed label in Tracy and the built-in frame profiler and
/// as Puffin's per-scope data.
#[macro_export]
macro_rules! profile_dynamic_scope {
    ($category:expr, $name:expr) => {
        let _plaxel_profile_name = $crate::profiling::is_enabled().then(|| $name);

        #[cfg(feature = "puffin")]
        puffin::profile_scope_if!(
            _plaxel_profile_name.is_some(),
            $category,
            _plaxel_profile_name.as_deref().unwrap_or("")
        );

        #[cfg(feature = "tracy")]
        let _plaxel_tracy_span = if $crate::profiling::external_scopes_enabled() {
            _plaxel_profile_name.as_deref().and_then(|name| {
                tracy_client::Client::running().map(|client| {
                    client.span_alloc(Some(name), module_path!(), file!(), line!(), 0)
                })
            })
        } else {
            None
        };

        let _plaxel_profile_scope = _plaxel_profile_name
            .as_ref()
            .map(|name| $crate::profiling::Scope::new_owned(name.clone()));
    };
}

#[macro_export]
macro_rules! profile_counter {
    ($name:expr, $value:expr) => {
        $crate::profiling::record_counter($name, $value as f64);
    };
}
