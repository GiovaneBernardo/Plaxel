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
    hot_functions_from_file!("editor/logic/src/lib.rs");
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_main() {
    run_editor().unwrap();
}

pub fn run_editor() -> anyhow::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    engine::logging::init();

    let event_loop = winit::event_loop::EventLoop::with_user_event().build()?;
    let mut app = engine::App::new(
        #[cfg(target_arch = "wasm32")]
        &event_loop,
    )
    .with_register_system(|state| {
        #[cfg(feature = "hot-reload")]
        editor_hot::register_editor(state);
        #[cfg(not(feature = "hot-reload"))]
        editor_logic::register_editor(state);

        #[cfg(feature = "hot-reload")]
        game::register_systems(state);
        #[cfg(not(feature = "hot-reload"))]
        game_logic::register_systems(state);
    })
    .with_update(|state| {
        #[cfg(feature = "hot-reload")]
        game::update(state);
        #[cfg(not(feature = "hot-reload"))]
        game_logic::update(state);

        #[cfg(feature = "hot-reload")]
        editor_hot::update_editor(state);
        #[cfg(not(feature = "hot-reload"))]
        editor_logic::update_editor(state);
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
