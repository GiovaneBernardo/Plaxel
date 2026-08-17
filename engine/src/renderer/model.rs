use std::ops::Range;

use uuid::Uuid;

use crate::{assets::manager::Asset, prelude::*, texture};

pub trait Vertex {
    fn layout() -> VertexLayout;
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ModelVertex {
    pub position: [f32; 3],
    pub tex_coords: [f32; 2],
    pub normal: [f32; 3],
}

impl Vertex for ModelVertex {
    fn layout() -> VertexLayout {
        use std::mem;
        let mut attributes = Vec::new();
        attributes.push(VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: AttributeFormat::Float32x3,
        });
        attributes.push(VertexAttribute {
            offset: mem::size_of::<[f32; 3]>() as u64,
            shader_location: 1,
            format: AttributeFormat::Float32x2,
        });
        attributes.push(VertexAttribute {
            offset: mem::size_of::<[f32; 5]>() as u64,
            shader_location: 2,
            format: AttributeFormat::Float32x3,
        });

        VertexLayout {
            stride: mem::size_of::<ModelVertex>() as u64,
            step_mode: StepMode::Vertex,
            attributes,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TransformInstance {
    pub model_matrix: [[f32; 4]; 4],
    pub material_index: u32,
}

impl Vertex for TransformInstance {
    fn layout() -> VertexLayout {
        use std::mem;

        VertexLayout {
            stride: mem::size_of::<TransformInstance>() as u64,
            step_mode: StepMode::Instance,
            attributes: vec![
                VertexAttribute {
                    offset: 0,
                    shader_location: 5,
                    format: AttributeFormat::Float32x4,
                },
                VertexAttribute {
                    offset: mem::size_of::<[f32; 4]>() as u64,
                    shader_location: 6,
                    format: AttributeFormat::Float32x4,
                },
                VertexAttribute {
                    offset: mem::size_of::<[f32; 8]>() as u64,
                    shader_location: 7,
                    format: AttributeFormat::Float32x4,
                },
                VertexAttribute {
                    offset: mem::size_of::<[f32; 12]>() as u64,
                    shader_location: 8,
                    format: AttributeFormat::Float32x4,
                },
                VertexAttribute {
                    offset: mem::size_of::<[[f32; 4]; 4]>() as u64,
                    shader_location: 9,
                    format: AttributeFormat::Uint32,
                },
            ],
        }
    }
}

#[derive(Clone)]
pub struct Model {
    pub meshes: Vec<MeshAsset>,
    pub materials: Vec<Material>,
}

#[derive(Clone)]
pub struct Material {
    #[allow(dead_code)]
    pub name: String,
    #[allow(dead_code)]
    pub diffuse_texture: texture::Texture,
    pub bind_group: wgpu::BindGroup,
}

#[derive(Clone, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MeshAsset {
    pub name: String,
    pub uuid: Uuid,
    pub vertices: Vec<u8>,
    pub indices: Vec<u32>,
    #[serde(default)]
    pub material_uuid: Option<Uuid>,
    pub vertex_layout: VertexLayout,
}

impl Asset for MeshAsset {
    fn uuid(&self) -> Uuid {
        self.uuid
    }
}

pub trait DrawModel<'a> {
    #[allow(unused)]
    fn draw_mesh(
        &mut self,
        mesh: &'a MeshAsset,
        material: &'a Material,
        camera_bind_group: &'a wgpu::BindGroup,
    );
    fn draw_mesh_instanced(
        &mut self,
        mesh: &'a MeshAsset,
        material: &'a Material,
        instances: Range<u32>,
        camera_bind_group: &'a wgpu::BindGroup,
    );

    #[allow(unused)]
    fn draw_model(&mut self, model: &'a Model, camera_bind_group: &'a wgpu::BindGroup);
    fn draw_model_instanced(
        &mut self,
        model: &'a Model,
        instances: Range<u32>,
        camera_bind_group: &'a wgpu::BindGroup,
    );
}

//impl<'a, 'b> DrawModel<'b> for wgpu::RenderPass<'a>
//where
//    'b: 'a,
//{
//    fn draw_mesh(
//        &mut self,
//        mesh: &'b MeshAsset,
//        material: &'b Material,
//        camera_bind_group: &'b wgpu::BindGroup,
//    ) {
//        self.draw_mesh_instanced(mesh, material, 0..1, camera_bind_group);
//    }
//
//    fn draw_mesh_instanced(
//        &mut self,
//        mesh: &'b MeshAsset,
//        material: &'b Material,
//        instances: Range<u32>,
//        camera_bind_group: &'b wgpu::BindGroup,
//    ) {
//        self.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
//        self.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
//        self.set_bind_group(0, &material.bind_group, &[]);
//        self.set_bind_group(1, camera_bind_group, &[]);
//        self.draw_indexed(0..mesh.num_elements, 0, instances);
//    }
//
//    fn draw_model(&mut self, model: &'b Model, camera_bind_group: &'b wgpu::BindGroup) {
//        self.draw_model_instanced(model, 0..1, camera_bind_group);
//    }
//
//    fn draw_model_instanced(
//        &mut self,
//        model: &'b Model,
//        instances: Range<u32>,
//        camera_bind_group: &'b wgpu::BindGroup,
//    ) {
//        for mesh in &model.meshes {
//            let material = &model.materials[mesh.material];
//            self.draw_mesh_instanced(mesh, material, instances.clone(), camera_bind_group);
//        }
//    }
//}
