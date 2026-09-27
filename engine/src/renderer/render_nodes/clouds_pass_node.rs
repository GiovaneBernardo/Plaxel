use std::any::Any;

use half::f16;
use noise::{NoiseFn, Perlin};
use uuid::Uuid;

use crate::assets::material::Material;
use crate::assets::material::{TextureAsset, TextureMip};
use crate::prelude::*;
use crate::renderer::{AtmosphereSettings, FullscreenPassNode, SunDirection};

pub struct CloudsPassNode {
    fullscreen: FullscreenPassNode,
    uniform_buffer: Option<BufferHandle>,
    bind_group_layout: Option<BindGroupLayoutHandle>,
    bind_group: Option<BindGroupHandle>,
    clouds_texture1: Option<TextureHandle>,
    pub uniform: CloudsUniform,
}

impl CloudsPassNode {
    pub fn new() -> Self {
        let material = Material::new("shaders/volumetric_clouds.wgsl".to_string())
            .with_vertex_layouts(Vec::new())
            .with_depth(None)
            .with_blend(BlendMode::Alpha);

        let radius = 6370.0 * 1000.0;
        // Exaggerated preview layer, scaled with the active planet.
        let uniform = CloudsUniform {
            camera_position: [0.0, 0.0, 0.0, 0.0],
            sun_direction: [0.0, 0.0, 0.0, 0.0],
            planet_center: [0.0, 0.0, 0.0, 0.0],
            params: [radius, radius * 0.002, radius * 0.006, radius * 0.025],
            screen_size: [1920.0, 1080.0],
            steps: 64,
            light_steps: 6,
            inverse_projection: [
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
            ],
            inverse_view: [
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
            ],
        };

        Self {
            fullscreen: FullscreenPassNode::new(material, Vec::new()),
            uniform,
            uniform_buffer: None,
            bind_group_layout: None,
            bind_group: None,
            clouds_texture1: None,
        }
    }

    fn load_clouds_texture1() -> TextureAsset {
        let width = 128;
        let height = 128;
        let pixels = Self::generate_cloud_noise(128);
        return TextureAsset {
            uuid: Uuid::new_v4(),
            name: ("clouds_texture1").to_string(),
            width,
            height,
            layers: 128,
            dimension: TextureDimension::D3,
            format: TextureFormat::R16Float,
            mip_levels: vec![TextureMip {
                width,
                height,
                bytes: bytemuck::cast_slice(&pixels).to_vec(),
            }],
        };
    }

    fn generate_cloud_noise(size: u32) -> Vec<f16> {
        let perlin = Perlin::new(42);

        let mut data = Vec::with_capacity((size * size * size) as usize);

        let scale = 4.0;

        for z in 0..size {
            for y in 0..size {
                for x in 0..size {
                    let px = x as f64 / size as f64 * scale;
                    let py = y as f64 / size as f64 * scale;
                    let pz = z as f64 / size as f64 * scale;

                    let n = perlin.get([px, py, pz]);

                    // Perlin is roughly -1..1.
                    let n = (n * 0.5 + 0.5) as f32;

                    data.push(f16::from_f32(n));
                }
            }
        }

        data
    }

    fn rebuild_bind_group(
        &mut self,
        ctx: &mut NodeCompileContext,
        scene_color: TextureHandle,
        scene_depth: TextureHandle,
        clouds_texture1: TextureHandle,
    ) -> BindGroupHandle {
        let layout = self
            .bind_group_layout
            .expect("CloudsPassNode bind group layout must be created before binding");

        let bind_group = ctx.create_bind_group(&BindGroupDescriptor {
            label: "Clouds_bind_group".to_string(),
            layout,
            entries: vec![
                (
                    0,
                    BindGroupEntry::Buffer(
                        self.uniform_buffer
                            .expect("CloudsPassNode uniform buffer must exist"),
                    ),
                ),
                (1, BindGroupEntry::Texture(scene_depth)),
                (2, BindGroupEntry::Texture(scene_color)),
                (3, BindGroupEntry::Sampler(ctx.api.get_default_sampler())),
                (4, BindGroupEntry::Texture(clouds_texture1)),
            ],
        });

        self.bind_group = Some(bind_group);
        bind_group
    }

    pub fn pass_descriptor() -> RenderNodeDescriptor {
        RenderNodeDescriptor {
            name: "clouds",
            color_attachments: vec![ColorAttachmentDescriptor {
                name: "swapchain_image",
                load_op: AttachmentLoadOp::ClearColor([0.0, 0.0, 0.0, 1.0]),
                store: true,
            }],
            depth_attachment: None,
            input_textures: vec!["atmosphere_image", "main_depth"],
            output_textures: vec![OutputTexture::WriteTo("swapchain_image")],
            input_buffers: Vec::new(),
            output_buffers: Vec::new(),
        }
    }
}

