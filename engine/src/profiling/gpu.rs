use std::{
    collections::VecDeque,
    sync::{LazyLock, Mutex},
};

const MAX_GPU_FRAMES: usize = 120;

#[derive(Clone, Copy, Debug, Default)]
pub struct GpuPipelineStatistics {
    pub vertex_shader_invocations: u64,
    pub clipper_invocations: u64,
    pub clipper_primitives_out: u64,
    pub fragment_shader_invocations: u64,
    pub compute_shader_invocations: u64,
}

#[derive(Clone, Debug, Default)]
pub struct GpuPassSample {
    pub name: String,
    pub duration_ms: Option<f64>,
    pub statistics: Option<GpuPipelineStatistics>,
}

#[derive(Clone, Debug, Default)]
pub struct GpuFrameSample {
    pub index: u64,
    pub passes: Vec<GpuPassSample>,
    pub summed_pass_ms: f64,
}

#[derive(Clone, Debug, Default)]
pub struct GpuProfileSnapshot {
    pub timestamp_supported: bool,
    pub pipeline_statistics_supported: bool,
    pub readback_latency_frames: u32,
    pub latest_frame: Option<GpuFrameSample>,
    pub frames: Vec<GpuFrameSample>,
}

#[derive(Default)]
struct GpuProfilerState {
    timestamp_supported: bool,
    pipeline_statistics_supported: bool,
    readback_latency_frames: u32,
    frames: VecDeque<GpuFrameSample>,
}

static GPU_PROFILER: LazyLock<Mutex<GpuProfilerState>> =
    LazyLock::new(|| Mutex::new(GpuProfilerState::default()));

pub fn configure(timestamp_supported: bool, pipeline_statistics_supported: bool, latency: u32) {
    let mut profiler = GPU_PROFILER.lock().unwrap();
    profiler.timestamp_supported = timestamp_supported;
    profiler.pipeline_statistics_supported = pipeline_statistics_supported;
    profiler.readback_latency_frames = latency;
}

pub fn publish(frame: GpuFrameSample) {
    let mut profiler = GPU_PROFILER.lock().unwrap();
    profiler.frames.push_back(frame);
    while profiler.frames.len() > MAX_GPU_FRAMES {
        profiler.frames.pop_front();
    }
}

pub fn snapshot() -> GpuProfileSnapshot {
    let profiler = GPU_PROFILER.lock().unwrap();
    let frames = profiler.frames.iter().cloned().collect::<Vec<_>>();
    GpuProfileSnapshot {
        timestamp_supported: profiler.timestamp_supported,
        pipeline_statistics_supported: profiler.pipeline_statistics_supported,
        readback_latency_frames: profiler.readback_latency_frames,
        latest_frame: frames.last().cloned(),
        frames,
    }
}
