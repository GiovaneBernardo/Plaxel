use std::sync::Arc;

use winit::window::Window;

use crate::{
    assets::manager::AssetManager,
    core::input::InputState,
    frame_capturer::FrameCapturer,
    multithreading::job_system::JobSystem,
    profiling::ProfileSnapshot,
    renderer::{self, Renderer},
};

pub struct GlobalResources {
    pub renderer: renderer::Renderer,
    pub asset_manager: AssetManager,
    pub frame_capturer: FrameCapturer,
    pub input: InputState,
    pub job_system: JobSystem,
    pub profiling_enabled: bool,
    pub profiler_snapshot: ProfileSnapshot,
}

impl GlobalResources {
    pub fn for_each_reflected_mut(
        &mut self,
        mut visit: impl FnMut(&'static str, &mut dyn crate::reflect::PartialReflect),
    ) {
        visit("input", &mut self.input);
        visit("profiling_enabled", &mut self.profiling_enabled);
    }

    pub async fn new(window: Arc<Window>) -> Self {
        let frame_capturer = FrameCapturer::new();
        let renderer = Renderer::new(window.clone()).await.unwrap();

        let worker_count = (num_cpus::get() - 1).max(1);
        Self {
            asset_manager: AssetManager::new(),
            frame_capturer,
            input: InputState::new(),
            renderer,
            job_system: JobSystem::new(worker_count),
            profiling_enabled: true,
            profiler_snapshot: ProfileSnapshot::default(),
        }
    }
}
