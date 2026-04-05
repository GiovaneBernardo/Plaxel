pub mod wgpu_backend;
use std::option;

pub use crate::State;
use crate::assets::manager::{AssetHeader, Handle};
use crate::assets::material::Material;
use crate::model::MeshAsset;
use crate::renderer::RenderData;
use crate::renderer::core::{
    BufferHandle, PipelineHandle, RenderGraph, RenderNode, RenderPassHandle, TextureHandle,
};
use crate::texture;
use uuid::Uuid;
pub use wgpu_backend::WgpuBackend;

pub trait RendererAPI {
    fn compile(&mut self);
    fn resize(&mut self, width: u32, height: u32);
    fn compile_pipeline(&mut self, node: &dyn RenderNode) -> PipelineHandle;
    fn create_buffer(&mut self, size: u64, usage: wgpu::BufferUsages) -> BufferHandle;
    fn submit(&mut self, graph: &RenderGraph);
    fn compile_render_graph_node(&mut self, node: &mut Box<dyn RenderNode>);
    fn render(&mut self, render_graph: &mut RenderGraph) -> anyhow::Result<()>;

    // Load assets
    fn load_mesh(&mut self, mesh: &MeshAsset) -> Handle<MeshAsset>;
    fn load_material(&mut self, header: &AssetHeader) -> Material;
    fn create_pipeline(&mut self, material: &Material);

    // Get using uuids
    fn get_pipeline(&mut self, uuid: Uuid) -> Option<PipelineHandle>;

    fn get_mesh_vertex_buffer(&mut self, mesh: &Handle<MeshAsset>) -> BufferHandle;
    fn get_mesh_index_buffer(&mut self, mesh: &Handle<MeshAsset>) -> BufferHandle;
    fn get_mesh_index_count(&mut self, mesh: &Handle<MeshAsset>) -> u32;

    // Temporary
    fn set_texture(&mut self, texture: &texture::Texture);
    fn create_render_data(
        &mut self,
        positions: Vec<cgmath::Point3<f32>>,
        material: Material,
    ) -> RenderData;
}

pub trait RenderContext {
    fn api(&mut self) -> &mut dyn RendererAPI;

    fn bind_pipeline(&mut self, pipeline: PipelineHandle);
    fn bind_vertex_buffer(&mut self, buffer: BufferHandle);
    fn bind_index_buffer(&mut self, buffer: BufferHandle);
    fn draw(&mut self, vertices: u32, instances: u32);
    fn draw_indexed(&mut self, indices: u32, instances: u32);

    fn get_pipeline(&mut self, uuid: Uuid) -> Option<PipelineHandle> {
        self.api().get_pipeline(uuid)
    }
    fn get_mesh_vertex_buffer(&mut self, mesh: &Handle<MeshAsset>) -> BufferHandle {
        self.api().get_mesh_vertex_buffer(mesh)
    }
    fn get_mesh_index_buffer(&mut self, mesh: &Handle<MeshAsset>) -> BufferHandle {
        self.api().get_mesh_index_buffer(mesh)
    }
    fn get_mesh_index_count(&mut self, mesh: &Handle<MeshAsset>) -> u32 {
        self.api().get_mesh_index_count(mesh)
    }
}
