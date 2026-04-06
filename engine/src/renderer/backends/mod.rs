pub mod wgpu_backend;
use std::collections::HashMap;
use std::option;

pub use crate::State;
use crate::assets::manager::{AssetHeader, Handle};
use crate::assets::material::{Material, PipelineDescriptor};
use crate::model::MeshAsset;
use crate::renderer::core::{
    BufferHandle, PipelineHandle, RenderGraph, RenderNode, RenderPassHandle, TextureHandle,
};
use crate::renderer::{
    BindGroupDescriptor, BindGroupHandle, BindGroupLayoutDescriptor, BindGroupLayoutHandle,
    BufferDescriptor, BufferUsages, RenderData, RenderResources, TextureDescriptor,
};
use crate::texture;
use uuid::Uuid;

pub trait RendererAPI {
    fn compile(&mut self);
    fn resize(&mut self, width: u32, height: u32);
    fn compile_pipeline(&mut self, node: &dyn RenderNode) -> PipelineHandle;
    fn submit(&mut self, graph: &RenderGraph);
    fn compile_render_graph_node(&mut self, node: &mut Box<dyn RenderNode>);
    fn render(&mut self, render_graph: &mut RenderGraph) -> anyhow::Result<()>;

    // Load assets
    fn load_mesh(&mut self, mesh: &MeshAsset) -> Handle<MeshAsset>;
    fn load_material(&mut self, header: &AssetHeader) -> Material;
    fn create_pipeline(
        &mut self,
        material: &Material,
        bind_group_layouts: &[BindGroupLayoutHandle],
    );
    fn create_texture(&mut self, descriptor: &TextureDescriptor) -> TextureHandle;
    fn create_buffer(&mut self, descriptor: &BufferDescriptor) -> BufferHandle;
    fn create_bind_group(&mut self, descriptor: &BindGroupDescriptor) -> BindGroupHandle;
    fn create_bind_group_layout(
        &mut self,
        descriptor: &BindGroupLayoutDescriptor,
    ) -> BindGroupLayoutHandle;
    fn write_buffer(&mut self, buffer: BufferHandle, data: &[u8]);

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
        pipeline_handle: &PipelineHandle,
    ) -> RenderData;
}

pub trait RenderContext {
    fn api(&mut self) -> &mut dyn RendererAPI;

    fn bind_pipeline(&mut self, pipeline: PipelineHandle);
    fn bind_vertex_buffer(&mut self, buffer: BufferHandle);
    fn bind_index_buffer(&mut self, buffer: BufferHandle);
    fn bind_bind_group(&mut self, index: u32, bind_group: BindGroupHandle);
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

pub struct NodeCompileContext<'a> {
    pub api: &'a mut dyn RendererAPI,
    pub render_resources: &'a mut RenderResources,
    pub resolved_inputs: HashMap<&'static str, TextureHandle>,
    pub resolved_outputs: HashMap<&'static str, TextureHandle>,
}

impl<'a> NodeCompileContext<'a> {
    // Graph-specific: resolve declared resource names to actual handles
    pub fn input_texture(&self, name: &str) -> TextureHandle {
        *self
            .resolved_inputs
            .get(name)
            .unwrap_or_else(|| panic!("Node declared no input named '{name}'"))
    }

    pub fn output_texture(&self, name: &str) -> TextureHandle {
        *self
            .resolved_outputs
            .get(name)
            .unwrap_or_else(|| panic!("Node declared no output named '{name}'"))
    }

    // Forward allocations to the backend
    pub fn create_buffer(&mut self, descriptor: &BufferDescriptor) -> BufferHandle {
        self.api.create_buffer(descriptor)
    }

    pub fn create_bind_group_layout(
        &mut self,
        descriptor: &BindGroupLayoutDescriptor,
    ) -> BindGroupLayoutHandle {
        self.api.create_bind_group_layout(descriptor)
    }

    pub fn create_bind_group(&mut self, descriptor: &BindGroupDescriptor) -> BindGroupHandle {
        self.api.create_bind_group(descriptor)
    }
}
