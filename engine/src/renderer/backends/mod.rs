pub mod wgpu_backend;
use std::collections::HashMap;

pub use crate::State;
use crate::assets::manager::{AssetHeader, Handle};
use crate::assets::material::{Material, TextureAsset};
use crate::math::UVec2;
use crate::model::MeshAsset;
use crate::renderer::core::{
    BufferHandle, GraphResources, MeshDrawRange, PipelineHandle, PipelineTargetInfo, RenderGraph,
    RenderNode, RenderNodeDescriptor, TextureHandle,
};
use crate::renderer::{
    BindGroupDescriptor, BindGroupHandle, BindGroupLayoutDescriptor, BindGroupLayoutHandle,
    BufferDescriptor, RenderData, RenderResources, SamplerDescriptor, SamplerHandle,
    TextureDescriptor,
};
use crate::texture;
use uuid::Uuid;

pub trait RendererAPI {
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    fn compile(&mut self);
    fn resize(&mut self, width: u32, height: u32);
    fn resize_texture(&mut self, texture_handle: &TextureHandle, descriptor: &TextureDescriptor);
    fn compile_pipeline(&mut self, node: &dyn RenderNode) -> PipelineHandle;
    fn submit(&mut self, graph: &RenderGraph);
    fn render(
        &mut self,
        render_graph: &mut RenderGraph,
        render_resources: &mut RenderResources,
    ) -> anyhow::Result<()>;
    fn reload_shader(&mut self, shader_path: &str);
    fn reload_shaders(&mut self);

    // Load assets
    fn create_white_texture(&mut self);
    fn get_white_texture(&self) -> TextureHandle;
    fn get_default_sampler(&self) -> SamplerHandle;

    fn upload_mesh(&mut self, mesh: &MeshAsset) -> Handle<MeshAsset>;
    fn create_texture_asset(&mut self, texture: &TextureAsset) -> TextureHandle;
    fn upload_texture_asset(&mut self, texture: &TextureAsset, index: Option<u32>)
    -> TextureHandle;
    fn is_texture_asset_uploaded(&self, uuid: Uuid) -> bool;
    fn upload_material_asset(&mut self, material: &Material, index: Option<u32>) -> u32;
    fn load_texture(&mut self, path: &String, descriptor: &TextureDescriptor, index: Option<u32>);
    fn load_material(&mut self, header: &AssetHeader) -> Material;
    fn create_pipeline(
        &mut self,
        material: &Material,
        bind_group_layouts: &[BindGroupLayoutHandle],
        target_info: &PipelineTargetInfo,
    );
    fn update_pipeline(
        &mut self,
        material: &Material,
        bind_group_layouts: &[BindGroupLayoutHandle],
        target_info: &PipelineTargetInfo,
    ) {
        self.create_pipeline(material, bind_group_layouts, target_info);
    }
    fn target_info_for_pass(
        &self,
        descriptor: &RenderNodeDescriptor,
        resources: &GraphResources,
    ) -> PipelineTargetInfo;
    fn create_texture(&mut self, descriptor: &TextureDescriptor) -> TextureHandle;
    fn create_buffer(&mut self, descriptor: &BufferDescriptor) -> BufferHandle;
    fn create_sampler(&mut self, descriptor: &SamplerDescriptor) -> SamplerHandle;
    fn create_bind_group(&mut self, descriptor: &BindGroupDescriptor) -> BindGroupHandle;
    fn create_bind_group_layout(
        &mut self,
        descriptor: &BindGroupLayoutDescriptor,
    ) -> BindGroupLayoutHandle;
    fn write_buffer(&mut self, buffer: BufferHandle, data: &[u8]);

    fn read_texture_bytes_at(&mut self, texture: &TextureHandle, x: f32, y: f32, out: &mut [u8]);
    fn upload_texture(&mut self, texture: &TextureHandle, index: Option<u32>);

