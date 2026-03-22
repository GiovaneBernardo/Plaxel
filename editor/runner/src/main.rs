use std::cell::RefCell;
use std::rc::Rc;

use egui_wgpu::wgpu;

#[hot_lib_reloader::hot_module(
    dylib = "game_logic",
    lib_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/debug")
)]
mod game {
    hot_functions_from_file!("game/logic/src/lib.rs");
}

#[hot_lib_reloader::hot_module(
    dylib = "editor_logic",
    lib_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/debug")
)]
mod editor {
    hot_functions_from_file!("editor/logic/src/hierarchy.rs");
    hot_functions_from_file!("editor/logic/src/lib.rs");
}

fn main() {
    run_editor().unwrap();
}

pub fn run_editor() -> anyhow::Result<()> {
    env_logger::init();

    let editor_state: Rc<RefCell<Option<EditorState>>> = Rc::new(RefCell::new(None));
    let editor_for_update = Rc::clone(&editor_state);
    let editor_for_render = Rc::clone(&editor_state);

    let event_loop = winit::event_loop::EventLoop::with_user_event().build()?;
    let mut app = engine::App::new()
        .with_update(move |state| {
            game::update(state);
            let mut opt = editor_for_update.borrow_mut();
            let es = opt.get_or_insert_with(|| EditorState::new(state));
            es.process(state);
        })
        .with_render(move |device, queue, view, encoder| {
            let mut guard = editor_for_render.borrow_mut();
            if let Some(es) = guard.as_mut() {
                let extra_cmds = es.prepare(device, queue, encoder);

                {
                    let mut rp = encoder
                        .begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("egui"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Load,
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            occlusion_query_set: None,
                            timestamp_writes: None,
                        })
                        .forget_lifetime();
                    es.paint(&mut rp);
                }

                queue.submit(extra_cmds);
                es.free_textures();
            }
        })
        .with_on_key(|code, pressed| {
            if code == engine::KeyCode::KeyY && pressed {
                std::process::Command::new("cargo")
                    .args(["build", "-p", "game-logic"])
                    .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
                    .spawn()
                    .ok();

                std::process::Command::new("cargo")
                    .args(["build", "-p", "editor-logic"])
                    .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
                    .spawn()
                    .ok();
            }
        });

    event_loop.run_app(&mut app)?;

    Ok(())
}

pub struct EditorState {
    egui_ctx: egui::Context,
    egui_winit: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    pub clipped_primitives: Vec<egui::ClippedPrimitive>,
    pub textures_delta: egui::TexturesDelta,
    pub screen_descriptor: egui_wgpu::ScreenDescriptor,
}

impl EditorState {
    pub fn new(state: &engine::State) -> Self {
        let egui_ctx = egui::Context::default();
        let egui_winit = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &*state.window,
            Some(state.window.scale_factor() as f32),
            None,
            None,
        );
        let egui_renderer = egui_wgpu::Renderer::new(
            &state.device,
            state.config.format,
            egui_wgpu::RendererOptions {
                depth_stencil_format: None,
                msaa_samples: 1,
                dithering: false,
                predictable_texture_filtering: true,
            },
        );
        let size = state.window.inner_size();
        Self {
            egui_ctx,
            egui_winit,
            egui_renderer,
            clipped_primitives: Vec::new(),
            textures_delta: egui::TexturesDelta::default(),
            screen_descriptor: egui_wgpu::ScreenDescriptor {
                size_in_pixels: [size.width, size.height],
                pixels_per_point: state.window.scale_factor() as f32,
            },
        }
    }

    /// Called each frame during the update phase. Processes input and builds the UI.
    pub fn process(&mut self, state: &mut engine::State) {
        for event in &state.events {
            let _ = self.egui_winit.on_window_event(&state.window, event);
        }

        let raw_input = self.egui_winit.take_egui_input(&state.window);

        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            egui::Window::new("Editor")
                .resizable([true, true])
                .show(ctx, |ui| {
                    ui.label("Hello from egui!");
                    if ui.button("Click me").clicked() {}
                });

            editor::hierarchy_draw(state, ctx);
        });

        self.egui_winit
            .handle_platform_output(&state.window, full_output.platform_output);

        let size = state.window.inner_size();
        self.screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size.width, size.height],
            pixels_per_point: state.window.scale_factor() as f32,
        };

        self.clipped_primitives = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);
        self.textures_delta = full_output.textures_delta;
    }

    /// Upload textures and update vertex/index buffers. Call before the render pass.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Vec<wgpu::CommandBuffer> {
        for (id, delta) in &self.textures_delta.set {
            self.egui_renderer.update_texture(device, queue, *id, delta);
        }
        self.egui_renderer.update_buffers(
            device,
            queue,
            encoder,
            &self.clipped_primitives,
            &self.screen_descriptor,
        )
    }

    /// Draw egui into an already-open render pass.
    /// The pass must be `RenderPass<'static>` via `.forget_lifetime()`.
    pub fn paint(&self, rp: &mut wgpu::RenderPass<'static>) {
        self.egui_renderer
            .render(rp, &self.clipped_primitives, &self.screen_descriptor);
    }

    /// Free GPU textures that egui no longer needs.
    pub fn free_textures(&mut self) {
        for id in &self.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
    }
}
