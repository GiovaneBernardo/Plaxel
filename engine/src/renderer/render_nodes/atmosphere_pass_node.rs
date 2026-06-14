use std::any::Any;

use crate::assets::material::Material;
use crate::renderer::*;

pub struct AtmospherePassNode {
    fullscreen: FullscreenPassNode,
    uniform_buffer: Option<BufferHandle>,
    bind_group_layout: Option<BindGroupLayoutHandle>,
    bind_group: Option<BindGroupHandle>,
}

impl AtmospherePassNode {
    pub fn new() -> Self {
        let material = Material::new("shaders/atmosphere.wgsl".to_string())
            .with_vertex_layouts(Vec::new())
            .with_depth(None)
            .with_blend(BlendMode::Alpha);

        Self {
            fullscreen: FullscreenPassNode::new(material, Vec::new()),
            uniform_buffer: None,
            bind_group_layout: None,
            bind_group: None,
        }
    }

    fn rebuild_bind_group(
        &mut self,
        ctx: &mut NodeCompileContext,
        scene_color: TextureHandle,
        scene_depth: TextureHandle,
    ) -> BindGroupHandle {
        let layout = self
            .bind_group_layout
            .expect("AtmospherePassNode bind group layout must be created before binding");

        let bind_group = ctx.create_bind_group(&BindGroupDescriptor {
            label: "atmosphere_bind_group".to_string(),
            layout,
            entries: vec![
                (
                    0,
                    BindGroupEntry::Buffer(
                        self.uniform_buffer
                            .expect("AtmospherePassNode uniform buffer must exist"),
                    ),
                ),
                (1, BindGroupEntry::Texture(scene_depth)),
                (2, BindGroupEntry::Texture(scene_color)),
                (3, BindGroupEntry::Sampler(ctx.api.get_default_sampler())),
            ],
        });

        self.bind_group = Some(bind_group);
        bind_group
    }

    pub fn pass_descriptor() -> RenderNodeDescriptor {
        RenderNodeDescriptor {
            name: "atmosphere",
            color_attachments: vec![ColorAttachmentDescriptor {
                name: "swapchain_image",
                load_op: AttachmentLoadOp::ClearColor([0.0, 0.0, 0.0, 1.0]),
                store: true,
            }],
            depth_attachment: None,
            input_textures: vec!["main_color", "main_depth"],
            output_textures: vec![OutputTexture::WriteTo("swapchain_image")],
            input_buffers: Vec::new(),
            output_buffers: Vec::new(),
        }
    }
}

impl RenderNode for AtmospherePassNode {
    fn should_render_to_swapchain(&self) -> bool {
        true
    }

    fn needs_depth(&self) -> bool {
        false
    }

    fn describe_pass(&self) -> RenderNodeDescriptor {
        Self::pass_descriptor()
    }

    fn compile(&mut self, ctx: &mut NodeCompileContext) {
        let uniform_buffer = ctx.create_buffer(&BufferDescriptor {
            label: "atmosphere_uniform".to_string(),
            size: size_of::<AtmosphereUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let layout = ctx.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: "atmosphere_layout".to_string(),
            entries: vec![
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::Fragment,
                    entry_type: BindingType::UniformBuffer,
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::Fragment,
                    entry_type: BindingType::Texture {
                        dimension: TextureDimension::D2,
                        sample_type: TextureSampleType::Depth,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::Fragment,
                    entry_type: BindingType::Texture {
                        dimension: TextureDimension::D2,
                        sample_type: TextureSampleType::FloatFilterable,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::Fragment,
                    entry_type: BindingType::Sampler,
                    count: None,
                },
            ],
        });

        let scene_color = ctx.input_texture("main_color");
        self.uniform_buffer = Some(uniform_buffer);
        self.bind_group_layout = Some(layout);
        let scene_depth = ctx.input_texture("main_depth");
        self.rebuild_bind_group(ctx, scene_color, scene_depth);

        self.fullscreen.bind_group_layouts = vec![layout];
        self.fullscreen.compile(ctx);
    }

    fn prepare(&mut self, resources: &mut RenderResources, api: &mut dyn RendererAPI) {
        let Some(buffer) = self.uniform_buffer else {
            return;
        };
        let Some(camera_data) = resources.get::<CameraData>() else {
            return;
        };
        let surface_size = api.get_surface_size();

        let terrain_radius = 65536.0 / 8.0;
        let planet_radius = terrain_radius * 1.05;
        let atmosphere_radius = planet_radius + 2000.0;

        let uniform = AtmosphereUniform {
            camera_position: [
                camera_data.uniform.position[0],
                camera_data.uniform.position[1],
                camera_data.uniform.position[2],
                0.0,
            ],
            sun_direction: [0.3, 0.6, 0.4, 0.0],
            planet_center: [0.0, 0.0, 0.0, 0.0],
            params: [planet_radius, atmosphere_radius, 0.0, 0.0],
            screen_size: [surface_size.x as f32, surface_size.y as f32],
            _padding: [0.0, 0.0],
            inverse_projection: camera_data.inverse_projection,
            inverse_view: camera_data.inverse_view,
        };

        api.write_buffer(buffer, bytemuck::cast_slice(&[uniform]));
    }

    fn resize(
        &mut self,
        ctx: &mut NodeCompileContext,
        graph_resources: &GraphResources,
        _width: u32,
        _height: u32,
    ) {
        if let (Some(scene_color), Some(scene_depth)) = (
            graph_resources.texture("main_color").copied(),
            graph_resources.texture("main_depth").copied(),
        ) {
            self.rebuild_bind_group(ctx, scene_color, scene_depth);
        }
    }

    fn run(&mut self, ctx: &mut dyn RenderContext, _render_resources: &RenderResources) {
        let Some(bind_group) = self.bind_group else {
            return;
        };
        self.fullscreen.run(ctx, &[bind_group]);
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct AtmosphereUniform {
    pub camera_position: [f32; 4],
    pub sun_direction: [f32; 4],
    pub planet_center: [f32; 4],
    pub params: [f32; 4],
    pub screen_size: [f32; 2],
    pub _padding: [f32; 2],
    pub inverse_projection: [[f32; 4]; 4],
    pub inverse_view: [[f32; 4]; 4],
}
