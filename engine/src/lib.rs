use rand::Rng;
#[cfg(feature = "renderdoc")]
use renderdoc::RenderDoc;
#[cfg(feature = "renderdoc")]
use renderdoc::V141;
use std::env;
use std::ptr;
use std::sync::Arc;

pub mod assets;
pub mod core;
pub mod logging;
pub mod renderer;

pub use core::camera;
pub use core::ecs;
pub use renderer::model;
pub use renderer::texture;

use cgmath::prelude::*;
use model::Vertex;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
use wgpu::util::DeviceExt;

pub use winit::{
    application::ApplicationHandler,
    event::*,
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::Window,
};

use crate::assets::manager::AssetManager;
use crate::core::components::core::TransformComponent;
use crate::renderer::Renderer;

// This will store the state of our game
pub struct State {
    pub window: Arc<Window>,
    pub camera: camera::Camera,
    pub camera_uniform: camera::CameraUniform,
    pub camera_controller: camera::CameraController,
    pub instances: Vec<Instance>,
    pub scene: ecs::Scene,
    pub events: Vec<WindowEvent>,
    pub renderer: renderer::Renderer,
    pub asset_manager: AssetManager,
    pub registered_systems: bool,
    pub game_data: Box<dyn std::any::Any>,
    #[cfg(feature = "renderdoc")]
    pub renderdoc: Option<renderdoc::RenderDoc<renderdoc::V141>>,
    pub capture_next_frame: bool,

    #[cfg(not(feature = "renderdoc"))]
    pub renderdoc: (),
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct InstanceRaw {
    model: [[f32; 4]; 4],
}

impl InstanceRaw {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<InstanceRaw>() as wgpu::BufferAddress,
            // We need to switch from using a step mode of Vertex to Instance
            // This means that our shaders will only change to use the next
            // instance when the shader starts processing a new instance
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                // A mat4 takes up 4 vertex slots as it is technically 4 vec4s. We need to define a slot
                // for each vec4. We'll have to reassemble the mat4 in the shader.
                wgpu::VertexAttribute {
                    offset: 0,
                    // While our vertex shader only uses locations 0, and 1 now, in later tutorials, we'll
                    // be using 2, 3, and 4, for Vertex. We'll start at slot 5, not conflict with them later
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 8]>() as wgpu::BufferAddress,
                    shader_location: 7,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 12]>() as wgpu::BufferAddress,
                    shader_location: 8,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

struct Instance {
    position: cgmath::Vector3<f32>,
    rotation: cgmath::Quaternion<f32>,
    scale: f32,
}

impl Instance {
    fn to_raw(&self) -> InstanceRaw {
        InstanceRaw {
            model: (cgmath::Matrix4::from_translation(self.position)
                * cgmath::Matrix4::from(self.rotation)
                * cgmath::Matrix4::from_scale(self.scale))
            .into(),
        }
    }
}

impl State {
    // We don't need this to be async right now,
    // but we will in the next tutorial
    pub async fn new(window: Arc<Window>) -> anyhow::Result<Self> {
        // TODO: MOVE TO A FRAME CAPTURER FILE
        // Notes: This expects the feature renderdoc, requires the renderdoc.dll, renderdoc.json and renderdoc.app files are in the executable directory and requires vulkan.
        // This is very sensible and not well implemented, in case of any issues, disable renderdoc feature
        #[cfg(feature = "renderdoc")]
        unsafe {
            let dll_loaded = libloading::Library::new("renderdoc.dll").is_ok();
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()));

