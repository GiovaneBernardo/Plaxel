use std::sync::Arc;

use winit::window::Window;

use crate::{
    assets::manager::AssetManager,
    core::input::InputState,
    frame_capturer::FrameCapturer,
    renderer::{self, Renderer},
};

pub struct GlobalResources {
    pub renderer: renderer::Renderer,
    pub asset_manager: AssetManager,
    pub frame_capturer: FrameCapturer,
    pub input: InputState,
}

impl GlobalResources {
    pub async fn new(window: Arc<Window>) -> Self {
        let renderer = Renderer::new(window.clone()).await.unwrap();

        Self {
            asset_manager: AssetManager::new(),
            frame_capturer: FrameCapturer::new(),
            input: InputState::new(),
            renderer,
        }
    }
}
