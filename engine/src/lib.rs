use std::sync::Arc;

pub mod assets;
pub mod core;
pub mod frame_capturer;
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
    keyboard::PhysicalKey,
    window::Window,
};

use crate::assets::manager::AssetManager;
use crate::core::components::core::TransformComponent;
use crate::core::ecs::world::World;
use crate::core::input::InputState;
use crate::core::input::KeyCode;
use crate::core::physics::physics::Physics;
use crate::core::time::Time;
use crate::ecs::scene::Scene;
use crate::frame_capturer::FrameCapturer;
use crate::renderer::GeometryRenderQueue;
use crate::renderer::Renderer;

// This will store the state of our game
pub struct State {
    pub window: Arc<Window>,
    pub active_scene_index: Option<u32>,
    pub scenes: Vec<ecs::scene::Scene>,
    pub events: Vec<WindowEvent>,
    pub renderer: renderer::Renderer,
    pub asset_manager: AssetManager,
    pub frame_capturer: FrameCapturer,
    pub frame_index: u32,
    pub registered_systems: bool,
    pub input: InputState,
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
    pub async fn new(window: Arc<Window>) -> anyhow::Result<Self> {
        let size = window.inner_size();
        let camera = camera::Camera {
            position: (0.0, 65536.0, 2.0).into(),
            orientation: cgmath::Quaternion::from_sv(1.0, cgmath::Vector3::new(0.0, 0.0, 0.0)),
            aspect: size.width as f32 / size.height as f32,
            fovy: 65.0,
            znear: 0.1,
            zfar: 15000000.0,
        };

        let mut camera_uniform = camera::CameraUniform::new();
        camera_uniform.update_view_proj(&camera);

        let mut scenes = Vec::new();
        scenes.insert(0, Self::create_main_game_scene());

        let mut renderer = Renderer::new(window.clone()).await?;
        renderer.init();

        let mut asset_manager = AssetManager::new();

        Ok(Self {
            window,
            events: Vec::new(),
            active_scene_index: Some(0),
            scenes,
            renderer,
            asset_manager,
            frame_capturer: FrameCapturer::new(),
            frame_index: 0,
            registered_systems: false,
            input: InputState::new(),
        })
    }

    pub fn active_scene(&self) -> Option<&ecs::scene::Scene> {
        self.active_scene_index
            .and_then(|i| self.scenes.get(i as usize))
    }

    pub fn active_scene_mut(&mut self) -> Option<&mut ecs::scene::Scene> {
        self.active_scene_index
            .and_then(|i| self.scenes.get_mut(i as usize))
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.renderer.resize(width, height);
            //self.camera.aspect = width as f32 / height as f32;
        }
    }

    fn handle_key(&mut self, event_loop: &ActiveEventLoop, code: KeyCode, is_pressed: bool) {
        if code == KeyCode::Escape && is_pressed {
            event_loop.exit();
        } else {
            //self.camera_controller.handle_key(code, is_pressed);
        }

        if code == KeyCode::KeyH && is_pressed {
            self.frame_capturer.request_capture();
        }

        let world = self.active_scene_mut().unwrap().world_mut();
        let mut input = world.get_resource_mut::<InputState>().unwrap();
        if is_pressed {
            input.pressed.insert(code);
            input.just_pressed.insert(code);
        } else {
            input.pressed.remove(&code);
            input.just_released.insert(code);
        }
    }

    fn handle_mouse_click(&mut self, button: MouseButton, is_pressed: bool) {
        if button == MouseButton::Right {
            //self.camera_controller.handle_mouse_click(is_pressed);
        }
    }

    fn handle_mouse_scroll(&mut self, delta: MouseScrollDelta) {
        //self.camera_controller.handle_mouse_scroll(delta);
    }

    fn update(&mut self) {
        if let Some(scene) = self.active_scene_mut() {
            scene.update();
        }

        let world = self.active_scene_mut().unwrap().world_mut();
        let Some(mut physics) = world.get_resource_mut::<Physics>() else {
            return;
        };
        physics.step();
        //self.camera_controller.update_camera(&mut self.camera);
        //self.camera_uniform.update_view_proj(&self.camera);
        //self.renderer.render_resources.insert(renderer::CameraData {
        //    uniform: self.camera_uniform,
        //});
    }

    fn update_after_render(&mut self) {
        let world = self.active_scene_mut().unwrap().world_mut();
        Self::clear_input_system(world);
    }

    fn clear_input_system(world: &mut World) {
        let Some(mut input) = world.get_resource_mut::<InputState>() else {
            return;
        };

        input.just_pressed.clear();
        input.just_released.clear();
        input.mouse_delta = (0.0, 0.0);
        input.scroll = 0.0;
    }

    fn insert_engine_resources(world: &mut World) {
        world.insert_resource(Time::new());
        world.insert_resource(InputState::new());
        world.insert_resource(Physics::new());
        world.insert_resource(GeometryRenderQueue::new());
    }

    pub fn create_main_game_scene() -> Scene {
        let mut scene = Scene::new();

        Self::insert_engine_resources(scene.world_mut());

        scene
    }
}