            if dll_loaded && let Some(dir) = exe_dir {
                let dir_str = dir.to_string_lossy();

                // Enable RenderDoc Vulkan layer
                env::set_var("ENABLE_VULKAN_RENDERDOC_CAPTURE", "1");

                // Add implicit layer path
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
        #[cfg(feature = "renderdoc")]
        let mut renderdoc: Option<renderdoc::RenderDoc<renderdoc::V141>> =
            renderdoc::RenderDoc::new().ok();

        #[cfg(feature = "renderdoc")]
        if let Some(renderdoc) = renderdoc.as_mut() {
            use renderdoc::OverlayBits;
            renderdoc.mask_overlay_bits(OverlayBits::empty(), OverlayBits::empty());
        }

        let size = window.inner_size();

        // The instance is a handle to our GPU
        // BackendBit::PRIMARY => Vulkan + Metal + DX12 + Browser WebGPU
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            #[cfg(not(target_arch = "wasm32"))]
            backends: wgpu::Backends::PRIMARY,
            #[cfg(target_arch = "wasm32")]
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                // WebGL doesn't support all of wgpu's features, so if
                // we're building for the web we'll have to disable some.
                required_limits: if cfg!(target_arch = "wasm32") {
                    wgpu::Limits::default()
                } else {
                    wgpu::Limits::default()
                },
                memory_hints: Default::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;

        let surface_caps = surface.get_capabilities(&adapter);
        // Shader code in this tutorial assumes an sRGB surface texture. Using a different
        // one will result in all the colors coming out darker. If you want to support non
        // sRGB surfaces, you'll need to account for that when drawing to the frame.
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        let camera = camera::Camera {
            position: (0.0, 65536.0, 2.0).into(), // TODO: switch position Y back to 1
            yaw: -90.0,
            pitch: 0.0,
            front: (0.0, 0.0, -1.0).into(),
            up: cgmath::Vector3::unit_y(),
            right: cgmath::Vector3::unit_x(),
            world_up: cgmath::Vector3::unit_y(),
            eye: (0.0, 1.0, 2.0).into(),
            // have it look at the origin
            target: (0.0, 0.0, 0.0).into(),
            // which way is "up"
            aspect: config.width as f32 / config.height as f32,
            fovy: 65.0,
            znear: 0.1,
            zfar: 15000000.0, // Increased zfar, not sure if it will break something, if needed return to 15km
        };

        let mut camera_uniform = camera::CameraUniform::new();
        camera_uniform.update_view_proj(&camera);

        let camera_controller = camera::CameraController::new(0.2);

        const NUM_INSTANCES_PER_ROW: u32 = 20;
        const SPACE_BETWEEN: f32 = 48.0;
        let instances = (0..NUM_INSTANCES_PER_ROW)
            .flat_map(|z| {
                (0..NUM_INSTANCES_PER_ROW).map(move |x| {
                    let x = SPACE_BETWEEN * (x as f32 - NUM_INSTANCES_PER_ROW as f32 / 2.0);
                    let z = SPACE_BETWEEN * (z as f32 - NUM_INSTANCES_PER_ROW as f32 / 2.0);

                    let position = cgmath::Vector3 { x, y: 0.0, z };

                    let rotation = if position.is_zero() {
                        cgmath::Quaternion::from_axis_angle(
                            cgmath::Vector3::unit_z(),
                            cgmath::Deg(0.0),
                        )
                    } else {
                        cgmath::Quaternion::from_axis_angle(position.normalize(), cgmath::Deg(45.0))
                    };

                    Instance {
                        position,
                        rotation,
                        scale: 0.01,
                    }
                })
            })
            .collect::<Vec<_>>();
        let mut scene = ecs::Scene::new();
        let mut rng = rand::thread_rng();

        for y in 0..NUM_INSTANCES_PER_ROW {
            for x in 0..NUM_INSTANCES_PER_ROW {
                let mut entity = scene.create_entity();
                let mut transform_component = TransformComponent {
                    position: (x as f32 * 32.0, 0.0, y as f32 * 32.0).into(),
                    rotation: cgmath::Quaternion::from_axis_angle(
                        cgmath::Vector3::unit_y(),
                        cgmath::Deg(0.0),
                    ),
                    scale: (0.01, 0.01, 0.01).into(),
                    velocity: (0.0, 0.0, 0.0).into(),
                };

                transform_component.velocity.x += rng.gen_range(-0.01..0.01);
                transform_component.velocity.y += rng.gen_range(-0.01..0.01);
                transform_component.velocity.z += rng.gen_range(-0.01..0.01);

                //let mut mesh_renderer = core::components::renderer::MeshRenderer {
                //    model: obj_model.clone(),
                //};
                scene.add_transform_component(&entity, transform_component);
                //scene.add_mesh_renderer(&entity, mesh_renderer);
            }
        }

        let mut renderer = Renderer::new(window.clone()).await?;
        renderer.init();

        let mut asset_manager = AssetManager::new();

        Ok(Self {
            window,
            camera,
            camera_uniform,
            camera_controller,
            instances,
            scene,
            events: Vec::new(),
            renderer,
            asset_manager,
            registered_systems: false,
            game_data: Box::new(()),
            #[cfg(feature = "renderdoc")]
            renderdoc,
            #[cfg(not(feature = "renderdoc"))]
            renderdoc: (),
            capture_next_frame: false,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.renderer.resize(width, height);
            self.camera.aspect = width as f32 / height as f32;
        }
    }

    fn handle_key(&mut self, event_loop: &ActiveEventLoop, code: KeyCode, is_pressed: bool) {
        if code == KeyCode::Escape && is_pressed {
            event_loop.exit();
        } else {
            self.camera_controller.handle_key(code, is_pressed);
        }

        #[cfg(feature = "renderdoc")]
        if code == KeyCode::KeyH && is_pressed {
            if let Some(renderdoc) = self.renderdoc.as_mut() {
                renderdoc.start_frame_capture(ptr::null(), ptr::null());
                self.capture_next_frame = true;
            }
        }
    }

    fn handle_mouse_click(&mut self, button: MouseButton, is_pressed: bool) {
        if button == MouseButton::Right {
            self.camera_controller.handle_mouse_click(is_pressed);
        }
    }

    fn handle_mouse_scroll(&mut self, delta: MouseScrollDelta) {
        self.camera_controller.handle_mouse_scroll(delta);
    }

    fn update(&mut self) {
        self.camera_controller.update_camera(&mut self.camera);
        self.camera_uniform.update_view_proj(&self.camera);
        self.renderer.render_resources.insert(renderer::CameraData {
            uniform: self.camera_uniform,
        });
    }
}

pub struct App {
    #[cfg(target_arch = "wasm32")]
    proxy: Option<winit::event_loop::EventLoopProxy<State>>,
    state: Option<State>,
    on_register_system: Option<Box<dyn FnMut(&mut State)>>,
    on_update: Option<Box<dyn FnMut(&mut State)>>,
    on_key: Option<Box<dyn FnMut(&mut State, KeyCode, bool)>>,
    on_render: Option<
        Box<dyn FnMut(&wgpu::Device, &wgpu::Queue, &wgpu::TextureView, &mut wgpu::CommandEncoder)>,
    >,
}

impl App {
    pub fn new(#[cfg(target_arch = "wasm32")] event_loop: &EventLoop<State>) -> Self {
        #[cfg(target_arch = "wasm32")]
        let proxy = Some(event_loop.create_proxy());
        Self {
            state: None,
            on_register_system: None,
            on_update: None,
            on_key: None,
            on_render: None,
            #[cfg(target_arch = "wasm32")]
            proxy,
        }
    }

