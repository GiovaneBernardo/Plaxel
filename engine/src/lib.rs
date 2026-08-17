use std::sync::Arc;

pub extern crate bevy_reflect as plaxel_reflect;

pub mod assets;
pub mod core;
pub mod frame_capturer;
pub mod global_resources;
pub mod logging;
pub mod math;
pub mod multithreading;
pub mod profiling;
pub mod reflect;
pub mod renderer;

use crate::assets::plugin::AssetPlugin;
use crate::core::ecs::plugin::Plugin;
use crate::core::ecs::resource::Resource;
use crate::core::ecs::schedule::CoreSchedule;
use crate::core::ecs::schedule::Schedules;
use crate::core::ecs::system::{SystemContext, SystemParamFunction};
use crate::core::ecs::world::World;
use crate::core::physics::physics::PhysicsPlugin;
use crate::core::time::TimePlugin;
use crate::core::window::WindowPlugin;
use crate::renderer::plugin::RendererPlugin;
pub use core::camera;
pub use core::ecs;
use plaxel_reflect::Reflect;
pub use renderer::model;
pub use renderer::texture;
pub use tracing;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
use winit::application::ApplicationHandler;
use winit::event::DeviceEvent;
use winit::event::DeviceId;
use winit::event::KeyEvent;
use winit::event::MouseScrollDelta;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::event_loop::EventLoop;
use winit::keyboard::PhysicalKey;
use winit::window::Window;

use crate::prelude::*;
pub struct App {
    pub world: World,
    pub schedules: Schedules,
    pub plugins: Vec<Box<dyn Plugin>>,
    pub global_resources: Option<GlobalResources>,
    startup_complete: bool,
    frame_index: u64,
    #[cfg(target_arch = "wasm32")]
    event_proxy: Option<winit::event_loop::EventLoopProxy<AppEvent>>,
}

pub enum AppEvent {
    Exit,
    Redraw,
    #[cfg(target_arch = "wasm32")]
    GlobalResourcesReady(GlobalResources),
}

impl App {
    pub fn new() -> Self {
        Self {
            world: World::new(),
            schedules: Schedules::new(),
            plugins: Vec::new(),
            global_resources: None,
            startup_complete: false,
            frame_index: 0,
            #[cfg(target_arch = "wasm32")]
            event_proxy: None,
        }
    }

    pub fn add_plugin<P>(&mut self, plugin: P) -> &mut Self
    where
        P: Plugin + 'static,
    {
        plugin.build(self);
        self.plugins.push(Box::new(plugin));
        self
    }

    pub fn add_system<Marker, F>(&mut self, schedule: CoreSchedule, system: F) -> &mut Self
    where
        F: SystemParamFunction<Marker>,
        Marker: 'static,
    {
        self.schedules
            .get_mut(schedule)
            .add_named_system(std::any::type_name::<F>(), system);
        self
    }

