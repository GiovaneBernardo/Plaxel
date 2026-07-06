fn main() {
    if let Err(error) = editor_runner::run_editor() {
        eprintln!("editor failed: {error:?}");
        std::process::exit(1);
    }
}
