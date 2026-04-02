use crate::Arc;
use crate::Window;
use crate::assets;
use crate::engine_info;

use super::{
    BufferHandle, PipelineHandle, RenderGraph, RenderNode, RenderPassHandle, RendererAPI,
    TextureHandle,
};
use std::collections::HashMap;

pub use crate::renderer::backends::*;
use wgpu;

pub struct WgpuBackend {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: HashMap<PipelineHandle, wgpu::RenderPipeline>,
    buffers: HashMap<BufferHandle, wgpu::Buffer>,
    textures: HashMap<TextureHandle, wgpu::Texture>,
}

impl RendererAPI for WgpuBackend {
    fn compile_pipeline(&mut self, node: &dyn RenderNode) -> PipelineHandle {
        PipelineHandle(0)
    }
    fn create_buffer(&mut self, size: u64, usage: wgpu::BufferUsages) -> BufferHandle {
        BufferHandle(0)
    }
    fn submit(&mut self, graph: &RenderGraph) {}

    fn compile_render_graph_node(&mut self, node: &mut dyn RenderNode, device: &wgpu::Device) {
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            }],
        };
    }

    fn render(
        &mut self,
        surface: &wgpu::Surface,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        render_graph: &RenderGraph,
    ) -> anyhow::Result<()> {
        let output = surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        for node in &render_graph.nodes {
            self.render_node(node.as_ref(), &mut encoder, &view);
        }

        queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }

    fn render_node(
        &mut self,
        node: &dyn RenderNode,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.1,
                        g: 0.2,
                        b: 0.3,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });

        node.run(self);
    }

    fn load_material(&mut self, header: &crate::assets::manager::AssetHeader) {
        engine_info!("Loading material: {:?}", header);
    }
}

impl WgpuBackend {
    pub async fn new(window: Arc<Window>) -> anyhow::Result<Self> {
        let size = window.inner_size();

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
                required_limits: wgpu::Limits::default(),
                memory_hints: Default::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;

        let surface_caps = surface.get_capabilities(&adapter);
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

        Ok(Self {
            window,
            device,
            queue,
            pipelines: HashMap::new(),
            buffers: HashMap::new(),
            textures: HashMap::new(),
        })
    }

    fn get_pipeline(&self, handle: PipelineHandle) -> Option<&wgpu::RenderPipeline> {
        self.pipelines.get(&handle)
    }

    fn add_pipeline(&mut self, pipeline: wgpu::RenderPipeline) -> PipelineHandle {
        let handle = PipelineHandle(self.pipelines.len() as u32);
        self.pipelines.insert(handle, pipeline);
        handle
    }

    fn get_buffer(&self, handle: BufferHandle) -> Option<&wgpu::Buffer> {
        self.buffers.get(&handle)
    }

    fn add_buffer(&mut self, buffer: wgpu::Buffer) -> BufferHandle {
        let handle = BufferHandle(self.buffers.len() as u32);
        self.buffers.insert(handle, buffer);
        handle
    }

    fn get_texture(&self, handle: TextureHandle) -> Option<&wgpu::Texture> {
        self.textures.get(&handle)
    }

    fn add_texture(&mut self, texture: wgpu::Texture) -> TextureHandle {
        let handle = TextureHandle(self.textures.len() as u32);
        self.textures.insert(handle, texture);
        handle
    }
}