    /// Adds an exclusive compatibility system that receives the complete ECS
    /// context. New systems should prefer typed system parameters when they do
    /// not need to inspect the entire world.  
    /// DEPRECATED: Use just add_system for new systems
    pub fn add_legacy_system<F>(&mut self, schedule: CoreSchedule, system: F) -> &mut Self
    where
        F: for<'world> FnMut(&mut SystemContext<'world>, &mut crate::ecs::commands::Commands)
            + Send
            + 'static,
    {
        self.schedules
            .get_mut(schedule)
            .add_named_legacy_system(std::any::type_name::<F>(), system);
        self
    }

    /// DEPRECATED: Use just add_system for new systems
    pub fn add_named_legacy_system<F>(
        &mut self,
        schedule: CoreSchedule,
        name: &'static str,
        system: F,
    ) -> &mut Self
    where
        F: for<'world> FnMut(&mut SystemContext<'world>, &mut crate::ecs::commands::Commands)
            + Send
            + 'static,
    {
        self.schedules
            .get_mut(schedule)
            .add_named_legacy_system(name, system);
        self
    }

    /// Registers the event buffer as a world resource and schedules its
    /// once-per-frame maintenance. No event vector is stored on `App`.
    pub fn add_event<E: crate::ecs::event::Event>(&mut self) -> &mut Self {
        if self
            .world
            .contains_resource::<crate::ecs::event::Events<E>>()
        {
            return self;
        }

        self.world.add_event::<E>();
        self.add_system(
            CoreSchedule::Last,
            crate::ecs::event::event_update_system::<E>,
        )
    }

    /// Sends an event into the typed `Events<E>` resource in the main world.
    pub fn send_event<E: crate::ecs::event::Event>(&mut self, event: E) {
        self.world
            .get_resource_mut::<crate::ecs::event::Events<E>>()
            .unwrap_or_else(|| {
                panic!(
                    "event `{}` is not registered; call App::add_event first",
                    std::any::type_name::<E>()
                )
            })
            .send(event);
    }

    /// Attempts to send a platform event when its plugin is installed.
    /// This keeps a deliberately minimal `App` usable without window plugins.
    pub fn try_send_event<E: crate::ecs::event::Event>(&mut self, event: E) -> bool {
        let Some(mut events) = self
            .world
            .get_resource_mut::<crate::ecs::event::Events<E>>()
        else {
            return false;
        };
        events.send(event);
        true
    }

    pub fn run(&mut self) {
        let event_loop: EventLoop<AppEvent> = winit::event_loop::EventLoop::with_user_event()
            .build()
            .unwrap();

        #[cfg(target_arch = "wasm32")]
        {
            self.event_proxy = Some(event_loop.create_proxy());
        }

        self.initialize_schedules();

        event_loop.run_app(self).unwrap();
    }

    pub fn initialize_schedules(&mut self) {
        self.schedules.initialize(&mut self.world);
    }

    /// Executes one schedule immediately against the App's world and globals.
    /// Returns `false` while the renderer/global runtime is not initialized or
    /// when the schedule label does not exist.
    pub fn run_schedule(&mut self, label: CoreSchedule) -> bool {
        let Some(globals) = self.global_resources.as_mut() else {
            return false;
        };
        let mut context = SystemContext {
            world: &mut self.world,
            globals,
            last_run_tick: Default::default(),
            this_run_tick: Default::default(),
        };
        self.schedules.run(label, &mut context)
    }

    /// Runs the non-render schedules for one application frame.
    pub fn update(&mut self) -> bool {
        if self.global_resources.is_none() {
            return false;
        }

        if !self.startup_complete {
            self.startup_complete = self.run_schedule(CoreSchedule::Startup);
        }

        for schedule in [
            CoreSchedule::First,
            CoreSchedule::PreUpdate,
            CoreSchedule::Update,
            CoreSchedule::PostUpdate,
            CoreSchedule::Last,
        ] {
            self.run_schedule(schedule);
        }

        true
    }

    fn render_frame(&mut self) {
        for schedule in [
            CoreSchedule::RenderExtract,
            CoreSchedule::RenderPrepare,
            CoreSchedule::Render,
        ] {
            self.run_schedule(schedule);
        }
    }

    pub fn insert_resource<R: Resource + Reflect>(&mut self, value: R) -> &mut Self {
        self.world.insert_resource(value);
        self
    }

    pub fn insert_opaque_resource<R: Resource>(&mut self, value: R) -> &mut Self {
        self.world.insert_opaque_resource(value);
        self
    }

    pub fn init_resource<R: Resource + Reflect + Default>(&mut self) -> &mut Self {
        self.world.init_resource::<R>();
        self
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

        if let Some(globals) = &self.global_resources {
            globals.window.request_redraw();
            return;
        }

        #[allow(unused_mut)]
        let mut attributes = Window::default_attributes();
        #[cfg(not(target_arch = "wasm32"))]
        {
            attributes = attributes.with_maximized(true);
        }
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            use winit::platform::web::WindowAttributesExtWebSys;

            let browser_window = wgpu::web_sys::window().unwrap_throw();
            let document = browser_window.document().unwrap_throw();
            let canvas = document.get_element_by_id("canvas").unwrap_throw();
            attributes = attributes.with_canvas(Some(canvas.unchecked_into()));
        }

        let window = Arc::new(event_loop.create_window(attributes).unwrap());

        #[cfg(not(target_arch = "wasm32"))]
        {
            let globals = pollster::block_on(GlobalResources::new(window));
            globals.window.request_redraw();
            self.global_resources = Some(globals);
        }

        #[cfg(target_arch = "wasm32")]
        {
            let proxy = self
                .event_proxy
                .clone()
                .expect("the event proxy is initialized before run_app");
            wasm_bindgen_futures::spawn_local(async move {
                let globals = GlobalResources::new(window).await;
                let _ = proxy.send_event(AppEvent::GlobalResourcesReady(globals));
            });
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Exit => event_loop.exit(),
            AppEvent::Redraw => {
                if let Some(globals) = &self.global_resources {
                    globals.window.request_redraw();
                }
            }
            #[cfg(target_arch = "wasm32")]
            AppEvent::GlobalResourcesReady(globals) => {
                globals.window.request_redraw();
                self.global_resources = Some(globals);
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(globals) = &self.global_resources {
            globals.window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self
            .global_resources
            .as_ref()
            .map(|globals| globals.window.clone())
        else {
            return;
        };
        if window.id() != window_id {
            return;
        }

        self.try_send_event(event.clone());

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                crate::profiling::sync_enabled(
                    self.global_resources
                        .as_ref()
                        .is_some_and(|globals| globals.profiling_enabled),
                );
                crate::profiling::begin_frame(self.frame_index);
                if self.update() {
                    self.render_frame();
                }

                crate::profiling::end_frame();
                self.frame_index = self.frame_index.wrapping_add(1);
            }
            WindowEvent::Resized(size) => {
                self.try_send_event(crate::core::window::WindowResized {
                    width: size.width,
                    height: size.height,
                });
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(key_code),
                        state,
                        repeat,
                        ..
                    },
                ..
            } => {
                let pressed = state.is_pressed();
                self.try_send_event(crate::core::window::KeyboardInput {
                    key_code,
                    pressed,
                    repeat,
                });
                if key_code == winit::keyboard::KeyCode::Escape && pressed {
                    event_loop.exit();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.try_send_event(crate::core::window::MouseButtonInput {
                    button,
                    pressed: state.is_pressed(),
                });
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.try_send_event(crate::core::window::CursorMoved {
                    x: position.x as f32,
                    y: position.y as f32,
                });
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (horizontal, vertical) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x, y),
                    MouseScrollDelta::PixelDelta(position) => {
                        (position.x as f32 / 100.0, position.y as f32 / 100.0)
                    }
                };
                self.try_send_event(crate::core::window::MouseWheel {
                    horizontal,
                    vertical,
                });
            }
            WindowEvent::DroppedFile(path) => {
                self.try_send_event(crate::core::window::FileDropped { path });
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        if let DeviceEvent::MouseMotion { delta } = event {
            self.try_send_event(crate::core::window::MouseMotion {
                delta_x: delta.0 as f32,
                delta_y: delta.1 as f32,
            });
        }
    }
}

