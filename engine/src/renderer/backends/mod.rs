pub mod wgpu_backend;
pub use crate::State;
use crate::assets::manager::AssetHeader;
use crate::renderer::core::{
    BufferHandle, PipelineHandle, RenderGraph, RenderNode, RenderPassHandle, TextureHandle,
};
pub use wgpu_backend::WgpuBackend;

pub trait RendererAPI {
    fn compile_pipeline(&mut self, node: &dyn RenderNode) -> PipelineHandle;
    fn create_buffer(&mut self, size: u64, usage: wgpu::BufferUsages) -> BufferHandle;
    fn submit(&mut self, graph: &RenderGraph);
    fn compile_render_graph_node(&mut self, node: &mut dyn RenderNode, device: &wgpu::Device);
    fn render(
        &mut self,
        surface: &wgpu::Surface,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        render_graph: &RenderGraph,
    ) -> anyhow::Result<()>;
    fn render_node(
        &mut self,
        node: &dyn RenderNode,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    );

    // Load assets
    fn load_material(&mut self, header: &AssetHeader);
}
