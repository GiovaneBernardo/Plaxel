use std::ops::Range;

use wgpu::VertexFormat;

use crate::texture;

pub trait Vertex {
    fn desc() -> wgpu::VertexBufferLayout<'static>;
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
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<ModelVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 5]>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        }
    }

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
            format: AttributeFormat::Float32x3,
        });
        attributes.push(VertexAttribute {
            offset: mem::size_of::<[f32; 5]>() as u64,
            shader_location: 2,
            format: AttributeFormat::Float32x3,
        });

        VertexLayout {
            stride: mem::size_of::<ModelVertex>() as u64,
            attributes,
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

#[derive(Debug, Clone, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VertexLayout {
    pub stride: u64,
    pub attributes: Vec<VertexAttribute>,
}

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VertexAttribute {
    pub offset: u64,
    pub shader_location: u32,
    pub format: AttributeFormat,
}

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AttributeFormat {
    Float32,
    Float32x2,
    Float32x3,
    Float32x4,
    Uint32,
    Uint8x4,
    Snorm8x4,
    Unorm8x4,
    // add as needed
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub struct MeshAsset {
    pub name: String,
    pub vertices: Vec<u8>,
    pub indices: Vec<u32>,
    pub vertex_layout: VertexLayout,
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
