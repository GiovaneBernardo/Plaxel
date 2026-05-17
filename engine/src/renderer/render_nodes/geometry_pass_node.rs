use std::any::Any;

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
        //    input_textures: &[],
        //    output_textures: &[],
        //    input_buffers: &[],
        //    output_buffers: &[],
        //}

        RenderNodeDescriptor {
            name: "geometry_pass",
            input_textures: &[],
            output_textures: &[
                // Later when the renderer have post processes, uncomment the create path and rename all other nodes to use the main_color
                //OutputTexture::Create(TextureSlot {
                //    name: "main_color",
                //    texture_descriptor: TextureDescriptor {
                //        label: "main_color",
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
                        label: "main_depth",
                        size: TextureSize::FullRes,
                        format: TextureFormat::Depth32Float,
                        dimension: TextureDimension::D2,
                        usage: TextureUsages::RENDER_ATTACHMENT,
                        mip_levels: 1,
                        sample_count: 1,
                    },
                }),
            ],
            input_buffers: &[],
            output_buffers: &[],
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
            label: "camera_uniform",
            size: size_of::<camera::CameraUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let layout = ctx
            .api
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: "camera_layout".to_string(),
                entries: vec![BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::Both,
                    entry_type: BindingType::UniformBuffer,
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

    fn run(&mut self, ctx: &mut dyn RenderContext) {
        ctx.bind_bind_group(0, self.camera_bind_group.unwrap());
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

    pub fn get_world_position(
        &mut self,
        ctx: &mut dyn RenderContext,
        resources: &mut RenderResources,
        x: f32,
        y: f32,
    ) {
        //let Some(texture) = resources.get_labeled::<TextureSlot>("color") else {
        //    return;
        //};
        //ctx.resolved_inputs;
        //ctx.api().read_texture(ctx.retexture.name, 0.0, 0.0);
    }
}
