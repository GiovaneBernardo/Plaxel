use crate::octree::{self, OctreeNode};
use engine::model::*;

pub struct Planet {
    pub id: u64,
    pub name: String,
    pub octree_root: OctreeNode,
}

pub struct PlanetMesh {
    pub positions: Vec<PlanetVertex>,
    pub indices: Vec<u32>,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PlanetVertex {
    pub position: [f32; 3], // 12 bytes
    pub normal: [f32; 3],   // 12 bytes
    pub mat_a: u16,         //  2 bytes  (material index A)
    pub mat_b: u16,         //  2 bytes  (material index B)
    pub blend: u8,          //  1 byte   (0=full A, 255=full B)
    pub _pad: [u8; 3],      // 3 bytes to force total of 32 bytes
}

impl Vertex for PlanetVertex {
    fn layout() -> VertexLayout {
        use std::mem;

        VertexLayout {
            stride: mem::size_of::<PlanetVertex>() as u64,
            step_mode: StepMode::Vertex,
            attributes: vec![
                // position
                VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: AttributeFormat::Float32x3,
                },
                // normal
                VertexAttribute {
                    offset: mem::size_of::<[f32; 3]>() as u64,
                    shader_location: 1,
                    format: AttributeFormat::Float32x3,
                },
                // mats (u32)
                VertexAttribute {
                    offset: mem::size_of::<[f32; 6]>() as u64,
                    shader_location: 2,
                    format: AttributeFormat::Uint32,
                },
                // blend (u32)
                VertexAttribute {
                    offset: (mem::size_of::<[f32; 6]>() + mem::size_of::<u32>()) as u64,
                    shader_location: 3,
                    format: AttributeFormat::Uint32,
                },
            ],
        }
    }
}

pub struct PlanetInstance {}

impl PlanetInstance {
    pub fn layout() -> VertexLayout {
        VertexLayout {
            stride: std::mem::size_of::<[[f32; 4]; 4]>() as u64,
            step_mode: StepMode::Instance,
            attributes: vec![
                VertexAttribute {
                    offset: 0,
                    shader_location: 5,
                    format: AttributeFormat::Float32x4,
                },
                VertexAttribute {
                    offset: std::mem::size_of::<[f32; 4]>() as u64,
                    shader_location: 6,
                    format: AttributeFormat::Float32x4,
                },
                VertexAttribute {
                    offset: std::mem::size_of::<[f32; 8]>() as u64,
                    shader_location: 7,
                    format: AttributeFormat::Float32x4,
                },
                VertexAttribute {
                    offset: std::mem::size_of::<[f32; 12]>() as u64,
                    shader_location: 8,
                    format: AttributeFormat::Float32x4,
                },
            ],
        }
    }
}
