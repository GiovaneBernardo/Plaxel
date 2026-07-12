fn main() {
    #[cfg(not(target_family = "wasm"))]
    dioxus_devtools::connect_subsecond();

    if let Err(error) = editor_runner::run_editor() {
        eprintln!("editor failed: {error:?}");
        std::process::exit(1);
    }
}
