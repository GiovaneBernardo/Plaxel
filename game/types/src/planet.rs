use engine::prelude::*;
use std::{collections::HashMap, sync::Arc};

use crate::octree::{DensityRange, OctreeNode};
use engine::math::Vec3;
use engine::{
    assets::{manager::Handle, material::TextureAsset},
    ecs::entity::Entity,
    model::*,
};

#[derive(Clone, Debug, plaxel_reflect::Reflect)]
#[reflect(from_reflect = false)]
pub struct Planet {
    pub id: u64,
    pub name: String,
    pub position: Vec3,
    #[reflect(ignore)]
    pub octree_root: OctreeNode,
    pub solar_system: Entity,
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

#[derive(Clone, plaxel_reflect::Reflect)]
#[reflect(from_reflect = false)]
pub struct PlanetTerrainEdits {
    #[reflect(ignore)]
    pub modified_chunks: HashMap<TerrainBrickKey, Arc<TerrainBrickSamples>>,
    /// Cached value bounds for each trilinearly sampled edit brick.
    #[reflect(ignore)]
    pub modified_ranges: HashMap<TerrainBrickKey, DensityRange>,
}

pub type TerrainBrickSamples = Vec<Vec<Vec<f32>>>;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TerrainBrickKey {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub level: u32,
}

#[derive(Clone)]
pub struct TerrainBrickEdits {
    pub resolution: u32,
    pub offsets: Vec<f32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlanetTerrainMaterial {
    pub name: String,
    pub diffuse: Handle<TextureAsset>,
    pub normal: Option<Handle<TextureAsset>>,
    pub displacement: Option<Handle<TextureAsset>>,
    pub roughness: Option<Handle<TextureAsset>>,
    pub texture_scale: f32,
    pub displacement_scale: f32,
    pub roughness_factor: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuPlanetTerrainMaterial {
    pub diffuse_texture_index: u32,
    pub normal_texture_index: u32,
    pub displacement_texture_index: u32,
    pub roughness_texture_index: u32,

    pub texture_scale: f32,
    pub displacement_scale: f32,
    pub roughness_factor: f32,
    pub flags: u32,
}

pub enum VoxelMaterial {
    Unknown,
    Air,
    Water,
    Dirt,
    Grass,
    Stone,
    Snow,
    Sand,
    Hillstone,
    DeepStone,
    IronOre,
}
