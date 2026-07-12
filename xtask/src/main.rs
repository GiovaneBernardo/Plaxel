use std::{
    process::{Command, Stdio, exit},
    thread,
    time::Duration,
};

fn main() {
    let task = std::env::args().nth(1);
    match task.as_deref() {
        Some("play") => play(false),
        Some("play-renderdoc") => play(true),
        Some("play-editor") => editor(),
        Some("play-wasm") => wasm().unwrap(),
        _ => {
            eprintln!("usage: cargo xtask <play|play-renderdoc|play-editor|play-wasm>");
            exit(2);
        }
    }
}

fn play(renderdoc: bool) {
    let mut args = vec!["serve", "--hotpatch", "--package", "game-runner"];
    if renderdoc {
        args.extend(["--features", "game-runner/renderdoc"]);
    }
    run_dx(&args);
}

fn editor() {
    run_dx(&["serve", "--hotpatch", "--package", "editor-runner"]);
}

fn run_dx(args: &[&str]) {
    let status = Command::new("dx").args(args).status().expect("spawn dx");
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}

pub fn wasm() -> std::io::Result<()> {
    let status = Command::new("wasm-pack")
        .args([
            "build",
            "editor/runner",
            "--target",
            "web",
            "--out-dir",
            "../../pkg",
            "--dev",
            "--no-default-features",
            "--features",
            "profiling",
        ])
        .status()?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    let mut server = Command::new("python")
        .args(["-m", "http.server", "8000"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;

    thread::sleep(Duration::from_millis(800));

    open_browser("http://localhost:8000")?;

    let status = server.wait()?;
    std::process::exit(status.code().unwrap_or(0));
}

fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    Command::new("cmd").args(["/C", "start", url]).spawn()?;

    #[cfg(target_os = "macos")]
    Command::new("open").arg(url).spawn()?;

    #[cfg(target_os = "linux")]
    Command::new("xdg-open").arg(url).spawn()?;

    Ok(())
}
