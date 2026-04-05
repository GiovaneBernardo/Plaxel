use crate::Arc;
use crate::InstanceRaw;
use crate::Window;
use crate::assets;
use crate::assets::manager::AssetType;
use crate::assets::manager::Handle;
use crate::assets::material::PipelineDescriptor;
use crate::engine_info;
use crate::model::AttributeFormat;
use crate::model::MeshAsset;
use crate::model::VertexAttribute;
use crate::model::VertexLayout;
use crate::texture;
use wgpu::BufferUsages;
use wgpu::IndexFormat;
use wgpu::util::DeviceExt;

use super::{
    BufferHandle, PipelineHandle, RenderGraph, RenderNode, RenderPassHandle, RendererAPI,
    TextureHandle,
};
use std::collections::HashMap;

pub use crate::renderer::backends::*;
use wgpu;

struct BufferPool {
    handle: BufferHandle,
    used: u64,
    capacity: u64,
}

pub struct GpuMesh {
    pub vertex_buffer: BufferHandle,
    pub vertex_byte_offset: u64,
    pub index_buffer: BufferHandle,
    pub index_byte_offset: u64,
    pub index_count: u64,
}

pub struct WgpuBackend {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,
    surface: wgpu::Surface<'static>,
    is_surface_configured: bool,
    depth_texture: texture::Texture,
    pipelines: HashMap<PipelineHandle, wgpu::RenderPipeline>,
    pipelines_by_uuid: HashMap<Uuid, PipelineHandle>,
    buffers: HashMap<BufferHandle, wgpu::Buffer>,
    textures: HashMap<TextureHandle, wgpu::Texture>,
    vertex_pools: HashMap<VertexLayout, BufferPool>,
    index_pool: Option<BufferPool>,
    gpu_meshes: HashMap<Handle<MeshAsset>, GpuMesh>,
}

pub struct WgpuRenderContext<'a> {
    pub backend: &'a mut WgpuBackend,
    pub pass: wgpu::RenderPass<'a>,
}

impl<'a> RenderContext for WgpuRenderContext<'a> {
    fn api(&mut self) -> &mut dyn RendererAPI {
        self.backend
    }

    fn bind_pipeline(&mut self, handle: PipelineHandle) {
        let pipeline = &self.backend.pipelines[&handle];
        self.pass.set_pipeline(pipeline);
    }

    fn draw(&mut self, vertices: u32, instances: u32) {
        self.pass.draw(0..vertices, 0..instances);
    }

    fn draw_indexed(&mut self, indices: u32, instances: u32) {
        self.pass.draw_indexed(0..indices, 0, 0..instances);
    }

    fn bind_vertex_buffer(&mut self, buffer: BufferHandle) {
        self.pass
            .set_vertex_buffer(0, self.backend.get_buffer(buffer).unwrap().slice(..));
    }

    fn bind_index_buffer(&mut self, buffer: BufferHandle) {
        self.pass.set_index_buffer(
            self.backend.get_buffer(buffer).unwrap().slice(..),
            IndexFormat::Uint32,
        );
    }
}

impl RendererAPI for WgpuBackend {
    fn compile(&mut self) {
        self.index_pool = Some(self.create_buffer_pool(1024 * 1024, wgpu::BufferUsages::INDEX));
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
        self.depth_texture = texture::Texture::create_depth_texture(
            &self.device,
            &self.surface_config,
            "depth_texture",
        );
    }

    fn compile_pipeline(&mut self, node: &dyn RenderNode) -> PipelineHandle {
        PipelineHandle(0)
    }
    fn create_buffer(&mut self, size: u64, usage: wgpu::BufferUsages) -> BufferHandle {
        BufferHandle(0)
    }
    fn submit(&mut self, graph: &RenderGraph) {}

