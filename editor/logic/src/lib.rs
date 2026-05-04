pub mod editor;
pub mod egui_node;
pub mod hierarchy;

use egui_node::EguiRenderNode;

pub const EGUI_NODE_INDEX: i8 = 10;

#[unsafe(no_mangle)]
pub fn register_editor(state: &mut engine::State) {
    let egui_node = EguiRenderNode::new();
    state
        .renderer
        .render_graph
        .nodes
        .push((EGUI_NODE_INDEX, Box::new(egui_node)));
    state.renderer.render_graph.compile(
        &mut state.renderer.render_resources,
        state.renderer.renderer_api.as_mut(),
    );
}

#[unsafe(no_mangle)]
pub fn update_editor(state: &mut engine::State) {
    if let Some(mut node_box) = state.renderer.render_graph.take_node(EGUI_NODE_INDEX) {
        if let Some(egui_node) = node_box.as_any_mut().downcast_mut::<EguiRenderNode>() {
            egui_node.process(state);
        }
        state
            .renderer
            .render_graph
            .return_node(EGUI_NODE_INDEX, node_box);
    }
}
