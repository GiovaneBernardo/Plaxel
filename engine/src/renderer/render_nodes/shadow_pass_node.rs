use std::any::Any;

use bytemuck::{Pod, Zeroable};

use crate::{
    math::{Mat4, Vec3},
    prelude::*,
    renderer::core::*,
};

pub const SHADOW_MAP_SIZE: u32 = 2048;
// Keep the single local shadow map around the player. In particular, do not include the
// opposite side of a planet in the caster volume.
const SHADOW_MAX_DISTANCE: f32 = 1_000.0;
const SHADOW_HALF_EXTENT: f32 = SHADOW_MAX_DISTANCE;
const SHADOW_DEPTH_RANGE: f32 = SHADOW_MAX_DISTANCE * 2.0;
// The orthographic shadow projection has linear depth. Express the receiver bias in world
// units so changing the shadow range does not silently turn a small bias into tens of metres.
const SHADOW_DEPTH_BIAS_WORLD: f32 = 1.0;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ShadowUniform {
    pub view_proj: [[f32; 4]; 4],
    pub light_direction: [f32; 3],
    pub depth_bias: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct ShadowBindings {
    pub uniform_buffer: BufferHandle,
    pub view_layout: BindGroupLayoutHandle,
    pub view_bind_group: BindGroupHandle,
    pub sampling_layout: BindGroupLayoutHandle,
    pub sampling_bind_group: BindGroupHandle,
    pub depth_texture: TextureHandle,
}

pub struct ShadowPassNode;

impl ShadowPassNode {
    pub fn new() -> Self {
        Self
    }

    pub fn pass_descriptor() -> RenderNodeDescriptor {
        const SHADOW_DEPTH_USAGE: TextureUsages = TextureUsages::RENDER_ATTACHMENT
            .union(TextureUsages::COPY_SRC)
            .union(TextureUsages::TEXTURE_BINDING);

        RenderNodeDescriptor {
            name: "shadow_pass",
            color_attachments: Vec::new(),
            depth_attachment: Some(DepthAttachmentDescriptor {
                name: "shadow_depth_map",
                // Shadows use conventional depth independently of the main reverse-Z camera.
                load_op: AttachmentLoadOp::ClearDepth(1.0),
                store: true,
            }),
            input_textures: Vec::new(),
            output_textures: vec![OutputTexture::Create(TextureSlot {
                name: "shadow_depth_map",
                texture_descriptor: TextureDescriptor {
                    label: "shadow_depth_map".to_string(),
                    size: TextureSize::Custom {
                        width: SHADOW_MAP_SIZE,
                        height: SHADOW_MAP_SIZE,
                    },
                    format: TextureFormat::Depth32Float,
                    dimension: TextureDimension::D2,
                    usage: SHADOW_DEPTH_USAGE,
                    mip_levels: 1,
                    sample_count: 1,
                },
            })],
            input_buffers: Vec::new(),
            output_buffers: Vec::new(),
        }
    }

    fn uniform(camera_position: Vec3) -> ShadowUniform {
        // This direction matches the terrain's directional light. It points from the world
        // toward the light, so the shadow camera sits along it and looks back at the scene.
        let light_direction = Vec3::new(0.3, 0.6, 0.4).normalize();
        let texel_world_size = (SHADOW_HALF_EXTENT * 2.0) / SHADOW_MAP_SIZE as f32;
        let center = Vec3::new(
            (camera_position.x / texel_world_size).round() * texel_world_size,
            (camera_position.y / texel_world_size).round() * texel_world_size,
            (camera_position.z / texel_world_size).round() * texel_world_size,
        );
        let center_relative = center - camera_position;
        let eye_relative = center_relative + light_direction * (SHADOW_DEPTH_RANGE * 0.5);
        let up = if light_direction.y.abs() > 0.95 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        let view = Mat4::look_at_rh(eye_relative, center_relative, up);
        // `orthographic_rh` maps depth directly to wgpu's [0, 1] range: near = 0, far = 1.
        let projection = Mat4::orthographic_rh(
            -SHADOW_HALF_EXTENT,
            SHADOW_HALF_EXTENT,
            -SHADOW_HALF_EXTENT,
            SHADOW_HALF_EXTENT,
            0.1,
            SHADOW_DEPTH_RANGE,
        );
        let view_proj = projection * view;

        ShadowUniform {
            view_proj: view_proj.to_cols_array_2d(),
            light_direction: light_direction.to_array(),
            depth_bias: SHADOW_DEPTH_BIAS_WORLD / (SHADOW_DEPTH_RANGE - 0.1),
        }
    }
}

impl Default for ShadowPassNode {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderNode for ShadowPassNode {
    fn should_render_to_swapchain(&self) -> bool {
        false
    }

    fn describe_pass(&self) -> RenderNodeDescriptor {
        Self::pass_descriptor()
    }

    fn compile(&mut self, ctx: &mut NodeCompileContext) {
        let depth_texture = ctx.output_texture("shadow_depth_map");
        let initial_uniform = Self::uniform(Vec3::ZERO);
        let uniform_buffer = ctx.api.create_buffer(&BufferDescriptor {
            label: "shadow_uniform".into(),
            size: std::mem::size_of::<ShadowUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });
        ctx.api
            .write_buffer(uniform_buffer, bytemuck::bytes_of(&initial_uniform));

        let view_layout = ctx
            .api
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: "shadow_view_layout".into(),
                entries: vec![BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::Both,
                    entry_type: BindingType::UniformBuffer,
                    count: None,
                }],
            });
        let view_bind_group = ctx.api.create_bind_group(&BindGroupDescriptor {
            label: "shadow_view_bind_group".into(),
            layout: view_layout,
            entries: vec![(0, BindGroupEntry::Buffer(uniform_buffer))],
        });

        let sampling_layout = ctx
            .api
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: "shadow_sampling_layout".into(),
                entries: vec![
                    BindGroupLayoutEntry {
                        binding: 0,
                        // `vs_main` uses the matrix to pass light-space position to the
                        // fragment shader; the remaining fields are consumed in fragment.
                        visibility: ShaderStages::Both,
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
                ],
            });
        let sampling_bind_group = ctx.api.create_bind_group(&BindGroupDescriptor {
            label: "shadow_sampling_bind_group".into(),
            layout: sampling_layout,
            entries: vec![
                (0, BindGroupEntry::Buffer(uniform_buffer)),
                (1, BindGroupEntry::Texture(depth_texture)),
            ],
        });

        ctx.render_resources.insert_labeled(
            "shadow_bindings",
            ShadowBindings {
                uniform_buffer,
                view_layout,
                view_bind_group,
                sampling_layout,
                sampling_bind_group,
                depth_texture,
            },
        );
    }

    fn prepare(&mut self, resources: &mut RenderResources, api: &mut dyn RendererAPI) {
        let Some(camera) = resources.get::<CameraData>() else {
            return;
        };
        let camera_position = Vec3::from_array(camera.uniform.position);
        let uniform = Self::uniform(camera_position);
        let buffer = resources
            .get_labeled::<ShadowBindings>("shadow_bindings")
            .expect("shadow bindings must be compiled before preparing shadows")
            .uniform_buffer;
        api.write_buffer(buffer, bytemuck::bytes_of(&uniform));
    }

    fn run(&mut self, ctx: &mut dyn RenderContext, resources: &RenderResources) {
        let shadow = resources
            .get_labeled::<ShadowBindings>("shadow_bindings")
            .expect("shadow bindings must exist while recording shadows");
        ctx.bind_bind_group(0, shadow.view_bind_group);
        if let Some(frame) = resources.get_labeled::<FrameBindings>("frame_bindings") {
            ctx.bind_bind_group(1, frame.materials_bind_group);
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_projection_uses_standard_depth_and_faces_the_light() {
        let camera_position = Vec3::new(120.0, 8_500.0, -75.0);
        let uniform = ShadowPassNode::uniform(camera_position);
        let view_proj = Mat4::from_cols_array_2d(&uniform.view_proj);
        let light_direction = Vec3::from_array(uniform.light_direction);

        let toward_light = view_proj.project_point3(light_direction * (SHADOW_MAX_DISTANCE * 0.5));
        let away_from_light =
            view_proj.project_point3(-light_direction * (SHADOW_MAX_DISTANCE * 0.5));

        assert!(toward_light.z < away_from_light.z);
        assert!((toward_light.z - 0.25).abs() < 0.001);
        assert!((away_from_light.z - 0.75).abs() < 0.001);
    }

    #[test]
    fn starting_terrain_is_inside_the_local_shadow_volume() {
        let camera_position = Vec3::new(0.0, 8_573.0, 0.0);
        let terrain_position = Vec3::new(0.0, 8_192.0, 0.0);
        let uniform = ShadowPassNode::uniform(camera_position);
        let ndc = Mat4::from_cols_array_2d(&uniform.view_proj)
            .project_point3(terrain_position - camera_position);

        assert!(ndc.x.abs() <= 1.0, "shadow x was {}", ndc.x);
        assert!(ndc.y.abs() <= 1.0, "shadow y was {}", ndc.y);
        assert!((0.0..=1.0).contains(&ndc.z), "shadow depth was {}", ndc.z);
    }
}