    fn compile_render_graph_node(&mut self, node: &mut Box<dyn RenderNode>) {
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Shader"),
                source: wgpu::ShaderSource::Wgsl(
                    pollster::block_on(assets::resources::load_string("shaders/cube.wgsl"))
                        .unwrap()
                        .into(),
                ),
            });

        let pipeline_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
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

        let render_pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Render Pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[vertex_buffer_layout],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Bgra8UnormSrgb,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: texture::Texture::DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: 1,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
                cache: None,
            });

        self.add_render_pipeline(render_pipeline);
    }

    fn render(&mut self, render_graph: &mut RenderGraph) -> anyhow::Result<()> {
        //match state.render(&mut self.on_render) {
        //    Ok(_) => {}
        //    // Reconfigure the surface if it's lost or outdated
        //    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
        //        let size = state.window.inner_size();
        //        state.resize(size.width, size.height);
        //    }
        //    Err(e) => {
        //        log::error!("Unable to render {}", e);
        //    }
        //}

        if !render_graph.compiled {
            render_graph.compile(self);
        }

        let surface = &self.surface;

        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        for (_, node) in &mut render_graph.nodes {
            //node.run(self);
            self.render_node(node.as_mut(), &mut encoder, &view);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }

    // Load assets
    fn load_material(&mut self, header: &crate::assets::manager::AssetHeader) -> Material {
        engine_info!("Loading material: {:?}", header);

        let pipeline_descriptor = PipelineDescriptor::default("res/cube.wgsl".to_string());
        let pipeline_uuid = pipeline_descriptor.uuid;
        Material {
            uuid: Uuid::new_v4(),
            pipeline_descriptor,
            pipeline_uuid,
        }
    }

    fn create_pipeline(&mut self, material: &Material) {
        let handle = PipelineHandle(self.pipelines.len() as u32);

        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Shader"),
                source: wgpu::ShaderSource::Wgsl(
                    pollster::block_on(assets::resources::load_string(
                        &material.pipeline_descriptor.shader,
                    ))
                    .unwrap()
                    .into(),
                ),
            });

        let render_pipeline_layout =
            self.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Render Pipeline Layout"),
                    bind_group_layouts: &[],
                    push_constant_ranges: &[],
                });

        engine_info!("Depth Format: {:?}", texture::Texture::DEPTH_FORMAT);

        let render_pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Render Pipeline"),
                layout: Some(&render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x3,
                        }],
                    }],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Bgra8UnormSrgb,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: Some(wgpu::Face::Back),
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: texture::Texture::DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: 1,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
                cache: None,
            });

        self.pipelines.insert(handle, render_pipeline);
        self.pipelines_by_uuid
            .insert(material.pipeline_descriptor.uuid, handle);
    }

    fn create_render_data(
        &mut self,
        positions: Vec<cgmath::Point3<f32>>,
        material: Material,
    ) -> RenderData {
        let positions_raw: Vec<[f32; 3]> = positions.iter().map(|p| [p.x, p.y, p.z]).collect();

        let indices: Vec<u32> = vec![
            4, 5, 6, 4, 6, 7, // front  (+z)
            1, 0, 3, 1, 3, 2, // back   (-z)
            5, 1, 2, 5, 2, 6, // right  (+x)
            0, 4, 7, 0, 7, 3, // left   (-x)
            3, 7, 6, 3, 6, 2, // top    (+y)
            0, 1, 5, 0, 5, 4, // bottom (-y)
        ];

        let vertex_bytes: Vec<u8> = bytemuck::cast_slice(&positions_raw).to_vec();
        let index_bytes: Vec<u8> = bytemuck::cast_slice(&indices).to_vec();

        let mesh = MeshAsset {
            name: "Cube".to_string(),
            vertices: vertex_bytes,
            indices: bytemuck::cast_slice(&indices).to_vec(),
            vertex_layout: VertexLayout {
                stride: std::mem::size_of::<[f32; 3]>() as u64,
                attributes: Vec::new(),
            },
        };

        RenderData {
            mesh: self.load_mesh_with_data(&mesh, &index_bytes),
            material,
            transform_index: 0,
            sort_key: 0,
        }
    }

    // Get using Uuids
    fn get_pipeline(&mut self, uuid: Uuid) -> Option<PipelineHandle> {
        self.pipelines_by_uuid.get(&uuid).cloned()
    }

    fn get_mesh_vertex_buffer(&mut self, mesh: &Handle<MeshAsset>) -> BufferHandle {
        self.gpu_meshes.get(mesh).unwrap().vertex_buffer
    }
    fn get_mesh_index_buffer(&mut self, mesh: &Handle<MeshAsset>) -> BufferHandle {
        self.gpu_meshes.get(mesh).unwrap().index_buffer
    }
    fn get_mesh_index_count(&mut self, mesh: &Handle<MeshAsset>) -> u32 {
        self.gpu_meshes.get(mesh).unwrap().index_count as u32
    }

    fn set_texture(&mut self, texture: &texture::Texture) {
        self.depth_texture = texture.clone();
    }

    fn load_mesh(&mut self, mesh: &MeshAsset) -> Handle<MeshAsset> {
        let index_bytes: Vec<u8> = bytemuck::cast_slice(&mesh.indices).to_vec();
        self.load_mesh_with_data(mesh, &index_bytes)
    }
}

