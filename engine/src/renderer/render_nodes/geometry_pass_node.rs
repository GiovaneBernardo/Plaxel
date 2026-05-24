use std::any::Any;

use cgmath::{EuclideanSpace, SquareMatrix, point3};

use crate::renderer::core::*;
use crate::{camera, texture};

pub struct GeometryPassNode {
    pub render_data: Vec<RenderData>,
    pub camera_buffer: Option<BufferHandle>,
    pub camera_bind_group: Option<BindGroupHandle>,
    pub camera_bind_group_layout: Option<BindGroupLayoutHandle>,
    pub pass_inputs_group: Option<BindGroupHandle>,
}

impl RenderNode for GeometryPassNode {
    fn should_render_to_swapchain(&self) -> bool {
        true
    }

    fn describe_pass(&self) -> RenderNodeDescriptor {
        //RenderNodeDescriptor {
        //    input_textures: Vec::new(),
        //    output_textures: Vec::new(),
        //    input_buffers: Vec::new(),
        //    output_buffers: Vec::new(),
        //}

        const MAIN_DEPTH_USAGE: TextureUsages =
            TextureUsages::RENDER_ATTACHMENT.union(TextureUsages::COPY_SRC);

        RenderNodeDescriptor {
            name: "geometry_pass",
            input_textures: Vec::new(),
            output_textures: vec![
                // Later when the renderer have post processes, uncomment the create path and rename all other nodes to use the main_color
                //OutputTexture::Create(TextureSlot {
                //    name: "main_color",
                //    texture_descriptor: TextureDescriptor {
                //        label: "main_color".to_string(),
                //        size: TextureSize::FullRes,
                //        format: TextureFormat::Bgra8UnormSrgb,
                //        dimension: TextureDimension::D2,
                //        usage: TextureUsages::RENDER_ATTACHMENT,
                //        mip_levels: 1,
                //        sample_count: 1,
                //    },
                //}),
                OutputTexture::WriteTo("swapchain_image"),
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
    }

    fn prepare(&mut self, resources: &mut RenderResources, api: &mut dyn RendererAPI) {
        if let (Some(buffer), Some(camera_data)) =
            (self.camera_buffer, resources.get::<CameraData>())
        {
            api.write_buffer(buffer, bytemuck::cast_slice(&[camera_data.uniform]));
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
        for render_data in &mut self.render_data {
            let pipeline = ctx
                .get_pipeline(render_data.material.pipeline_descriptor.uuid)
                .unwrap();

            ctx.bind_pipeline(pipeline);

            let vertex_buffer = ctx.get_mesh_vertex_buffer(&render_data.mesh);
            ctx.bind_vertex_buffer(0, vertex_buffer);

            let index_buffer = ctx.get_mesh_index_buffer(&render_data.mesh);
            ctx.bind_index_buffer(index_buffer);

            let instance_buffer = ctx.get_mesh_instance_buffer(&render_data.mesh);
            ctx.bind_vertex_buffer(1, instance_buffer);

            let range = ctx.get_mesh_draw_range(&render_data.mesh);
            ctx.draw_indexed(range.first_index, range.index_count, range.base_vertex, 1);
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl GeometryPassNode {
    pub fn add_render_data(&mut self, new_render_data: RenderData) {
        self.render_data.push(new_render_data);
    }

    pub fn clear_render_data(&mut self) {
        self.render_data.clear();
    }

    pub fn get_world_position_from_depth(
        &mut self,
        api: &mut dyn RendererAPI,
        graph_resources: &mut GraphResources,
        render_resources: &RenderResources,
        x: f32,
        y: f32,
    ) -> cgmath::Point3<f32> {
        let Some(texture) = graph_resources.texture("main_depth") else {
            return point3(0.0, 0.0, 0.0);
        };

        let texture_width = api.get_texture_size(texture).x as f32;
        let texture_height = api.get_texture_size(texture).y as f32;

        let Some(camera_data) = render_resources.get::<CameraData>() else {
            return point3(0.0, 0.0, 0.0);
        };

        let depth = api.read_texture::<f32>(texture, x, y);
        println!("Depth: {}", depth);
        let view_proj: cgmath::Matrix4<f32> = camera_data.uniform.view_proj.into();
        let Some(inv_view_proj) = view_proj.invert() else {
            return point3(0.0, 0.0, 0.0);
        };

        let ndc_x = (x / texture_width) * 2.0 - 1.0;
        let ndc_y = 1.0 - (y / texture_height) * 2.0;

        let clip = cgmath::Vector4::new(ndc_x, ndc_y, depth, 1.0);

        let world = inv_view_proj * clip;
        let world_pos = world.truncate() / world.w;
        let world_pos = cgmath::Point3::from_vec(world_pos);
        world_pos
    }
}
