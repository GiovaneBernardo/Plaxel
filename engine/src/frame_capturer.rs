#[cfg(feature = "renderdoc")]
use crate::{engine_info, engine_warn};
#[cfg(feature = "renderdoc")]
use std::{env, path::PathBuf};

pub struct FrameCapturer {
    #[cfg(feature = "renderdoc")]
    renderdoc: Option<renderdoc::RenderDoc<renderdoc::V141>>,
    #[cfg(feature = "renderdoc")]
    _renderdoc_library: Option<libloading::Library>,
    #[cfg(feature = "renderdoc")]
    capture_count_before_request: u32,
    #[cfg(feature = "renderdoc")]
    capture_poll_frames_remaining: u32,
    capture_next_frame: bool,
}

impl FrameCapturer {
    pub fn new() -> Self {
        #[cfg(feature = "renderdoc")]
        let (renderdoc, renderdoc_library) = {
            let renderdoc_library = configure_renderdoc_environment();

            let mut renderdoc: Option<renderdoc::RenderDoc<renderdoc::V141>> =
                renderdoc::RenderDoc::new().ok();

            if let Some(renderdoc) = renderdoc.as_mut() {
                use renderdoc::OverlayBits;
                renderdoc.mask_overlay_bits(OverlayBits::empty(), OverlayBits::empty());
                let (major, minor, patch) = renderdoc.get_api_version();
                engine_info!("RenderDoc API loaded: {major}.{minor}.{patch}");
            } else if renderdoc_library.is_some() {
                engine_warn!("renderdoc.dll was loaded, but RENDERDOC_GetAPI was unavailable");
            }

            (renderdoc, renderdoc_library)
        };

        Self {
            #[cfg(feature = "renderdoc")]
            renderdoc,
            #[cfg(feature = "renderdoc")]
            _renderdoc_library: renderdoc_library,
            #[cfg(feature = "renderdoc")]
            capture_count_before_request: 0,
            #[cfg(feature = "renderdoc")]
            capture_poll_frames_remaining: 0,
            capture_next_frame: false,
        }
    }

    pub fn request_capture(&mut self) {
        #[cfg(feature = "renderdoc")]
        if let Some(renderdoc) = self.renderdoc.as_mut() {
            self.capture_count_before_request = renderdoc.get_num_captures();
            self.capture_poll_frames_remaining = 120;
            renderdoc.trigger_capture();
            self.capture_next_frame = true;
        }
    }

    pub fn finish_capture_after_frame(&mut self) {
        if !self.capture_next_frame {
            return;
        }

        #[cfg(feature = "renderdoc")]
        if let Some(renderdoc) = self.renderdoc.as_mut() {
            let num = renderdoc.get_num_captures();
            if num > self.capture_count_before_request {
                let capture_index = num - 1;
                let Some((path, _)) = renderdoc.get_capture(capture_index) else {
                    return;
                };

                println!("Opening capture: {:?}", path);
                renderdoc.launch_replay_ui(true, path.to_str()).ok();
            } else if self.capture_poll_frames_remaining > 0 {
                self.capture_poll_frames_remaining -= 1;
                return;
            } else {
                engine_warn!("RenderDoc did not report a completed capture");
            }
        }

        self.capture_next_frame = false;
    }
}

impl Drop for FrameCapturer {
    fn drop(&mut self) {
        #[cfg(feature = "renderdoc")]
        if let Some(renderdoc) = self.renderdoc.as_mut() {
            if renderdoc.is_frame_capturing() {
                renderdoc.discard_frame_capture(std::ptr::null(), std::ptr::null());
            }
            renderdoc.unload_crash_handler();
        }
    }
}

impl Default for FrameCapturer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "renderdoc")]
fn configure_renderdoc_environment() -> Option<libloading::Library> {
    unsafe {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()));

        if let Some(dir) = exe_dir {
            let dll_path = dir.join("renderdoc.dll");
            if !dll_path.is_file() {
                engine_warn!(
                    "renderdoc.dll not found at {}, RenderDoc is disabled",
                    dll_path.display()
                );
                return None;
            }

            let layer_manifest_path = dir.join("renderdoc.json");
            if !layer_manifest_path.is_file() {
                engine_warn!(
                    "renderdoc.json not found at {}, Vulkan capture layer may not load",
                    layer_manifest_path.display()
                );
            }

            let dir_str = dir.to_string_lossy();

            env::set_var("ENABLE_VULKAN_RENDERDOC_CAPTURE", "1");

            let existing = env::var("VK_ADD_IMPLICIT_LAYER_PATH").unwrap_or_default();

            let new_path = if existing.is_empty() {
                dir_str.to_string()
            } else {
                format!("{};{}", dir_str, existing)
            };

            env::set_var("VK_ADD_IMPLICIT_LAYER_PATH", new_path);

            match libloading::Library::new(renderdoc_dll_path(&dir)) {
                Ok(library) => Some(library),
                Err(error) => {
                    engine_warn!("failed to load renderdoc.dll: {error}");
                    None
                }
            }
        } else {
            engine_warn!("unable to resolve executable directory, RenderDoc is disabled");
            None
        }
    }
}

#[cfg(feature = "renderdoc")]
fn renderdoc_dll_path(exe_dir: &std::path::Path) -> PathBuf {
    exe_dir.join("renderdoc.dll")
}
