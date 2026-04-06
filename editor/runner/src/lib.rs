use editor_logic::egui_node::EguiRenderNode;
use engine::renderer::GeometryPassNode;

#[cfg(feature = "hot-reload")]
#[hot_lib_reloader::hot_module(
    dylib = "game_logic",
    lib_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/debug")
)]
mod game {
    use engine::KeyCode;
    hot_functions_from_file!("game/logic/src/lib.rs");
}

#[cfg(feature = "hot-reload")]
#[hot_lib_reloader::hot_module(
    dylib = "editor_logic",
    lib_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/debug")
)]
mod editor_hot {
    hot_functions_from_file!("editor/logic/src/hierarchy.rs");
    hot_functions_from_file!("editor/logic/src/lib.rs");
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_main() {
    run_editor().unwrap();
}

const EGUI_NODE_INDEX: i8 = 10;

pub fn run_editor() -> anyhow::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    engine::logging::init();

    let event_loop = winit::event_loop::EventLoop::with_user_event().build()?;
    let mut app = engine::App::new(
        #[cfg(target_arch = "wasm32")]
        &event_loop,
    )
    .with_register_system(|state| {
        // Add egui render node to the graph (runs after geometry at priority 10)
        let egui_node = EguiRenderNode::new();
        state
            .renderer
            .render_graph
            .nodes
            .push((EGUI_NODE_INDEX, Box::new(egui_node)));

        // Recompile the graph so the new node gets compiled
        state
            .renderer
            .render_graph
            .compile(&mut state.renderer.render_resources, state.renderer.renderer_api.as_mut());

        #[cfg(feature = "hot-reload")]
        game::register_systems(state);
        #[cfg(not(feature = "hot-reload"))]
        game_logic::register_systems(state);
    })
    .with_update(move |state| {
        #[cfg(feature = "hot-reload")]
        game::update(state);
        #[cfg(not(feature = "hot-reload"))]
        game_logic::update(state);

        // Process egui input/UI – take the node out so we can pass &mut State
        // without conflicting with the render_graph borrow
        if let Some(mut node_box) = state.renderer.render_graph.take_node(EGUI_NODE_INDEX) {
            if let Some(egui_node) = node_box.as_any_mut().downcast_mut::<EguiRenderNode>() {
                egui_node.process(state);
            }
            state.renderer.render_graph.return_node(EGUI_NODE_INDEX, node_box);
        }
    })
    .with_on_key(|state, code, pressed| {
        if code == engine::KeyCode::KeyY && pressed {
            #[cfg(feature = "hot-reload")]
            {
                std::process::Command::new("cargo")
                    .args(["build", "-p", "game-logic"])
                    .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
                    .spawn()
                    .ok();

                std::process::Command::new("cargo")
                    .args(["build", "-p", "editor-logic"])
                    .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
                    .spawn()
                    .ok();
            }
        }
        #[cfg(feature = "hot-reload")]
        game::handle_key_press(state, code, pressed);

        #[cfg(not(feature = "hot-reload"))]
        game_logic::handle_key_press(state, code, pressed);
    });

    event_loop.run_app(&mut app)?;

    Ok(())
}
