use engine::model::*;

pub struct Planet {
    pub id: u64,
    pub name: String,
    pub mesh: PlanetMesh,
}

pub struct PlanetMesh {
    pub positions: Vec<PlanetVertex>,
    pub indices: Vec<u32>,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PlanetVertex {
    pub position: [f32; 3],
    pub tex_coords: [f32; 2],
    pub normal: [f32; 3],
}

impl Vertex for PlanetVertex {
    fn layout() -> VertexLayout {
        use std::mem;

        let mut attributes = Vec::new();

        // Position
        attributes.push(VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: AttributeFormat::Float32x3,
        });

        // Uvs
        attributes.push(VertexAttribute {
            offset: mem::size_of::<[f32; 3]>() as u64,
            shader_location: 1,
            format: AttributeFormat::Float32x2,
        });

        // Normal
        attributes.push(VertexAttribute {
            offset: mem::size_of::<[f32; 5]>() as u64,
            shader_location: 2,
            format: AttributeFormat::Float32x3,
        });

        VertexLayout {
            stride: mem::size_of::<PlanetVertex>() as u64,
            step_mode: StepMode::Vertex,
            attributes,
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
