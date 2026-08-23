pub mod backends;
pub mod core;
pub mod default_meshes;
pub mod gpu;
pub mod gpu_mesh;
pub mod ids;
pub mod model;
pub mod plugin;
pub mod pool;
pub mod producer;
pub mod render_database;
pub mod render_graph;
pub mod render_nodes;
pub mod resources;
pub mod texture;
pub mod types;

pub use self::types::*;
pub use backends::*;
pub use core::Renderer;
pub use core::*;
pub use default_meshes::*;
pub use gpu::*;
pub use gpu_mesh::*;
pub use ids::*;
pub use producer::*;
pub use render_database::*;

pub mod prelude {
    pub use super::backends::{NodeCompileContext, RenderContext, RendererAPI};
    pub use super::core::{
        CameraData, FrameBindings, RenderGraph, RenderNode, RenderResources, Renderer,
    };
    pub use super::default_meshes::DefaultMeshes;
    pub use super::gpu_mesh::{GpuMeshBinding, GpuMeshHandle, MeshUpload, MeshUploadError};

    pub use super::ids::*;
    pub use super::plugin::*;
    pub use super::producer::*;
    pub use super::render_database::*;
    pub use super::render_graph::*;
    pub use super::resources::*;
    pub use super::texture::*;
    pub use super::types::*;
}