    // Get using uuids
    fn get_pipeline(&mut self, uuid: Uuid) -> Option<PipelineHandle>;

    fn get_mesh_vertex_buffer(&mut self, mesh: &Handle<MeshAsset>) -> BufferHandle;
    fn get_mesh_index_buffer(&mut self, mesh: &Handle<MeshAsset>) -> BufferHandle;
    fn get_mesh_index_count(&mut self, mesh: &Handle<MeshAsset>) -> u32;
    fn get_mesh_draw_range(&mut self, mesh: &Handle<MeshAsset>) -> MeshDrawRange;
    fn get_mesh_instance_count(&mut self, mesh: &Handle<MeshAsset>) -> u32;
    fn get_mesh_instance_buffer(&mut self, mesh: &Handle<MeshAsset>) -> BufferHandle;

    fn get_texture_size(&self, handle: &TextureHandle) -> UVec2;
    fn get_surface_size(&self) -> UVec2;

    // Temporary
    fn set_texture(&mut self, texture: &texture::Texture);
    fn create_render_data(
        &mut self,
        vertex_bytes: &Vec<u8>,
        indices: &Vec<u32>,
        material: Material,
        pipeline_handle: &PipelineHandle,
    ) -> RenderData;
}

pub trait RenderContext {
    fn api(&mut self) -> &mut dyn RendererAPI;
    fn bind_pipeline(&mut self, pipeline: PipelineHandle);
    fn bind_vertex_buffer(&mut self, slot: u32, buffer: BufferHandle);
    fn bind_vertex_buffer_range(&mut self, slot: u32, buffer: BufferHandle, offset: u64, size: u64);
    fn bind_index_buffer(&mut self, buffer: BufferHandle);
    fn bind_bind_group(&mut self, index: u32, bind_group: BindGroupHandle);
    fn draw(&mut self, vertices: u32, instances: u32);
    fn draw_indexed(
        &mut self,
        first_index: u32,
        index_count: u32,
        base_vertex: i32,
        instances: u32,
    );

    /// Run a closure with the raw wgpu render pass (with 'static lifetime via forget_lifetime).
    /// This is the escape hatch for nodes that need direct backend access (e.g. egui).
    fn with_raw_pass(&mut self, _f: &mut dyn FnMut(&mut wgpu::RenderPass<'static>)) {}

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
    fn get_mesh_draw_range(&mut self, mesh: &Handle<MeshAsset>) -> MeshDrawRange {
        self.api().get_mesh_draw_range(mesh)
    }
    fn get_mesh_instance_count(&mut self, mesh: &Handle<MeshAsset>) -> u32 {
        self.api().get_mesh_instance_count(mesh)
    }
    fn get_mesh_instance_buffer(&mut self, mesh: &Handle<MeshAsset>) -> BufferHandle {
        self.api().get_mesh_instance_buffer(mesh)
    }
}

pub struct NodeCompileContext<'a> {
    pub api: &'a mut dyn RendererAPI,
    pub render_resources: &'a mut RenderResources,
    pub resolved_inputs: HashMap<&'static str, TextureHandle>,
    pub resolved_outputs: HashMap<&'static str, TextureHandle>,
    pub target_info: PipelineTargetInfo,
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

pub trait RendererReadTextureExt {
    fn read_texture<T: bytemuck::Pod>(&mut self, texture: &TextureHandle, x: f32, y: f32) -> T;
}

impl<R: RendererAPI + ?Sized> RendererReadTextureExt for R {
    fn read_texture<T: bytemuck::Pod>(&mut self, texture: &TextureHandle, x: f32, y: f32) -> T {
        let mut value = std::mem::MaybeUninit::<T>::uninit();
        let bytes = unsafe {
            std::slice::from_raw_parts_mut(value.as_mut_ptr() as *mut u8, std::mem::size_of::<T>())
        };

        self.read_texture_bytes_at(texture, x, y, bytes);

        unsafe { value.assume_init() }
    }
}
