use engine::core::input::KeyCode;

#[cfg(all(not(feature = "hot-reload"), not(feature = "static-game-logic")))]
compile_error!("game-runner without hot-reload requires the static-game-logic feature");

#[cfg(all(not(feature = "hot-reload"), feature = "static-game-logic"))]
use static_game_logic as game_logic;

#[cfg(feature = "hot-reload")]
#[hot_lib_reloader::hot_module(
    dylib = "game_logic",
    lib_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/debug")
)]
mod game {
    use engine::core::input::KeyCode;
    hot_functions_from_file!("game/logic/src/lib.rs");
}

#[cfg(feature = "hot-reload")]
fn register_hot_game_systems(state: &mut engine::State) {
    game::initialize_game_state(state);

    let Some(scene) = state.active_scene_mut() else {
        return;
    };

    scene
        .init_schedule_mut()
        .add_named_system("game.planet_init", game::hot_planet_system_init);

    let schedule = scene.update_schedule_mut();
    schedule.add_named_system("game.planet_update", game::hot_planet_system_update);
    schedule.add_named_system(
        "game.create_missing_rapier_bodies",
        engine::core::physics::physics::Physics::create_missing_rapier_bodies_system,
    );
    schedule.add_static_named_system(
        "game.player_interaction",
        game::hot_player_interaction_system,
    );
    schedule.add_named_system("game.camera_update", game::hot_camera_update_system);
    schedule.add_named_system(
        "game.engine_input",
        engine::core::systems::systems::engine_input_system,
    );
}

fn main() {
    #[cfg(not(target_family = "wasm"))]
    dioxus_devtools::connect_subsecond();

    engine::logging::init();

    let event_loop = winit::event_loop::EventLoop::with_user_event()
        .build()
        .unwrap();
    let mut app = engine::App::new()
        .with_register_system(|state| {
            #[cfg(feature = "hot-reload")]
            register_hot_game_systems(state);

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
            if code == KeyCode::KeyY && pressed {
                #[cfg(feature = "hot-reload")]
                {
                    let mut features = String::from("game-runner/hot-reload");
                    features.push_str(",game-logic/dynamic_linking");
                    #[cfg(feature = "profiling")]
                    features.push_str(",game-runner/profiling");
                    #[cfg(feature = "profiling")]
                    features.push_str(",game-logic/profiling");
                    #[cfg(feature = "profiling")]
                    features.push_str(",game-logic/puffin");
                    #[cfg(feature = "tracy")]
                    features.push_str(",game-runner/tracy");
                    #[cfg(feature = "tracy")]
                    features.push_str(",game-logic/tracy");
                    #[cfg(feature = "renderdoc")]
                    features.push_str(",game-runner/renderdoc");
                    #[cfg(feature = "renderdoc")]
                    features.push_str(",game-logic/renderdoc");

                    let args = [
                        "build",
                        "-p",
                        "game-runner",
                        "-p",
                        "game-logic",
                        "--features",
                        &features,
                    ];
                    log::info!("hot reload build starting: cargo {}", args.join(" "));

                    match std::process::Command::new("cargo")
                        .args(args)
                        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
                        .spawn()
                    {
                        Ok(mut child) => {
                            std::thread::spawn(move || match child.wait() {
                                Ok(status) => log::info!("hot reload build finished: {status}"),
                                Err(error) => log::warn!("hot reload build wait failed: {error}"),
                            });
                        }
                        Err(error) => log::warn!("hot reload build spawn failed: {error}"),
                    }
                }
            }

            #[cfg(feature = "hot-reload")]
            game::handle_key_press(state, code, pressed);

            #[cfg(not(feature = "hot-reload"))]
            game_logic::handle_key_press(state, code, pressed);
        })
        .with_on_resize(|state, width, height| {
            #[cfg(feature = "hot-reload")]
            game::handle_resize(state, width, height);

            #[cfg(not(feature = "hot-reload"))]
            game_logic::handle_resize(state, width, height);
        })
        .with_on_mouse_button(|state, button, pressed| {
            #[cfg(feature = "hot-reload")]
            game::handle_mouse_button(state, button, pressed);

            #[cfg(not(feature = "hot-reload"))]
            game_logic::handle_mouse_button(state, button, pressed);
        })
        .with_on_mouse_motion(|state, dx, dy| {
            #[cfg(feature = "hot-reload")]
            game::handle_mouse_motion(state, dx, dy);

            #[cfg(not(feature = "hot-reload"))]
            game_logic::handle_mouse_motion(state, dx, dy);
        })
        .with_on_mouse_scroll(|state, delta| {
            #[cfg(feature = "hot-reload")]
            game::handle_mouse_scroll(state, delta);

            #[cfg(not(feature = "hot-reload"))]
            game_logic::handle_mouse_scroll(state, delta);
        });

    event_loop.run_app(&mut app).unwrap();
}