pub struct App {
    #[cfg(target_arch = "wasm32")]
    proxy: Option<winit::event_loop::EventLoopProxy<State>>,
    state: Option<State>,
    on_register_system: Option<Box<dyn FnMut(&mut State)>>,
    on_update: Option<Box<dyn FnMut(&mut State)>>,
    on_key: Option<Box<dyn FnMut(&mut State, KeyCode, bool)>>,
    on_resize: Option<Box<dyn FnMut(&mut State, u32, u32)>>,
    on_mouse_button: Option<Box<dyn FnMut(&mut State, MouseButton, bool)>>,
    on_mouse_motion: Option<Box<dyn FnMut(&mut State, f64, f64)>>,
    on_mouse_scroll: Option<Box<dyn FnMut(&mut State, MouseScrollDelta)>>,
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
            on_resize: None,
            on_mouse_button: None,
            on_mouse_motion: None,
            on_mouse_scroll: None,
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

    pub fn with_on_resize(mut self, f: impl FnMut(&mut State, u32, u32) + 'static) -> Self {
        self.on_resize = Some(Box::new(f));
        self
    }

    pub fn with_on_mouse_button(
        mut self,
        f: impl FnMut(&mut State, MouseButton, bool) + 'static,
    ) -> Self {
        self.on_mouse_button = Some(Box::new(f));
        self
    }

    pub fn with_on_mouse_motion(mut self, f: impl FnMut(&mut State, f64, f64) + 'static) -> Self {
        self.on_mouse_motion = Some(Box::new(f));
        self
    }

    pub fn with_on_mouse_scroll(
        mut self,
        f: impl FnMut(&mut State, MouseScrollDelta) + 'static,
    ) -> Self {
        self.on_mouse_scroll = Some(Box::new(f));
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
            WindowEvent::Resized(size) => {
                state.resize(size.width, size.height);
                if let Some(f) = &mut self.on_resize {
                    f(state, size.width, size.height);
                }
            }
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
                let mut state = self.state.as_mut().unwrap();
                match state.renderer.render() {
                    Ok(_) => {
                        state.frame_capturer.finish_capture_after_frame();
                    }
                    Err(e) => {
                        log::error!("Unable to render {}", e);
                    }
                }

                state.update_after_render();
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
            } => {
                state.handle_mouse_click(button, key_state.is_pressed());
                if let Some(f) = &mut self.on_mouse_button {
                    f(state, button, key_state.is_pressed());
                }
            }

            WindowEvent::MouseWheel {
                device_id,
                delta,
                phase,
            } => {
                state.handle_mouse_scroll(delta);
                if let Some(f) = &mut self.on_mouse_scroll {
                    f(state, delta);
                }
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
                if let Some(f) = &mut self.on_mouse_motion {
                    f(state, dx, dy);
                }
                //if state.camera_controller.is_right_click_pressed {
                //    state.camera_controller.handle_mouse(dx as f32, dy as f32);
                //    state
                //        .window
                //        .set_cursor_grab(winit::window::CursorGrabMode::Locked)
                //        .ok();
                //    state.window.set_cursor_visible(false);
                //} else {
                //    state.window.set_cursor_visible(true);
                //    state
                //        .window
                //        .set_cursor_grab(winit::window::CursorGrabMode::None)
                //        .ok();
                //}
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
