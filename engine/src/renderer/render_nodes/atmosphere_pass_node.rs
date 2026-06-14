use std::any::Any;

use crate::assets::material::Material;
use crate::renderer::*;

pub struct AtmospherePassNode {
    fullscreen: FullscreenPassNode,
}

impl AtmospherePassNode {
    pub fn new() -> Self {
        let material = Material::new("shaders/atmosphere.wgsl".to_string())
            .with_vertex_layouts(Vec::new())
            .with_depth(None)
            .with_blend(BlendMode::Alpha);

        Self {
            fullscreen: FullscreenPassNode::new(material, Vec::new()),
        }
    }

    pub fn pass_descriptor() -> RenderNodeDescriptor {
        RenderNodeDescriptor {
            name: "atmosphere",
            color_attachments: vec![ColorAttachmentDescriptor {
                name: "swapchain_image",
                load_op: AttachmentLoadOp::Load,
                store: true,
            }],
            depth_attachment: None,
            input_textures: Vec::new(),
            output_textures: vec![OutputTexture::WriteTo("swapchain_image")],
            input_buffers: Vec::new(),
            output_buffers: Vec::new(),
        }
    }
}

impl RenderNode for AtmospherePassNode {
    fn should_render_to_swapchain(&self) -> bool {
        true
    }

    fn needs_depth(&self) -> bool {
        false
    }

    fn describe_pass(&self) -> RenderNodeDescriptor {
        Self::pass_descriptor()
    }

    fn compile(&mut self, ctx: &mut NodeCompileContext) {
        self.fullscreen.compile(ctx);
    }

    fn prepare(&mut self, _resources: &mut RenderResources, _api: &mut dyn RendererAPI) {}

    fn run(&mut self, ctx: &mut dyn RenderContext, _render_resources: &RenderResources) {
        self.fullscreen.run(ctx, &[]);
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