#[cfg(test)]
mod app_event_tests {
    use super::*;
    use crate::{core::window::WindowResized, ecs::event::Events};

    #[test]
    fn app_events_are_stored_in_the_world() {
        let mut app = App::new();
        app.add_plugin(WindowPlugin);
        app.send_event(WindowResized {
            width: 1280,
            height: 720,
        });

        let events = app.world.get_resource::<Events<WindowResized>>().unwrap();
        let mut reader = events.get_reader();
        assert_eq!(
            reader.read(&events).collect::<Vec<_>>(),
            vec![&WindowResized {
                width: 1280,
                height: 720,
            }]
        );
    }
}

pub struct PlaxelDefaultPlugin;
impl Plugin for PlaxelDefaultPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugin(TimePlugin)
            .insert_resource(crate::core::input::InputState::new())
            .add_plugin(WindowPlugin)
            .add_plugin(PhysicsPlugin)
            .add_plugin(AssetPlugin)
            .add_plugin(RendererPlugin);
    }
}

/*

fn update(&mut self) {
    crate::profile_scope!("engine.update");
    {
        crate::profile_scope!("engine.scene.update");
        if let Some(scene_index) = self.active_scene_index.map(|i| i as usize) {
            if let Some(scene) = self.scenes.get_mut(scene_index) {
                scene.update(&mut self.global_resources);
            }
        }
    }

    let world = self.active_scene_mut().unwrap().world_mut();
    let Some(mut physics) = world.get_resource_mut::<Physics>() else {
        return;
    };
    crate::profile_scope!("physics.step");
    physics.step();
    //self.camera_controller.update_camera(&mut self.camera);
    //self.camera_uniform.update_view_proj(&self.camera);
    //self.renderer.render_resources.insert(renderer::CameraData {
    //    uniform: self.camera_uniform,
    //});
}

fn update_after_render(&mut self) {
    crate::profile_scope!("engine.update_after_render");
    let world = self.active_scene_mut().unwrap().world_mut();
    crate::profile_scope!("engine.input.clear");
    Self::clear_input_system(world);
}

fn sync_render_queues(&mut self) {
    crate::profile_scope!("renderer.sync_render_queues");
    let Some(scene_index) = self.active_scene_index.map(|i| i as usize) else {
        return;
    };
    let Some(scene) = self.scenes.get_mut(scene_index) else {
        return;
    };

    let globals = &mut self.global_resources;
    globals
        .renderer
        .sync_render_database(scene.world_mut(), &globals.asset_manager);
}






        fn handle_key(
    &mut self,
    event_loop: &ActiveEventLoop,
    code: KeyCode,
    is_pressed: bool,
    is_repeat: bool,
) {
    if code == KeyCode::Escape && is_pressed {
        event_loop.exit();
    } else {
        //self.camera_controller.handle_key(code, is_pressed);
    }

    if code == KeyCode::KeyH && is_pressed {
        self.global_resources.frame_capturer.request_capture();
    }

    if code == KeyCode::KeyR && is_pressed {
        self.global_resources.renderer.renderer_api.reload_shaders();
    }

    if code == KeyCode::F10 && is_pressed && !is_repeat {
        self.global_resources
            .renderer
            .renderer_api
            .toggle_present_mode();
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

    let world = self.active_scene_mut().unwrap().world_mut();
    let mut input = world.get_resource_mut::<InputState>().unwrap();
    if is_pressed {
        input.mouse_pressed.insert(button);
        input.mouse_just_pressed.insert(button);
    } else {
        input.mouse_pressed.remove(&button);
        input.mouse_just_released.insert(button);
    }
}

fn handle_mouse_scroll(&mut self, delta: MouseScrollDelta) {
    let scroll = match delta {
        MouseScrollDelta::LineDelta(_, y) => y,
        MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / 100.0,
    };

    let world = self.active_scene_mut().unwrap().world_mut();
    let mut input = world.get_resource_mut::<InputState>().unwrap();
    input.scroll += scroll.clamp(-1.0, 1.0);
}

fn handle_mouse_motion(&mut self, dx: f64, dy: f64) {
    let world = self.active_scene_mut().unwrap().world_mut();
    let mut input = world.get_resource_mut::<InputState>().unwrap();
    input.mouse_delta.0 += dx as f32;
    input.mouse_delta.1 += dy as f32;
}

fn handle_cursor_moved(&mut self, x: f64, y: f64) {
    let world = self.active_scene_mut().unwrap().world_mut();
    let mut input = world.get_resource_mut::<InputState>().unwrap();
    input.mouse_position = Some((x as f32, y as f32));
}
 */

pub mod prelude {
    pub use crate::assets::prelude::*;
    pub use crate::ecs::prelude::*;
    pub use crate::global_resources::*;
    pub use crate::logging::prelude::*;
    pub use crate::math::prelude::*;
    pub use crate::renderer::prelude::*;
}
