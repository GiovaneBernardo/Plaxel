use std::fs;

use wgpu::RenderPipeline;

use crate::Arc;
use crate::InstanceRaw;
use crate::Window;
use crate::assets::manager::Handle;
pub use crate::core::camera;
use crate::model;
pub use crate::renderer::backends::*;
use crate::renderer::model::Vertex;
use crate::{State, texture};
use wgpu;
use wgpu::util::DeviceExt;

#[derive(Copy, Clone, Eq, Hash, PartialEq)]
pub struct PipelineHandle(pub u32);
#[derive(Copy, Clone, Eq, Hash, PartialEq)]
pub struct BufferHandle(pub u32);
#[derive(Copy, Clone, Eq, Hash, PartialEq)]
pub struct TextureHandle(pub u32);
#[derive(Copy, Clone, Eq, Hash, PartialEq)]
pub struct RenderPassHandle(pub u32);

pub struct Renderer {
    pub renderer_api: Box<dyn RendererAPI>,
    pub render_graph: RenderGraph,
    pub pipelines: Vec<wgpu::RenderPipeline>,
    pub textures: Vec<texture::Texture>,
}

pub struct Texture {
    pub name: String,
    pub wgpu_texture: wgpu::Texture,
}

pub struct Buffer {
    pub name: String,
    pub wgpu_buffer: wgpu::Buffer,
}

pub struct RenderGraph {
    pub nodes: Vec<Box<dyn RenderNode>>,
    pub compiled: bool,
}

pub struct PassResources {
    pub input_textures: Vec<Texture>,
    pub output_textures: Vec<Texture>,
    pub input_buffers: Vec<Buffer>,
    pub output_buffers: Vec<Buffer>,
}

pub trait RenderNode {
    fn input_textures(&self) -> &[&str];
    fn output_textures(&self) -> &[&str];
    fn input_buffers(&self) -> &[&str];
    fn output_buffers(&self) -> &[&str];
    fn run(&self, api: &mut dyn RendererAPI);
    fn get_render_pass(&self) -> RenderPassHandle;
    fn set_render_pass(&self, render_pass_handle: RenderPassHandle);
    fn should_render_to_swapchain(&self) -> bool;
}

pub struct RenderData {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub num_elements: u32,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> anyhow::Result<Self> {
        Ok(Self {
            renderer_api: Box::new(WgpuBackend::new(window).await?),
            render_graph: RenderGraph {
                nodes: Vec::new(),
                compiled: false,
            },
            pipelines: Vec::new(),
            textures: Vec::new(),
        })
    }

    pub fn init(&mut self, device: &wgpu::Device) {
        self.render_graph = RenderGraph::default_render_graph();
        self.render_graph
            .compile(&device, self.renderer_api.as_mut());
    }

    pub fn render(
        &mut self,
        surface: &wgpu::Surface,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> anyhow::Result<()> {
        self.renderer_api
            .render(surface, device, queue, &self.render_graph)
    }

    pub fn compile(
        &self,
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
    ) -> wgpu::RenderPipeline {
        let width = 1920;
        let height = 1080;
        let camera = camera::Camera {
            position: (0.0, 1.0, 2.0).into(),
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
            aspect: width as f32 / height as f32,
            fovy: 65.0,
            znear: 0.1,
            zfar: 15000.0,
        };

        let mut camera_uniform = camera::CameraUniform::new();
        camera_uniform.update_view_proj(&camera);

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: Some("camera_bind_group_layout"),
            });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
            label: Some("camera_bind_group"),
        });

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        // This should match the filterable field of the
                        // corresponding Texture entry above.
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
                label: Some("texture_bind_group_layout"),
            });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[&texture_bind_group_layout, &camera_bind_group_layout],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                buffers: &[model::ModelVertex::desc(), InstanceRaw::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
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
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });
        render_pipeline
    }

    pub fn create_render_data(
        &mut self,
        device: &wgpu::Device,
        positions: Vec<cgmath::Point3<f32>>,
    ) -> RenderData {
        let positions_raw: Vec<[f32; 3]> = positions.iter().map(|p| [p.x, p.y, p.z]).collect();

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(&positions_raw),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let indices: Vec<u32> = vec![
            4, 5, 6, 4, 6, 7, // front  (+z)
            1, 0, 3, 1, 3, 2, // back   (-z)
            5, 1, 2, 5, 2, 6, // right  (+x)
            0, 4, 7, 0, 7, 3, // left   (-x)
            3, 7, 6, 3, 6, 2, // top    (+y)
            0, 1, 5, 0, 5, 4, // bottom (-y)
        ];

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        RenderData {
            vertex_buffer,
            index_buffer,
            num_elements: indices.len() as u32,
        }
    }
}

impl RenderGraph {
    pub fn default_render_graph() -> Self {
        let mut graph = RenderGraph {
            nodes: Vec::new(),
            compiled: false,
        };

        let geometry_pass_node = RenderNode {
            shader_name: "shaders/cube.wgsl".to_string(),
            render_pipeline: PipelineHandle(0),
            buffers: Vec::new(),
            render_data: Vec::new(),
        };
        graph.nodes.push(geometry_pass_node);

        graph
    }

    pub fn compile(&mut self, device: &wgpu::Device, renderer_api: &mut dyn RendererAPI) {
        for node in &mut self.nodes {
            renderer_api.compile_render_graph_node(node, device);
        }
        self.compiled = true;
    }
}

impl RenderNode {
    pub fn add_render_data(&mut self, render_data: RenderData) {
        self.render_data.push(render_data);
    }
}