    pub fn with_register_system(mut self, f: impl FnMut(&mut State) + 'static) -> Self {
        self.on_register_system = Some(Box::new(f));
        self
    }

    pub fn with_update(mut self, f: impl FnMut(&mut State) + 'static) -> Self {
        self.on_update = Some(Box::new(f));
        self
    }

    pub fn with_on_key(mut self, f: impl FnMut(&mut State, KeyCode, bool) + 'static) -> Self {
        self.on_key = Some(Box::new(f));
        self
    }

    pub fn with_render(
        mut self,
        f: impl FnMut(&wgpu::Device, &wgpu::Queue, &wgpu::TextureView, &mut wgpu::CommandEncoder)
        + 'static,
    ) -> Self {
        self.on_render = Some(Box::new(f));
        self
    }
}

impl ApplicationHandler<State> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Run the loop continuously instead of sleeping between OS events —
        // we want every frame to tick update() so background workers can
        // make progress even when the player isn't moving.
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

        #[allow(unused_mut)]
        let mut window_attributes = Window::default_attributes();

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            use winit::platform::web::WindowAttributesExtWebSys;

            const CANVAS_ID: &str = "canvas";

            let window = wgpu::web_sys::window().unwrap_throw();
            let document = window.document().unwrap_throw();
            let canvas = document.get_element_by_id(CANVAS_ID).unwrap_throw();
            let html_canvas_element = canvas.unchecked_into();
            window_attributes = window_attributes.with_canvas(Some(html_canvas_element));
        }

        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());

        #[cfg(not(target_arch = "wasm32"))]
        {
            // If we are not on web we can use pollster to
            // await the
            self.state = Some(pollster::block_on(State::new(window)).unwrap());
        }

        #[cfg(target_arch = "wasm32")]
        {
            // Run the future asynchronously and use the
            // proxy to send the results to the event loop
            if let Some(proxy) = self.proxy.take() {
                wasm_bindgen_futures::spawn_local(async move {
                    assert!(
                        proxy
                            .send_event(
                                State::new(window)
                                    .await
                                    .expect("Unable to create canvas!!!")
                            )
                            .is_ok()
                    )
                });
            }
        }
    }

    #[allow(unused_mut)]
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, mut event: State) {
        #[cfg(target_arch = "wasm32")]
        {
            event.window.request_redraw();
            event.resize(
                event.window.inner_size().width,
                event.window.inner_size().height,
            );
        }
        self.state = Some(event);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let state = match &mut self.state {
            Some(canvas) => canvas,
            None => return,
        };

        if !matches!(event, WindowEvent::RedrawRequested) {
            state.events.push(event.clone());
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => state.resize(size.width, size.height),
            WindowEvent::RedrawRequested => {
                if !state.registered_systems {
                    if let Some(f) = &mut self.on_register_system {
                        f(state);
                    }
                    state.registered_systems = true;
                }
                state.window.request_redraw();
                state.update();
                if let Some(f) = &mut self.on_update {
                    f(state);
                }
                state.events.clear();
                match state.renderer.render() {
                    Ok(_) => {
                        let mut state = self.state.as_mut().unwrap();
                        #[cfg(feature = "renderdoc")]
                        if state.capture_next_frame {
                            if let Some(renderdoc) = state.renderdoc.as_mut() {
                                let null = std::ptr::null();

                                renderdoc.end_frame_capture(null, null);

                                let num = renderdoc.get_num_captures();
                                if num > 0 {
                                    if let Some((path, _)) = renderdoc.get_capture(num - 1) {
                                        println!("Opening capture: {:?}", path);
                                        renderdoc.launch_replay_ui(true, path.to_str()).ok();
                                    }
                                }

                                state.capture_next_frame = false;
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Unable to render {}", e);
                    }
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state: key_state,
                        ..
                    },
                ..
            } => {
                state.handle_key(event_loop, code, key_state.is_pressed());
                if let Some(f) = &mut self.on_key {
                    f(state, code, key_state.is_pressed());
                }
            }
            WindowEvent::CursorMoved {
                position: winit::dpi::PhysicalPosition { x, y },
                ..
            } => {
                // state.camera_controller.handle_mouse(x, y);
            }
            WindowEvent::MouseInput {
                device_id,
                state: key_state,
                button,
            } => state.handle_mouse_click(button, key_state.is_pressed()),

            WindowEvent::MouseWheel {
                device_id,
                delta,
                phase,
            } => {
                state.handle_mouse_scroll(delta);
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        device_id: DeviceId,
        event: DeviceEvent,
    ) {
        if let Some(state) = &mut self.state {
            if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
                if state.camera_controller.is_right_click_pressed {
                    state.camera_controller.handle_mouse(dx as f32, dy as f32);
                    state
                        .window
                        .set_cursor_grab(winit::window::CursorGrabMode::Locked)
                        .ok();
                    state.window.set_cursor_visible(false);
                } else {
                    state.window.set_cursor_visible(true);
                    state
                        .window
                        .set_cursor_grab(winit::window::CursorGrabMode::None)
                        .ok();
                }
            }
        }
    }
}

pub fn run() -> anyhow::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        logging::init();
    }
    #[cfg(target_arch = "wasm32")]
    {
        console_log::init_with_level(log::Level::Info).unwrap_throw();
    }

    let event_loop = EventLoop::with_user_event().build()?;
    let mut app = App::new(
        #[cfg(target_arch = "wasm32")]
        &event_loop,
    );
    event_loop.run_app(&mut app)?;

    Ok(())
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn run_web() -> Result<(), wasm_bindgen::JsValue> {
    console_error_panic_hook::set_once();
    run().unwrap_throw();

    Ok(())
}
