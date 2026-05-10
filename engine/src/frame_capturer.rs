#[cfg(feature = "renderdoc")]
use std::env;
#[cfg(feature = "renderdoc")]
use crate::engine_warn;

pub struct FrameCapturer {
    #[cfg(feature = "renderdoc")]
    renderdoc: Option<renderdoc::RenderDoc<renderdoc::V141>>,
    capture_next_frame: bool,
}

impl FrameCapturer {
    pub fn new() -> Self {
        #[cfg(feature = "renderdoc")]
        let renderdoc = {
            configure_renderdoc_environment();

            let mut renderdoc: Option<renderdoc::RenderDoc<renderdoc::V141>> =
                renderdoc::RenderDoc::new().ok();

            if let Some(renderdoc) = renderdoc.as_mut() {
                use renderdoc::OverlayBits;
                renderdoc.mask_overlay_bits(OverlayBits::empty(), OverlayBits::empty());
            }

            renderdoc
        };

        Self {
            #[cfg(feature = "renderdoc")]
            renderdoc,
            capture_next_frame: false,
        }
    }

    pub fn request_capture(&mut self) {
        #[cfg(feature = "renderdoc")]
        if let Some(renderdoc) = self.renderdoc.as_mut() {
            renderdoc.start_frame_capture(std::ptr::null(), std::ptr::null());
            self.capture_next_frame = true;
        }
    }

    pub fn finish_capture_after_frame(&mut self) {
        if !self.capture_next_frame {
            return;
        }

        #[cfg(feature = "renderdoc")]
        if let Some(renderdoc) = self.renderdoc.as_mut() {
            let null = std::ptr::null();

            renderdoc.end_frame_capture(null, null);

            let num = renderdoc.get_num_captures();
            if num > 0 {
                if let Some((path, _)) = renderdoc.get_capture(num - 1) {
                    println!("Opening capture: {:?}", path);
                    renderdoc.launch_replay_ui(true, path.to_str()).ok();
                }
            }
        }

        self.capture_next_frame = false;
    }
}

impl Default for FrameCapturer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "renderdoc")]
fn configure_renderdoc_environment() {
    unsafe {
        let dll_loaded = libloading::Library::new("renderdoc.dll").is_ok();
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()));

        if dll_loaded && let Some(dir) = exe_dir {
            let dir_str = dir.to_string_lossy();

            env::set_var("ENABLE_VULKAN_RENDERDOC_CAPTURE", "1");

            let existing = env::var("VK_ADD_IMPLICIT_LAYER_PATH").unwrap_or_default();

            let new_path = if existing.is_empty() {
                dir_str.to_string()
            } else {
                format!("{};{}", dir_str, existing)
            };

            env::set_var("VK_ADD_IMPLICIT_LAYER_PATH", new_path);
        } else {
            engine_warn!(
                "renderdoc.dll not found, ensure renderdoc.dll can be found in the executable directory. Renderdoc is disabled!."
            );
        }
    }
}
