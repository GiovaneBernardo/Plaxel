use anyhow::Ok;
pub mod editor;

#[hot_lib_reloader::hot_module(
    dylib = "game_logic",
    lib_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../target/debug")
)]
mod game {
    hot_functions_from_file!("game/logic/src/lib.rs");
}

fn main() {
    run_editor().unwrap();
}

pub fn run_editor() -> anyhow::Result<()> {
    let mut editor_state = editor::EditorState {
        egui_context: egui::Context::default(),
    };

    env_logger::init();

    let event_loop = winit::event_loop::EventLoop::with_user_event().build()?;
    let mut app = engine::App::new()
        .with_update(move |game_state| {
            game::update(game_state);
            editor::update(game_state, &mut editor_state);
        })
        .with_on_key(|code, pressed| {
            if code == engine::KeyCode::KeyY && pressed {
                std::process::Command::new("cargo")
                    .args(["build", "-p", "game-logic"])
                    .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
                    .spawn()
                    .ok();
            }
        });

    event_loop.run_app(&mut app)?;

    Ok(())
}
