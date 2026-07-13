use std::any::Any;

use crate::math::{Mat4, vec3};

use crate::camera;
use crate::model::TransformInstance;
use crate::renderer::core::*;

pub struct GeometryPassNode {
    pub render_data: Vec<RenderData>,
    pub camera_buffer: Option<BufferHandle>,
    pub camera_bind_group: Option<BindGroupHandle>,
    pub camera_bind_group_layout: Option<BindGroupLayoutHandle>,
    pub pass_inputs_group: Option<BindGroupHandle>,
    pub transforms: Vec<TransformInstance>,
    pub transform_buffer: Option<BufferHandle>,
    pub transform_capacity: u32,
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

        let buffer = ctx.create_buffer(&BufferDescriptor {
            label: "camera_uniform".to_string(),
            size: size_of::<camera::CameraUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST | BufferUsages::VERTEX,
        });

        let instance_buffer = ctx.create_buffer(&BufferDescriptor {
            label: "geometry_transform_instances".to_string(),
            size: self.transform_capacity.max(1) as u64 * size_of::<TransformInstance>() as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        });

        let layout = ctx
            .api
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: "camera_layout".to_string(),
                entries: vec![BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::Both,
                    entry_type: BindingType::UniformBuffer,
                    count: None,
                }],
            });

        let bind_group = ctx.api.create_bind_group(&BindGroupDescriptor {
            label: "camera_bind_group".to_string(),
            layout,
            entries: vec![(0, BindGroupEntry::Buffer(buffer))],
        });

        self.camera_buffer = Some(buffer);
        self.camera_bind_group = Some(bind_group);
        self.camera_bind_group_layout = Some(layout);

        self.transform_buffer = Some(instance_buffer);
    }

    fn prepare(&mut self, resources: &mut RenderResources, api: &mut dyn RendererAPI) {
        if let (Some(buffer), Some(camera_data)) =
            (self.camera_buffer, resources.get::<CameraData>())
        {
            api.write_buffer(buffer, bytemuck::cast_slice(&[camera_data.uniform]));
        }

        if self.transforms.is_empty() {
            return;
        }

        if self.transforms.len() as u32 > self.transform_capacity || self.transform_buffer.is_none()
        {
            self.transform_capacity = (self.transforms.len() as u32).next_power_of_two().max(1);
            self.transform_buffer = Some(api.create_buffer(&BufferDescriptor {
                label: "geometry_transform_instances".to_string(),
                size: self.transform_capacity as u64 * size_of::<TransformInstance>() as u64,
                usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            }));
        }

        if let (Some(buffer), transforms) = (self.transform_buffer, &self.transforms) {
            api.write_buffer(buffer, bytemuck::cast_slice(&transforms));
        }
    }

    fn run(&mut self, ctx: &mut dyn RenderContext, render_resources: &RenderResources) {
        ctx.bind_bind_group(0, self.camera_bind_group.unwrap());
        if let Some(frame_bindings) =
            render_resources.get_labeled::<FrameBindings>("frame_bindings")
        {
            ctx.bind_bind_group(1, frame_bindings.materials_bind_group);
        }
        //ctx.bind_bind_group(1, self.pass_inputs_group);
        let mut last_vertex_buffer = BufferHandle(0);
        let mut last_index_buffer = BufferHandle(0);
        for render_data in &mut self.render_data {
            let pipeline = ctx
                .get_pipeline(render_data.material.pipeline_descriptor.uuid)
                .unwrap();

            ctx.bind_pipeline(pipeline);

            for &(group_index, bind_group) in &render_data.extra_bind_groups {
                ctx.bind_bind_group(group_index, bind_group);
            }

            let vertex_buffer = ctx.get_mesh_vertex_buffer(&render_data.mesh);
            if last_vertex_buffer.0 != vertex_buffer.0 {
                ctx.bind_vertex_buffer(0, vertex_buffer);
            }
            last_vertex_buffer = vertex_buffer;

            let index_buffer = ctx.get_mesh_index_buffer(&render_data.mesh);
            if last_index_buffer.0 != index_buffer.0 {
                ctx.bind_index_buffer(index_buffer);
            }
            last_index_buffer = index_buffer;

            let Some(instance_buffer) = self.transform_buffer else {
                continue;
            };
            let transform_index = render_data.transform_index as usize;
            if transform_index >= self.transforms.len() {
                continue;
            }
            let instance_size = size_of::<TransformInstance>() as u64;
            ctx.bind_vertex_buffer_range(
                1,
                instance_buffer,
                transform_index as u64 * instance_size,
                instance_size,
            );

            let range = ctx.get_mesh_draw_range(&render_data.mesh);
            ctx.draw_indexed(range.first_index, range.index_count, range.base_vertex, 1);
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl GeometryPassNode {
    pub fn add_render_data(&mut self, mut new_render_data: RenderData) {
        new_render_data.transform_index = self.transforms.len() as u32;
        self.transforms.push(TransformInstance {
            model_matrix: Mat4::IDENTITY.to_cols_array_2d(),
            material_index: new_render_data.material.material_index,
        });
        self.render_data.push(new_render_data);
    }

    pub fn clear_render_data(&mut self) {
        self.render_data.clear();
        self.transforms.clear();
    }

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
