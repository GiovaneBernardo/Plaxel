use std::{
    env,
    path::PathBuf,
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
            eprintln!("usage: cargo xtask <play|play-renderdoc>");
            exit(2);
        }
    }
}

fn play(renderdoc: bool) {
    let mut runner_features =
        String::from("game-runner/hot-reload,game-runner/profiling,game-runner/tracy");
    if renderdoc {
        runner_features.push_str(",game-runner/renderdoc");
    }

    run(&[
        "build",
        "-p",
        "game-runner",
        "-p",
        "game-logic",
        "--features",
        &runner_features,
    ]);

    run_built_exe("game-runner");
}

fn editor() {
    let runner_features = String::from(
        "editor-runner/hot-reload,editor-runner/profiling,editor-runner/tracy,editor-runner/renderdoc",
    );
    run(&[
        "build",
        "-p",
        "editor-runner",
        "-p",
        "editor-logic",
        "-p",
        "game-logic",
        "--features",
        &runner_features,
    ]);

    run_built_exe("editor-runner-bin");
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

fn run_built_exe(name: &str) {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask should be inside the workspace")
        .to_path_buf();

    let mut exe = workspace_root.join("target");
    exe.push("debug");
    exe.push(format!("{name}{}", env::consts::EXE_SUFFIX));

    eprintln!("launching {}", exe.display());

    let mut command = Command::new(&exe);
    command.current_dir(&workspace_root);
    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());
    add_rust_sysroot_bin_to_path(&mut command);

    let status = command.status().expect("spawn built executable");
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}

fn add_rust_sysroot_bin_to_path(command: &mut Command) {
    let sysroot_out = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .expect("failed to invoke `rustc --print sysroot`");
    let sysroot = PathBuf::from(
        String::from_utf8(sysroot_out.stdout)
            .unwrap()
            .trim()
            .to_owned(),
    );
    let bin_dir = sysroot.join("bin");

    let current_path = env::var_os("PATH").unwrap_or_default();
    let mut paths = env::split_paths(&current_path).collect::<Vec<_>>();
    paths.insert(0, bin_dir);
    let path = env::join_paths(paths).expect("join PATH entries");
    command.env("PATH", path);
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
