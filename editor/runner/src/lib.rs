#[cfg(all(not(feature = "hot-reload"), not(feature = "static-game-logic")))]
compile_error!("editor-runner without hot-reload requires the static-game-logic feature");

#[cfg(feature = "static-game-logic")]
use static_game_logic as game_logic;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_main() {
    run_editor().unwrap();
}

pub fn run_editor() -> anyhow::Result<()> {
    engine::logging::init();
    engine::profiling::init(true);
    log::info!("Editor profiling initialized");

    let mut app = engine::App::new();
    app.add_plugin(engine::PlaxelDefaultPlugin);

    #[cfg(feature = "static-game-logic")]
    app.add_plugin(game_logic::GamePlugin);

    app.add_plugin(editor_logic::EditorPlugin);
    app.run();

    Ok(())
}
