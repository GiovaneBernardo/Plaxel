use std::any::Any;

use half::f16;
use image::codecs::hdr::HdrDecoder;
use uuid::Uuid;

use crate::assets::material::Material;
use crate::assets::material::{TextureAsset, TextureMip};
use crate::prelude::*;
use crate::renderer::FullscreenPassNode;

pub struct AtmospherePassNode {
    fullscreen: FullscreenPassNode,
    uniform_buffer: Option<BufferHandle>,
    bind_group_layout: Option<BindGroupLayoutHandle>,
    bind_group: Option<BindGroupHandle>,
    skybox_texture: Option<TextureHandle>,
    pub settings: AtmosphereSettings,
}

impl AtmospherePassNode {
    const SKYBOX_PATH: &'static str = "skybox/HDR_multi_nebulae_1.hdr";

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
            skybox_texture: None,
            settings: AtmosphereSettings::default(),
        }
    }

    fn load_skybox_texture() -> TextureAsset {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../res")
                .join(Self::SKYBOX_PATH);
            let load_result = std::fs::File::open(&path)
                .map(std::io::BufReader::new)
                .map_err(image::ImageError::IoError)
                .and_then(HdrDecoder::new)
                .and_then(|decoder| {
                    let metadata = decoder.metadata();
                    let width = metadata.width;
                    let height = metadata.height;
                    let mut pixels =
                        vec![[f16::from_f32(0.0); 4]; width as usize * height as usize];
                    decoder.read_image_transform(
                        |pixel| {
                            let rgb = pixel.to_hdr().0;
                            [
                                f16::from_f32(rgb[0]),
                                f16::from_f32(rgb[1]),
                                f16::from_f32(rgb[2]),
                                f16::from_f32(1.0),
                            ]
                        },
                        pixels.as_mut_slice(),
                    )?;
                    Ok((width, height, pixels))
                });

            match load_result {
                Ok((width, height, pixels)) => {
                    return TextureAsset {
                        uuid: Uuid::new_v4(),
                        name: Self::SKYBOX_PATH.to_string(),
                        width,
                        height,
                        format: TextureFormat::Rgba16Float,
                        mip_levels: vec![TextureMip {
                            width,
                            height,
                            bytes: bytemuck::cast_slice(&pixels).to_vec(),
                        }],
                    };
                }
                Err(error) => {
                    log::warn!("Unable to load skybox {:?}: {error}", path);
                }
            }
        }

        let fallback = [
            f16::from_f32(0.0),
            f16::from_f32(0.0),
            f16::from_f32(0.0),
            f16::from_f32(1.0),
        ];
        TextureAsset {
            uuid: Uuid::new_v4(),
            name: "fallback_skybox".to_string(),
            width: 1,
            height: 1,
            format: TextureFormat::Rgba16Float,
            mip_levels: vec![TextureMip {
                width: 1,
                height: 1,
                bytes: bytemuck::cast_slice(&fallback).to_vec(),
            }],
        }
    }

    fn rebuild_bind_group(
        &mut self,
        ctx: &mut NodeCompileContext,
        scene_color: TextureHandle,
        scene_depth: TextureHandle,
        skybox_texture: TextureHandle,
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
                (4, BindGroupEntry::Texture(skybox_texture)),
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

    fn reflect_mut(&mut self) -> Option<&mut dyn crate::reflect::PartialReflect> {
        Some(&mut self.settings)
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
                BindGroupLayoutEntry {
                    binding: 4,
                    visibility: ShaderStages::Fragment,
                    entry_type: BindingType::Texture {
                        dimension: TextureDimension::D2,
                        sample_type: TextureSampleType::FloatFilterable,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let skybox_texture = self.skybox_texture.unwrap_or_else(|| {
            let texture = Self::load_skybox_texture();
            let handle = ctx.api.create_texture_asset(&texture);
            self.skybox_texture = Some(handle);
            handle
        });
        let scene_color = ctx.input_texture("main_color");
        self.uniform_buffer = Some(uniform_buffer);
        self.bind_group_layout = Some(layout);
        let scene_depth = ctx.input_texture("main_depth");
        self.rebuild_bind_group(ctx, scene_color, scene_depth, skybox_texture);

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

        let planet_radius = self.settings.planet_radius.max(1.0);
        let atmosphere_radius =
            (planet_radius + self.settings.atmosphere_height).max(planet_radius + 1.0);

        let uniform = AtmosphereUniform {
            camera_position: [
                camera_data.uniform.position[0],
                camera_data.uniform.position[1],
                camera_data.uniform.position[2],
                0.0,
            ],
            sun_direction: [
                self.settings.sun_direction[0],
                self.settings.sun_direction[1],
                self.settings.sun_direction[2],
                0.0,
            ],
            planet_center: [
                self.settings.planet_center[0],
                self.settings.planet_center[1],
                self.settings.planet_center[2],
                0.0,
            ],
            params: [
                planet_radius,
                atmosphere_radius,
                self.settings.mie_anisotropy.clamp(-0.999, 0.999),
                self.settings.sun_intensity.max(0.0),
            ],
            screen_size: [surface_size.x as f32, surface_size.y as f32],
            num_in_scattering_points: self.settings.num_in_scattering_points.max(1),
            num_optical_depth_points: self.settings.num_optical_depth_points.max(1),
            rayleigh_scattering: self
                .settings
                .rayleigh_scattering_per_megameter
                .map(|value| value.max(0.0) * 1.0e-6),
            rayleigh_scale_height: self.settings.rayleigh_scale_height.max(1.0),
            mie_scattering: self
                .settings
                .mie_scattering_per_megameter
                .map(|value| value.max(0.0) * 1.0e-6),
            mie_scale_height: self.settings.mie_scale_height.max(1.0),
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
        if let (Some(scene_color), Some(scene_depth), Some(skybox_texture)) = (
            graph_resources.texture("main_color").copied(),
            graph_resources.texture("main_depth").copied(),
            self.skybox_texture,
        ) {
            self.rebuild_bind_group(ctx, scene_color, scene_depth, skybox_texture);
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

#[derive(Clone, Copy, Debug, plaxel_reflect::Reflect)]
pub struct AtmosphereSettings {
    pub sun_direction: [f32; 3],
    pub planet_center: [f32; 3],
    pub planet_radius: f32,
    pub atmosphere_height: f32,
    pub rayleigh_scattering_per_megameter: [f32; 3],
    pub rayleigh_scale_height: f32,
    pub mie_scattering_per_megameter: [f32; 3],
    pub mie_scale_height: f32,
    pub mie_anisotropy: f32,
    pub sun_intensity: f32,
    pub num_in_scattering_points: i32,
    pub num_optical_depth_points: i32,
}

impl AtmosphereSettings {
    /// Updates the active planet while preserving an Earth-like atmosphere when
    /// a generated planet uses a different world-space radius.
    pub fn set_planet(&mut self, center: [f32; 3], radius: f32) {
        let radius = radius.max(1.0);
        let previous_radius = self.planet_radius.max(1.0);

        if radius != previous_radius {
            let length_scale = radius / previous_radius;
            self.atmosphere_height *= length_scale;
            self.rayleigh_scale_height *= length_scale;
            self.mie_scale_height *= length_scale;

            // Coefficients are inverse lengths. Scaling them inversely keeps the
            // optical depth Earth-like when the whole planet is scaled.
            for coefficient in &mut self.rayleigh_scattering_per_megameter {
                *coefficient /= length_scale;
            }
            for coefficient in &mut self.mie_scattering_per_megameter {
                *coefficient /= length_scale;
            }
        }

        self.planet_center = center;
        self.planet_radius = radius;
    }
}

impl Default for AtmosphereSettings {
    fn default() -> Self {
        // Earth reference atmosphere in meters. `set_planet` scales the length
        // and inverse-length quantities for generated planets of other sizes.
        let earth_radius = 6_371_000.0;
        Self {
            sun_direction: [0.3, 0.6, 0.4],
            planet_center: [0.0, 0.0, 0.0],
            planet_radius: earth_radius,
            atmosphere_height: 100_000.0,
            // Inverse megameters keep the small SI coefficients editable in the
            // render-graph inspector, which displays three decimal places.
            rayleigh_scattering_per_megameter: [5.802, 13.558, 33.1],
            rayleigh_scale_height: 8_000.0,
            mie_scattering_per_megameter: [21.0; 3],
            mie_scale_height: 1_200.0,
            mie_anisotropy: 0.76,
            sun_intensity: 20.0,
            num_in_scattering_points: 16,
            num_optical_depth_points: 8,
        }
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
    pub num_in_scattering_points: i32,
    pub num_optical_depth_points: i32,
    pub rayleigh_scattering: [f32; 3],
    pub rayleigh_scale_height: f32,
    pub mie_scattering: [f32; 3],
    pub mie_scale_height: f32,
    pub inverse_projection: [[f32; 4]; 4],
    pub inverse_view: [[f32; 4]; 4],
}

const _: () = assert!(size_of::<AtmosphereUniform>() == 240);
