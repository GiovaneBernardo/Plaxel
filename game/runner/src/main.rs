#[cfg(feature = "hot-reload")]
#[hot_lib_reloader::hot_module(
    dylib = "game_logic",
    lib_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/debug")
)]
mod game {
    hot_functions_from_file!("game/logic/src/lib.rs");
}

fn main() {
    engine::logging::init();

    let event_loop = winit::event_loop::EventLoop::with_user_event()
        .build()
        .unwrap();
    let mut app = engine::App::new()
        .with_register_system(|state| {
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
        })
        .with_on_key(|state, code, pressed| {
            if code == engine::KeyCode::KeyY && pressed {
                #[cfg(feature = "hot-reload")]
                {
                    std::process::Command::new("cargo")
                        .args(["build", "-p", "game-logic"])
                        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
                        .spawn()
                        .ok();
                }
            }

            #[cfg(feature = "hot-reload")]
            game::handle_key_press(state, code, pressed);

            #[cfg(not(feature = "hot-reload"))]
            game_logic::handle_key_press(state, code, pressed);
        });

    event_loop.run_app(&mut app).unwrap();
}