impl WgpuBackend {
    fn load_mesh_with_data(&mut self, mesh: &MeshAsset, index_bytes: &[u8]) -> Handle<MeshAsset> {
        let handle: Handle<MeshAsset> = Handle {
            uuid: Uuid::new_v4(),
            asset_type: AssetType::Mesh,
            _marker: std::marker::PhantomData,
        };

        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Buffer"),
                contents: &mesh.vertices,
                usage: wgpu::BufferUsages::VERTEX,
            });
        let vertex_handle = self.add_buffer(vertex_buffer);

        let index_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Index Buffer"),
                contents: index_bytes,
                usage: wgpu::BufferUsages::INDEX,
            });
        let index_handle = self.add_buffer(index_buffer);

        let gpu_mesh = GpuMesh {
            vertex_buffer: vertex_handle,
            vertex_byte_offset: 0,
            index_buffer: index_handle,
            index_byte_offset: 0,
            index_count: mesh.indices.len() as u64,
        };
        self.gpu_meshes.insert(handle, gpu_mesh);

        handle
    }
}

impl WgpuBackend {
    pub fn get_vertex_pool(&mut self, vertex_layout: &VertexLayout) -> &BufferPool {
        if self.vertex_pools.contains_key(vertex_layout) {
            self.vertex_pools.get(vertex_layout).unwrap()
        } else {
            let vertex_pool = self.create_buffer_pool(1024 * 1024, BufferUsages::VERTEX);
            self.vertex_pools.insert(vertex_layout.clone(), vertex_pool);
            self.vertex_pools.get(vertex_layout).unwrap()
        }
    }

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

        let depth_texture =
            texture::Texture::create_depth_texture(&device, &config, "depth_texture");

        Ok(Self {
            window,
            device,
            queue,
            surface,
            surface_config: config,
            is_surface_configured: false,
            depth_texture,
            pipelines: HashMap::new(),
            pipelines_by_uuid: HashMap::new(),
            buffers: HashMap::new(),
            textures: HashMap::new(),
            vertex_pools: HashMap::new(),
            index_pool: None,
            gpu_meshes: HashMap::new(),
        })
    }

    fn create_buffer_pool(
        &mut self,
        initial_capacity: u64,
        usage: wgpu::BufferUsages,
    ) -> BufferPool {
        let desc: wgpu::BufferDescriptor = wgpu::BufferDescriptor {
            label: Some("Buffer Pool"),
            size: initial_capacity,
            usage,
            mapped_at_creation: false,
        };
        let wgpu_buffer = self.device.create_buffer(&desc);
        let handle = self.add_buffer(wgpu_buffer);
        BufferPool {
            handle,
            capacity: initial_capacity,
            used: 0,
        }
    }

    fn render_node(
        &mut self,
        node: &mut dyn RenderNode,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        //let pipeline = self.get_render_pipeline(node).unwrap();
        //engine_info!("{:?}", pipeline);

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
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
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth_texture.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),

            occlusion_query_set: None,
            timestamp_writes: None,
        });

        let mut ctx = WgpuRenderContext {
            backend: self,
            pass: render_pass,
        };
        node.run(&mut ctx);

        //for render_data in &node.render_data {
        //    render_pass.set_pipeline(render_data.pipeline);
        //    render_pass.set_vertex_buffer(0, render_data.vertex_buffer.slice(..));
        //    render_pass.set_index_buffer(
        //        render_data.index_buffer.slice(..),
        //        wgpu::IndexFormat::Uint32,
        //    );
        //    render_pass.draw_indexed(0..render_data.num_elements, 0, 0..1);
        //}
    }

    fn get_render_pipeline(
        &self,
        pipeline_handle: PipelineHandle,
    ) -> Option<&wgpu::RenderPipeline> {
        self.pipelines.get(&pipeline_handle)
    }

    fn add_render_pipeline(&mut self, pipeline: wgpu::RenderPipeline) -> PipelineHandle {
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
