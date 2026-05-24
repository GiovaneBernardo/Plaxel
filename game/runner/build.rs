//! Copies `std-*.dll` from the Rust sysroot next to the built binaries.
//!
//! Any `dylib` crate in the graph flips rustc into linking libstd dynamically
//! as `std-<hash>.dll`, which lives in the toolchain's `bin/` directory.
//! `cargo run` adds that directory to PATH, but launching the exe directly
//! (Explorer, RenderDoc, an attached debugger, ...) won't — so Windows fails
//! to resolve the DLL. Copying it beside the exe makes the output self-
//! contained for dev builds.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_DYNAMIC_LINKING");
    println!("cargo:rerun-if-env-changed=RUSTUP_TOOLCHAIN");

    if std::env::var_os("CARGO_FEATURE_DYNAMIC_LINKING").is_none() {
        return;
    }

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

    // OUT_DIR is `<target>/<profile>/build/<crate>-<hash>/out`; climb 3 to
    // get to the profile directory where cargo drops final artifacts.
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR missing"));
    let target_profile_dir = out_dir
        .ancestors()
        .nth(3)
        .expect("unexpected OUT_DIR depth")
        .to_path_buf();

    for entry in std::fs::read_dir(&bin_dir)
        .expect("read sysroot bin dir")
        .flatten()
    {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let is_std_artifact = name_str.starts_with("std-")
            && (name_str.ends_with(".dll") || name_str.ends_with(".pdb"));
        if !is_std_artifact {
            continue;
        }
        let src = entry.path();
        let dest = target_profile_dir.join(&*name_str);
        std::fs::copy(&src, &dest).expect("copy std artifact");
        println!("cargo:rerun-if-changed={}", src.display());
    }
}
