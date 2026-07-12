use std::any::Any;

use crate::assets::manager::Handle;
use crate::assets::material::Material;
use crate::camera;
use crate::model::MeshAsset;
use crate::renderer::core::*;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct InstanceRaw {
    pub model: [[f32; 4]; 4],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct InstanceRawMaterial {
    pub model: [[f32; 4]; 4],
    pub color: [f32; 4],
    pub material_index: u32,
}

pub struct DebugCube {
    pub position: crate::math::Vec3,
    pub scale: f32,
    pub color: [f32; 4],
}

pub struct DebugPassNode {
    pub cubes: Vec<DebugCube>,
    pub wire_cubes: Vec<DebugCube>,
    pub sphere_positions: Vec<crate::math::Vec3>,
    pub camera_buffer: Option<BufferHandle>,
    pub camera_bind_group: Option<BindGroupHandle>,
    pub camera_bind_group_layout: Option<BindGroupLayoutHandle>,
    pub pass_inputs_group: Option<BindGroupHandle>,
    pub sphere_mesh: Handle<MeshAsset>,
    pub sphere_material: Material,
    pub cube_mesh: Handle<MeshAsset>,
    pub cube_material: Material,
    pub wire_cube_mesh: Handle<MeshAsset>,
    pub wire_cube_material: Material,
    pub sphere_instance_buffer: Option<BufferHandle>,
    pub cube_instance_buffer: Option<BufferHandle>,
    pub wire_cube_instance_buffer: Option<BufferHandle>,
    pub sphere_instance_capacity: u32,
    pub cube_instance_capacity: u32,
    pub wire_cube_instance_capacity: u32,
    pub sphere_instance_count: u32,
    pub cube_instance_count: u32,
    pub wire_cube_instance_count: u32,
}

impl RenderNode for DebugPassNode {
    fn should_render_to_swapchain(&self) -> bool {
        true
    }

    fn describe_pass(&self) -> RenderNodeDescriptor {
        RenderNodeDescriptor {
            name: "debug",
            color_attachments: vec![ColorAttachmentDescriptor {
                name: "swapchain_image",
                load_op: AttachmentLoadOp::Load,
                store: true,
            }],
            depth_attachment: Some(DepthAttachmentDescriptor {
                name: "main_depth",
                load_op: AttachmentLoadOp::Load,
                store: true,
            }),
            input_textures: Vec::new(),
            output_textures: vec![
                OutputTexture::WriteTo("swapchain_image"),
                OutputTexture::WriteTo("main_depth"),
            ],
            input_buffers: Vec::new(),
            output_buffers: Vec::new(),
        }
    }

    fn compile(&mut self, ctx: &mut NodeCompileContext) {
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
                    visibility: ShaderStages::Vertex,
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

        ctx.api
            .create_pipeline(&self.sphere_material, &[layout], &ctx.target_info);
        ctx.api
            .create_pipeline(&self.cube_material, &[layout], &ctx.target_info);
        ctx.api
            .create_pipeline(&self.wire_cube_material, &[layout], &ctx.target_info);
    }

    fn prepare(&mut self, resources: &mut RenderResources, api: &mut dyn RendererAPI) {
        if let (Some(buffer), Some(camera_data)) =
            (self.camera_buffer, resources.get::<CameraData>())
        {
            api.write_buffer(buffer, bytemuck::cast_slice(&[camera_data.uniform]));
        }

        // Upload sphere instance data
        {
            let instances: Vec<InstanceRaw> = self
                .sphere_positions
                .iter()
                .map(|p| {
                    let m =
                        crate::math::Mat4::from_translation(crate::math::Vec3::new(p.x, p.y, p.z));
                    InstanceRaw {
                        model: m.to_cols_array_2d(),
                        color: [0.3, 0.3, 0.3, 1.0],
                    }
                })
                .collect();
            self.sphere_instance_count = instances.len() as u32;
            if self.sphere_instance_count > self.sphere_instance_capacity {
                let new_cap = self.sphere_instance_count.next_power_of_two().max(64);
                self.sphere_instance_buffer = Some(api.create_buffer(&BufferDescriptor {
                    label: "sphere_instance_buffer".to_string(),
                    size: new_cap as u64 * size_of::<InstanceRaw>() as u64,
                    usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                }));
                self.sphere_instance_capacity = new_cap;
            }
            if !instances.is_empty() {
                api.write_buffer(
                    self.sphere_instance_buffer.unwrap(),
                    bytemuck::cast_slice(&instances),
                );
            }
        }

        // Upload cube instance data
        {
            let instances: Vec<InstanceRaw> = self
                .cubes
                .iter()
                .map(|cube| {
                    let p = &cube.position;
                    let m =
                        crate::math::Mat4::from_translation(crate::math::Vec3::new(p.x, p.y, p.z))
                            * crate::math::Mat4::from_scale(crate::math::Vec3::splat(cube.scale));
                    InstanceRaw {
                        model: m.to_cols_array_2d(),
                        color: cube.color,
                    }
                })
                .collect();
            self.cube_instance_count = instances.len() as u32;
            if self.cube_instance_count > self.cube_instance_capacity {
                let new_cap = self.cube_instance_count.next_power_of_two().max(64);
                self.cube_instance_buffer = Some(api.create_buffer(&BufferDescriptor {
                    label: "cube_instance_buffer".to_string(),
                    size: new_cap as u64 * size_of::<InstanceRaw>() as u64,
                    usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                }));
                self.cube_instance_capacity = new_cap;
            }
            if !instances.is_empty() {
                api.write_buffer(
                    self.cube_instance_buffer.unwrap(),
                    bytemuck::cast_slice(&instances),
                );
            }
        }

        // Upload wire cube instance data
        {
            let instances: Vec<InstanceRaw> = self
                .wire_cubes
                .iter()
                .map(|cube| {
                    let p = &cube.position;
                    let m =
                        crate::math::Mat4::from_translation(crate::math::Vec3::new(p.x, p.y, p.z))
                            * crate::math::Mat4::from_scale(crate::math::Vec3::splat(cube.scale));
                    InstanceRaw {
                        model: m.to_cols_array_2d(),
                        color: cube.color,
                    }
                })
                .collect();
            self.wire_cube_instance_count = instances.len() as u32;
            if self.wire_cube_instance_count > self.wire_cube_instance_capacity {
                let new_cap = self.wire_cube_instance_count.next_power_of_two().max(64);
                self.wire_cube_instance_buffer = Some(api.create_buffer(&BufferDescriptor {
                    label: "wire_cube_instance_buffer".to_string(),
                    size: new_cap as u64 * size_of::<InstanceRaw>() as u64,
                    usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                }));
                self.wire_cube_instance_capacity = new_cap;
            }
            if !instances.is_empty() {
                api.write_buffer(
                    self.wire_cube_instance_buffer.unwrap(),
                    bytemuck::cast_slice(&instances),
                );
            }
        }
    }

    fn run(&mut self, ctx: &mut dyn RenderContext, _render_resources: &RenderResources) {
        ctx.bind_bind_group(0, self.camera_bind_group.unwrap());

        // Draw spheres
        if self.sphere_instance_count > 0 {
            let pipeline = ctx
                .get_pipeline(self.sphere_material.pipeline_descriptor.uuid)
                .unwrap();
            ctx.bind_pipeline(pipeline);

            let vertex_buffer = ctx.get_mesh_vertex_buffer(&self.sphere_mesh);
            ctx.bind_vertex_buffer(0, vertex_buffer);

            let index_buffer = ctx.get_mesh_index_buffer(&self.sphere_mesh);
            ctx.bind_index_buffer(index_buffer);

            ctx.bind_vertex_buffer(1, self.sphere_instance_buffer.unwrap());

            let range = ctx.get_mesh_draw_range(&self.sphere_mesh);
            ctx.draw_indexed(
                range.first_index,
                range.index_count,
                range.base_vertex,
                self.sphere_instance_count,
            );
        }

        // Draw cubes
        if self.cube_instance_count > 0 {
            let pipeline = ctx
                .get_pipeline(self.cube_material.pipeline_descriptor.uuid)
                .unwrap();
            ctx.bind_pipeline(pipeline);

            let vertex_buffer = ctx.get_mesh_vertex_buffer(&self.cube_mesh);
            ctx.bind_vertex_buffer(0, vertex_buffer);

            let index_buffer = ctx.get_mesh_index_buffer(&self.cube_mesh);
            ctx.bind_index_buffer(index_buffer);

            ctx.bind_vertex_buffer(1, self.cube_instance_buffer.unwrap());

            let range = ctx.get_mesh_draw_range(&self.cube_mesh);
            ctx.draw_indexed(
                range.first_index,
                range.index_count,
                range.base_vertex,
                self.cube_instance_count,
            );
        }

        // Draw wire cubes
        if self.wire_cube_instance_count > 0 {
            let pipeline = ctx
                .get_pipeline(self.wire_cube_material.pipeline_descriptor.uuid)
                .unwrap();
            ctx.bind_pipeline(pipeline);

            let vertex_buffer = ctx.get_mesh_vertex_buffer(&self.wire_cube_mesh);
            ctx.bind_vertex_buffer(0, vertex_buffer);

            let index_buffer = ctx.get_mesh_index_buffer(&self.wire_cube_mesh);
            ctx.bind_index_buffer(index_buffer);

            ctx.bind_vertex_buffer(1, self.wire_cube_instance_buffer.unwrap());

            let range = ctx.get_mesh_draw_range(&self.wire_cube_mesh);
            ctx.draw_indexed(
                range.first_index,
                range.index_count,
                range.base_vertex,
                self.wire_cube_instance_count,
            );
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl DebugPassNode {
    pub fn add_cube(&mut self, position: crate::math::Vec3, scale: f32, color: [f32; 4]) {
        self.cubes.push(DebugCube {
            position,
            scale,
            color,
        });
    }

    pub fn add_wire_cube(&mut self, position: crate::math::Vec3, scale: f32, color: [f32; 4]) {
        self.wire_cubes.push(DebugCube {
            position,
            scale,
            color,
        });
    }

    pub fn clear_wire_cubes(&mut self) {
        self.wire_cubes.clear();
    }

    pub fn clear_cubes(&mut self) {
        self.cubes.clear();
    }

    pub fn clear_spheres(&mut self) {
        self.sphere_positions.clear();
    }

    pub fn add_sphere(&mut self, position: crate::math::Vec3) {
        self.sphere_positions.push(position);
    }
}
