pub mod editor;
pub mod editor_ui;
pub mod egui_node;
pub mod hierarchy;
pub mod terrain_editor;
pub mod viewport_context_menu;

use egui_node::EguiRenderNode;
use viewport_context_menu::{EditorSpawnRequests, editor_spawn_system};

pub const EGUI_NODE_INDEX: engine::renderer::ids::GraphPassId =
    engine::renderer::ids::graph_passes::EGUI;

#[unsafe(no_mangle)]
pub fn register_editor(state: &mut engine::State) {
    engine::profiling::init(true);
    log::info!("Editor logic profiling initialized");

    if let Some(scene) = state.active_scene_mut() {
        scene
            .world_mut()
            .insert_resource(EditorSpawnRequests::default());
        #[cfg(not(feature = "dynamic_linking"))]
        scene
            .update_schedule_mut()
            .add_named_system("editor.spawn", hot_editor_spawn_system);
    }

    let egui_node = EguiRenderNode::new();
    state
        .global_resources
        .renderer
        .render_graph
        .nodes
        .push((EGUI_NODE_INDEX, Box::new(egui_node)));
    state.global_resources.renderer.render_graph.compile(
        &mut state.global_resources.renderer.render_resources,
        state.global_resources.renderer.renderer_api.as_mut(),
    );
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn hot_editor_spawn_system(
    ctx: &mut engine::ecs::system::SystemContext,
    commands: &mut engine::ecs::commands::Commands,
) {
    editor_spawn_system(ctx, commands);
}

#[unsafe(no_mangle)]
pub fn update_editor(state: &mut engine::State) {
    update_editor_impl(state);
}

fn update_editor_impl(state: &mut engine::State) {
    engine::profile_scope!("editor.update");
    let taken_node = {
        engine::profile_scope!("editor.egui.take_node");
        state
            .global_resources
            .renderer
            .render_graph
            .take_node(EGUI_NODE_INDEX)
    };
    if let Some(mut taken_node) = taken_node {
        if let Some(egui_node) = taken_node.as_any_mut().downcast_mut::<EguiRenderNode>() {
            engine::profile_scope!("editor.egui.process");
            egui_node.process(state);
        }
        {
            engine::profile_scope!("editor.egui.return_node");
            state
                .global_resources
                .renderer
                .render_graph
                .return_node(taken_node);
        }
    }
}
