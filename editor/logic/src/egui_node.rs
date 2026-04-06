use std::any::Any;

use egui_wgpu::wgpu;
use engine::renderer::backends::NodeCompileContext;
use engine::renderer::backends::RenderContext;
use engine::renderer::backends::RendererAPI;
use engine::renderer::wgpu_backend::WgpuBackend;
use engine::renderer::{
    OutputTexture, RenderNode, RenderNodeDescriptor, RenderResources,
};

use crate::hierarchy::hierarchy_draw;

pub struct EguiRenderNode {
    egui_ctx: egui::Context,
    egui_winit: Option<egui_winit::State>,
    egui_renderer: Option<egui_wgpu::Renderer>,
    clipped_primitives: Vec<egui::ClippedPrimitive>,
    textures_delta: egui::TexturesDelta,
    screen_descriptor: egui_wgpu::ScreenDescriptor,
}

impl EguiRenderNode {
    pub fn new() -> Self {
        Self {
            egui_ctx: egui::Context::default(),
            egui_winit: None,
            egui_renderer: None,
            clipped_primitives: Vec::new(),
            textures_delta: egui::TexturesDelta::default(),
            screen_descriptor: egui_wgpu::ScreenDescriptor {
                size_in_pixels: [1, 1],
                pixels_per_point: 1.0,
            },
        }
    }

    /// Initialize egui_winit. Call once after window is available.
    pub fn init_winit(&mut self, window: &engine::Window) {
        if self.egui_winit.is_none() {
            self.egui_winit = Some(egui_winit::State::new(
                self.egui_ctx.clone(),
                egui::ViewportId::ROOT,
                window,
                Some(window.scale_factor() as f32),
                None,
                None,
            ));
        }
    }

    /// Process input and build egui UI. Call during the update phase.
    pub fn process(&mut self, state: &mut engine::State) {
        self.init_winit(&state.window);
        let egui_winit = self.egui_winit.as_mut().unwrap();

        for event in &state.events {
            let _ = egui_winit.on_window_event(&state.window, event);
        }

        let raw_input = egui_winit.take_egui_input(&state.window);

        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            egui::Window::new("Editor")
                .resizable([true, true])
                .show(ctx, |ui| {
                    ui.label("Hello from egui!");
                    if ui.button("Click me").clicked() {}
                });

            hierarchy_draw(state, ctx);
        });

        egui_winit.handle_platform_output(&state.window, full_output.platform_output);

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
}

impl RenderNode for EguiRenderNode {
    fn describe(&self) -> RenderNodeDescriptor {
        RenderNodeDescriptor {
            input_textures: &[],
            output_textures: &[OutputTexture::WriteTo("color")],
            input_buffers: &[],
            output_buffers: &[],
        }
    }

    fn compile(&mut self, ctx: &mut NodeCompileContext) {
        let backend = ctx
            .api
            .as_any_mut()
            .downcast_mut::<WgpuBackend>()
            .expect("EguiRenderNode requires WgpuBackend");

        self.egui_renderer = Some(egui_wgpu::Renderer::new(
            backend.device(),
            backend.surface_format(),
            egui_wgpu::RendererOptions {
                depth_stencil_format: None,
                msaa_samples: 1,
                dithering: false,
                predictable_texture_filtering: true,
            },
        ));
    }

    fn prepare(&mut self, _resources: &mut RenderResources, api: &mut dyn RendererAPI) {
        let backend = api
            .as_any_mut()
            .downcast_mut::<WgpuBackend>()
            .expect("EguiRenderNode requires WgpuBackend");

        let renderer = self.egui_renderer.as_mut().unwrap();

        // Upload egui textures
        for (id, delta) in &self.textures_delta.set {
            renderer.update_texture(backend.device(), backend.queue(), *id, delta);
        }

        // Update vertex/index buffers
        let mut encoder =
            backend
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("egui prepare encoder"),
                });

        let extra_cmds = renderer.update_buffers(
            backend.device(),
            backend.queue(),
            &mut encoder,
            &self.clipped_primitives,
            &self.screen_descriptor,
        );

        backend.queue().submit(
            extra_cmds
                .into_iter()
                .chain(std::iter::once(encoder.finish())),
        );

        // Free textures egui no longer needs
        for id in &self.textures_delta.free {
            renderer.free_texture(id);
        }
        self.textures_delta = egui::TexturesDelta::default();
    }

    fn run(&mut self, ctx: &mut dyn RenderContext) {
        let primitives = &self.clipped_primitives;
        let screen = &self.screen_descriptor;
        let renderer = self.egui_renderer.as_ref().unwrap();

        ctx.with_raw_pass(&mut |pass| {
            renderer.render(pass, primitives, screen);
        });
    }

    fn should_render_to_swapchain(&self) -> bool {
        true
    }

    fn needs_depth(&self) -> bool {
        false
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
