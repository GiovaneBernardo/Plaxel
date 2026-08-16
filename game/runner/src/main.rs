use static_game_logic::GamePlugin;

fn main() {
    #[cfg(not(target_family = "wasm"))]
    dioxus_devtools::connect_subsecond();

    engine::logging::init();

    let mut app = engine::App::new();
    app.add_plugin(engine::PlaxelDefaultPlugin)
        .add_plugin(GamePlugin);
    app.run();
    // .with_register_system(|state| {
    //     #[cfg(feature = "hot-reload")]
    //     register_hot_game_systems(state);

    //     #[cfg(not(feature = "hot-reload"))]
    //     game_logic::register_systems(state);
    // })
    // .with_update(|state| {
    //     #[cfg(feature = "hot-reload")]
    //     game::update(state);

    //     #[cfg(not(feature = "hot-reload"))]
    //     game_logic::update(state);
    // })
    // .with_on_key(|state, code, pressed| {
    //     if code == KeyCode::KeyY && pressed {
    //         #[cfg(feature = "hot-reload")]
    //         {
    //             let mut features = String::from("game-runner/hot-reload");
    //             features.push_str(",game-logic/dynamic_linking");
    //             #[cfg(feature = "profiling")]
    //             features.push_str(",game-runner/profiling");
    //             #[cfg(feature = "profiling")]
    //             features.push_str(",game-logic/profiling");
    //             #[cfg(feature = "profiling")]
    //             features.push_str(",game-logic/puffin");
    //             #[cfg(feature = "tracy")]
    //             features.push_str(",game-runner/tracy");
    //             #[cfg(feature = "tracy")]
    //             features.push_str(",game-logic/tracy");
    //             #[cfg(feature = "renderdoc")]
    //             features.push_str(",game-runner/renderdoc");
    //             #[cfg(feature = "renderdoc")]
    //             features.push_str(",game-logic/renderdoc");

    //             let args = [
    //                 "build",
    //                 "-p",
    //                 "game-runner",
    //                 "-p",
    //                 "game-logic",
    //                 "--features",
    //                 &features,
    //             ];
    //             log::info!("hot reload build starting: cargo {}", args.join(" "));

    //             match std::process::Command::new("cargo")
    //                 .args(args)
    //                 .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
    //                 .spawn()
    //             {
    //                 Ok(mut child) => {
    //                     std::thread::spawn(move || match child.wait() {
    //                         Ok(status) => log::info!("hot reload build finished: {status}"),
    //                         Err(error) => log::warn!("hot reload build wait failed: {error}"),
    //                     });
    //                 }
    //                 Err(error) => log::warn!("hot reload build spawn failed: {error}"),
    //             }
    //         }
    //     }

    //     #[cfg(feature = "hot-reload")]
    //     game::handle_key_press(state, code, pressed);

    //     #[cfg(not(feature = "hot-reload"))]
    //     game_logic::handle_key_press(state, code, pressed);
    // })
    // .with_on_resize(|state, width, height| {
    //     #[cfg(feature = "hot-reload")]
    //     game::handle_resize(state, width, height);

    //     #[cfg(not(feature = "hot-reload"))]
    //     game_logic::handle_resize(state, width, height);
    // })
    // .with_on_mouse_button(|state, button, pressed| {
    //     #[cfg(feature = "hot-reload")]
    //     game::handle_mouse_button(state, button, pressed);

    //     #[cfg(not(feature = "hot-reload"))]
    //     game_logic::handle_mouse_button(state, button, pressed);
    // })
    // .with_on_mouse_motion(|state, dx, dy| {
    //     #[cfg(feature = "hot-reload")]
    //     game::handle_mouse_motion(state, dx, dy);

    //     #[cfg(not(feature = "hot-reload"))]
    //     game_logic::handle_mouse_motion(state, dx, dy);
    // })
    // .with_on_mouse_scroll(|state, delta| {
    //     #[cfg(feature = "hot-reload")]
    //     game::handle_mouse_scroll(state, delta);

    //     #[cfg(not(feature = "hot-reload"))]
    //     game_logic::handle_mouse_scroll(state, delta);
    // });
    //
    // event_loop.run_app(&mut app).unwrap();
}