impl RenderNode for CloudsPassNode {
    fn should_render_to_swapchain(&self) -> bool {
        true
    }

    fn needs_depth(&self) -> bool {
        false
    }

    fn reflect_mut(&mut self) -> Option<&mut dyn crate::reflect::PartialReflect> {
        Some(&mut self.uniform)
    }

    fn describe_pass(&self) -> RenderNodeDescriptor {
        Self::pass_descriptor()
    }

    fn compile(&mut self, ctx: &mut NodeCompileContext) {
        let uniform_buffer = ctx.create_buffer(&BufferDescriptor {
            label: "clouds_uniform".to_string(),
            size: size_of::<CloudsUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let layout = ctx.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: "clouds_layout".to_string(),
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
                BindGroupLayoutEntry {
                    binding: 4,
                    visibility: ShaderStages::Fragment,
                    entry_type: BindingType::Texture {
                        dimension: TextureDimension::D3,
                        sample_type: TextureSampleType::FloatFilterable,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let clouds_texture1 = self.clouds_texture1.unwrap_or_else(|| {
            let texture = Self::load_clouds_texture1();
            let handle = ctx.api.create_texture_asset(&texture);
            self.clouds_texture1 = Some(handle);
            handle
        });
        let scene_color = ctx.input_texture("atmosphere_image");
        self.uniform_buffer = Some(uniform_buffer);
        self.bind_group_layout = Some(layout);
        let scene_depth = ctx.input_texture("main_depth");
        self.rebuild_bind_group(ctx, scene_color, scene_depth, clouds_texture1);

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
        let sun = resources
            .get::<SunDirection>()
            .copied()
            .unwrap_or_default()
            .0;

        let settings = resources
            .get::<AtmosphereSettings>()
            .copied()
            .unwrap_or_default();
        let radius = settings.planet_radius.max(1.0);
        // Exaggerated preview layer, scaled with the active planet.
        self.uniform.camera_position = [
            camera_data.uniform.position[0],
            camera_data.uniform.position[1],
            camera_data.uniform.position[2],
            0.0,
        ];

        self.uniform.sun_direction = [sun.x, sun.y, sun.z, 0.0];
        self.uniform.planet_center = [
            settings.planet_center[0],
            settings.planet_center[1],
            settings.planet_center[2],
            0.0,
        ];
        self.uniform.screen_size = [surface_size.x as f32, surface_size.y as f32];
        self.uniform.inverse_projection = camera_data.inverse_projection;
        self.uniform.inverse_view = camera_data.inverse_view;
        api.write_buffer(buffer, bytemuck::bytes_of(&self.uniform));
    }

    fn resize(
        &mut self,
        ctx: &mut NodeCompileContext,
        graph_resources: &GraphResources,
        _width: u32,
        _height: u32,
    ) {
        if let (Some(scene_color), Some(scene_depth), Some(clouds_texture1)) = (
            graph_resources.texture("atmosphere_image").copied(),
            graph_resources.texture("main_depth").copied(),
            self.clouds_texture1,
        ) {
            self.rebuild_bind_group(ctx, scene_color, scene_depth, clouds_texture1);
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
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, plaxel_reflect::Reflect)]
pub struct CloudsUniform {
    pub camera_position: [f32; 4],
    pub sun_direction: [f32; 4],
    pub planet_center: [f32; 4],
    pub params: [f32; 4],
    pub screen_size: [f32; 2],
    pub steps: i32,
    pub light_steps: i32,
    pub inverse_projection: [[f32; 4]; 4],
    pub inverse_view: [[f32; 4]; 4],
}

const _: () = assert!(size_of::<CloudsUniform>() == 208);

impl Default for CloudsUniform {
    fn default() -> Self {
        // Earth reference atmosphere in meters. `set_planet` scales the length
        // and inverse-length quantities for generated planets of other sizes.
        let earth_radius = 6_371_000.0;
        Self {
            camera_position: [0.0, 0.0, 0.0, 0.0],
            sun_direction: [0.0, 0.0, 0.0, 0.0],
            planet_center: [0.0, 0.0, 0.0, 0.0],
            params: [0.0, 0.0, 0.0, 0.0],
            screen_size: [0.0, 0.0],
            steps: 1,
            light_steps: 1,
            inverse_projection: [
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
            ],
            inverse_view: [
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
            ],
        }
    }
}
