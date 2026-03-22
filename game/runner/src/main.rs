#[cfg(feature = "hot-reload")]
#[hot_lib_reloader::hot_module(
    dylib = "game_logic",
    lib_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/debug")
)]
mod game {
    hot_functions_from_file!("game/logic/src/lib.rs");
}

fn main() {
    env_logger::init();

    let event_loop = winit::event_loop::EventLoop::with_user_event()
        .build()
        .unwrap();
    let mut app = engine::App::new()
        .with_update(|state| {
            #[cfg(feature = "hot-reload")]
            game::update(state);

            #[cfg(not(feature = "hot-reload"))]
            game_logic::update(state);
        })
        .with_on_key(|code, pressed| {
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
        });

    event_loop.run_app(&mut app).unwrap();
}
