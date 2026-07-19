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
        match self.renderdoc.as_mut() {
            Some(renderdoc) => {
                self.capture_count_before_request = renderdoc.get_num_captures();
                self.capture_poll_frames_remaining = 120;
                renderdoc.trigger_capture();
                self.capture_next_frame = true;
            }
            None => {
                engine_warn!("cannot capture GPU frame because RenderDoc is unavailable");
            }
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

                engine_info!("Opening capture: {:?}", path);
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

        // `dx serve` recreates its staging directory after Cargo build scripts
        // have run, so files copied beside the Cargo artifact do not survive in
        // `target/dx/.../app`. Prefer a staged DLL when present (for packaged
        // builds), then load the repository's development copy directly.
        let dev_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../res/dev");
        let renderdoc_dir = exe_dir
            .filter(|dir| dir.join("renderdoc.dll").is_file())
            .or_else(|| {
                dev_dir
                    .join("renderdoc.dll")
                    .is_file()
                    .then_some(dev_dir.clone())
            });

        if let Some(dir) = renderdoc_dir {
            let dll_path = renderdoc_dll_path(&dir);

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

            match libloading::Library::new(&dll_path) {
                Ok(library) => {
                    engine_info!("Loaded RenderDoc library from {}", dll_path.display());
                    Some(library)
                }
                Err(error) => {
                    engine_warn!(
                        "failed to load RenderDoc library from {}: {error}",
                        dll_path.display()
                    );
                    None
                }
            }
        } else {
            engine_warn!(
                "renderdoc.dll was not found beside the executable or in {}, RenderDoc is disabled",
                dev_dir.display()
            );
            None
        }
    }
}

#[cfg(feature = "renderdoc")]
fn renderdoc_dll_path(exe_dir: &std::path::Path) -> PathBuf {
    exe_dir.join("renderdoc.dll")
}
