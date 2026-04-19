#[cfg(feature = "dynamic_linking")]
#[allow(unused_imports)]
use engine_dylib;

#[cfg(feature = "hot-reload")]
#[hot_lib_reloader::hot_module(
    dylib = "game_logic",
    lib_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/debug")
)]
mod game {
    use engine::KeyCode;
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
                    // Build with the same package set + features as the
                    // launch command so feature unification resolves
                    // `engine` to the same metadata hash — otherwise the
                    // new `game_logic.dll` links against a different
                    // `engine_dylib.dll` than the running exe loaded, and
                    // `TypeId`s silently diverge. The game-runner relink
                    // will fail (exe is locked), which is fine — cargo
                    // still produces the updated game_logic cdylib.
                    let mut features = String::from("game-runner/hot-reload");
                    #[cfg(feature = "renderdoc")]
                    features.push_str(",game-runner/renderdoc");

                    std::process::Command::new("cargo")
                        .args([
                            "build",
                            "-p",
                            "game-runner",
                            "-p",
                            "game-logic",
                            "--features",
                            &features,
                        ])
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
