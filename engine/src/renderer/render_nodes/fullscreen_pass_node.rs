use crate::assets::material::Material;
use crate::renderer::core::*;

pub struct FullscreenPassNode {
    pub material: Material,
    pub bind_group_layouts: Vec<BindGroupLayoutHandle>,
}

impl FullscreenPassNode {
    pub fn new(material: Material, bind_group_layouts: Vec<BindGroupLayoutHandle>) -> Self {
        Self {
            material,
            bind_group_layouts,
        }
    }

    pub fn compile(&self, ctx: &mut NodeCompileContext) {
        ctx.api
            .create_pipeline(&self.material, &self.bind_group_layouts, &ctx.target_info);
    }

    pub fn run(&self, ctx: &mut dyn RenderContext, bind_groups: &[BindGroupHandle]) {
        let pipeline = ctx
            .get_pipeline(self.material.pipeline_descriptor.uuid)
            .unwrap();
        ctx.bind_pipeline(pipeline);

        for (index, bind_group) in bind_groups.iter().enumerate() {
            ctx.bind_bind_group(index as u32, *bind_group);
        }

        ctx.draw(3, 1);
    }
}
