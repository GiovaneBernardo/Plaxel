use std::any::Any;

use crate::math::{Mat4, vec3};

use crate::renderer::core::*;

pub struct GeometryPassNode {
    pub camera_bind_group: Option<BindGroupHandle>,
    pub camera_bind_group_layout: Option<BindGroupLayoutHandle>,
    pub pass_inputs_group: Option<BindGroupHandle>,
}

impl GeometryPassNode {
    pub fn pass_descriptor() -> RenderNodeDescriptor {
        const MAIN_COLOR_USAGE: TextureUsages = TextureUsages::RENDER_ATTACHMENT
            .union(TextureUsages::COPY_SRC)
            .union(TextureUsages::TEXTURE_BINDING);
        const MAIN_DEPTH_USAGE: TextureUsages = TextureUsages::RENDER_ATTACHMENT
            .union(TextureUsages::COPY_SRC)
            .union(TextureUsages::TEXTURE_BINDING);

        RenderNodeDescriptor {
            name: "geometry_pass",
            color_attachments: vec![ColorAttachmentDescriptor {
                name: "main_color",
                load_op: AttachmentLoadOp::ClearColor([0.0, 0.0, 0.0, 1.0]),
                store: true,
            }],
            depth_attachment: Some(DepthAttachmentDescriptor {
                name: "main_depth",
                // Reverse-Z: clear to 0.0 (the "far" value); depth_compare = Greater.
                load_op: AttachmentLoadOp::ClearDepth(0.0),
                store: true,
            }),
            input_textures: Vec::new(),
            output_textures: vec![
                OutputTexture::Create(TextureSlot {
                    name: "main_color",
                    texture_descriptor: TextureDescriptor {
                        label: "main_color".to_string(),
                        size: TextureSize::FullRes,
                        format: TextureFormat::Bgra8UnormSrgb,
                        dimension: TextureDimension::D2,
                        usage: MAIN_COLOR_USAGE,
                        mip_levels: 1,
                        sample_count: 1,
                    },
                }),
                OutputTexture::Create(TextureSlot {
                    name: "main_depth",
                    texture_descriptor: TextureDescriptor {
                        label: "main_depth".to_string(),
                        size: TextureSize::FullRes,
                        format: TextureFormat::Depth32Float,
                        dimension: TextureDimension::D2,
                        usage: MAIN_DEPTH_USAGE,
                        mip_levels: 1,
                        sample_count: 1,
                    },
                }),
            ],
            input_buffers: Vec::new(),
            output_buffers: Vec::new(),
        }
    }
}

impl RenderNode for GeometryPassNode {
    fn should_render_to_swapchain(&self) -> bool {
        false
    }

    fn describe_pass(&self) -> RenderNodeDescriptor {
        Self::pass_descriptor()
    }

    fn compile(&mut self, ctx: &mut NodeCompileContext) {
        let descriptor = self.describe_pass();

        for output_texture in descriptor.output_textures {
            match output_texture {
                OutputTexture::Create(texture_slot) => {
                    ctx.render_resources
                        .insert_labeled(texture_slot.name, texture_slot);
                }
                _ => {}
            }
        }

        let frame = ctx
            .render_resources
            .get_labeled::<FrameBindings>("frame_bindings")
            .expect("frame bindings must exist before compiling geometry");
        self.camera_bind_group = Some(frame.camera_bind_group);
        self.camera_bind_group_layout = Some(frame.camera_layout);
    }

    fn prepare(&mut self, _resources: &mut RenderResources, _api: &mut dyn RendererAPI) {}

    fn run(&mut self, ctx: &mut dyn RenderContext, render_resources: &RenderResources) {
        ctx.bind_bind_group(0, self.camera_bind_group.unwrap());
        if let Some(frame_bindings) =
            render_resources.get_labeled::<FrameBindings>("frame_bindings")
        {
            ctx.bind_bind_group(1, frame_bindings.materials_bind_group);
        }
        if let Some(shadow) = render_resources.get_labeled::<ShadowBindings>("shadow_bindings") {
            ctx.bind_bind_group(3, shadow.sampling_bind_group);
        }
        // Matching retained and custom producers record immediately after this setup.
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl GeometryPassNode {
    pub fn get_world_position_from_depth(
        &mut self,
        api: &mut dyn RendererAPI,
        graph_resources: &mut GraphResources,
        render_resources: &RenderResources,
        x: f32,
        y: f32,
    ) -> crate::math::Vec3 {
        let Some(texture) = graph_resources.texture("main_depth") else {
            return vec3(0.0, 0.0, 0.0);
        };

        let texture_size = api.get_texture_size(texture);
        let texture_width = texture_size.x as f32;
        let texture_height = texture_size.y as f32;
        let x = x.clamp(0.0, (texture_width - 1.0).max(0.0));
        let y = y.clamp(0.0, (texture_height - 1.0).max(0.0));

        let Some(camera_data) = render_resources.get::<CameraData>() else {
            return vec3(0.0, 0.0, 0.0);
        };

        let depth = api.read_texture::<f32>(texture, x, y);
        engine_info!("Depth: {}", depth);
        let view_proj = Mat4::from_cols_array_2d(&camera_data.uniform.view_proj);
        let inv_view_proj = view_proj.inverse();

        let ndc_x = (x / texture_width) * 2.0 - 1.0;
        let ndc_y = 1.0 - (y / texture_height) * 2.0;

        let clip = crate::math::Vec4::new(ndc_x, ndc_y, depth, 1.0);

        let world = inv_view_proj * clip;
        let world_pos = world.truncate() / world.w;
        let world_pos = crate::math::Vec3::from(world_pos);
        world_pos
    }
}
use crate::engine_info;
