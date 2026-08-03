use std::{error::Error, fmt};

use offset_allocator::Allocation;

use crate::{
    model::VertexLayout,
    renderer::{BufferHandle, MeshDrawRange, gpu::GpuHandle, pool::VertexPoolId},
};

pub type GpuMeshHandle = GpuHandle<GpuMesh>;

pub struct GpuMesh {
    pub(crate) pool: VertexPoolId,
    pub(crate) vertex_allocation: Allocation,
    pub(crate) index_page: u32,
    pub(crate) index_allocation: Allocation,
    pub(crate) draw_range: MeshDrawRange,
}

pub struct MeshUpload<'a> {
    pub label: &'a str,
    pub vertices: &'a [u8],
    pub indices: &'a [u32],
    pub vertex_layout: &'a VertexLayout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshUploadError {
    EmptyVertices,
    EmptyIndices,
    InvalidVertexStride(u64),
    MisalignedVertexData { bytes: usize, stride: usize },
    TooManyVertices(usize),
    TooManyIndices(usize),
}

impl fmt::Display for MeshUploadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::EmptyVertices => formatter.write_str("mesh has no vertex data"),
            Self::EmptyIndices => formatter.write_str("mesh has no index data"),
            Self::InvalidVertexStride(stride) => {
                write!(formatter, "mesh vertex stride {stride} is invalid")
            }
            Self::MisalignedVertexData { bytes, stride } => write!(
                formatter,
                "mesh vertex data length {bytes} is not a multiple of stride {stride}"
            ),
            Self::TooManyVertices(count) => {
                write!(formatter, "mesh vertex count {count} exceeds u32 capacity")
            }
            Self::TooManyIndices(count) => {
                write!(formatter, "mesh index count {count} exceeds u32 capacity")
            }
        }
    }
}

impl Error for MeshUploadError {}

#[derive(Debug, Clone, Copy)]
pub struct GpuMeshBinding {
    pub vertex_buffer: BufferHandle,
    pub index_buffer: BufferHandle,
    pub draw_range: MeshDrawRange,
}
