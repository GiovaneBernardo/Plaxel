use std::process::{Command, exit};

fn main() {
    let task = std::env::args().nth(1);
    match task.as_deref() {
        Some("play") => play(false),
        Some("play-renderdoc") => play(true),
        Some("play-editor") => editor(),
        _ => {
            eprintln!("usage: cargo xtask <play|play-renderdoc>");
            exit(2);
        }
    }
}

fn play(renderdoc: bool) {
    let mut features = String::from("game-runner/hot-reload");
    if renderdoc {
        features.push_str(",game-runner/renderdoc");
    }

    // Build both packages so game-logic's cdylib is produced/refreshed.
    run(&[
        "build",
        "-p",
        "game-runner",
        "-p",
        "game-logic",
        "--features",
        &features,
    ]);

    run(&["run", "-p", "game-runner", "--features", &features]);
}

fn editor() {
    let features = String::from("editor-runner/hot-reload");
    //features.push_str(",editor-runner/renderdoc");

    run(&[
        "build",
        "-p",
        "editor-runner",
        "-p",
        "game-logic",
        "-p",
        "editor-logic",
        "--features",
        &features,
    ]);

    run(&["run", "-p", "editor-runner", "--features", &features]);
}

fn run(args: &[&str]) {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .args(args)
        .status()
        .expect("spawn cargo");
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}
