use anyhow::*;
use fs_extra::copy_items;
use fs_extra::dir::CopyOptions;
use std::env;
use std::path::Path;

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=../res");

    let out_dir = env::var("OUT_DIR")?;
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    let res_dir = Path::new(&manifest_dir).join("../res");

    let mut copy_options = CopyOptions::new();
    copy_options.overwrite = true;
    copy_items(&[&res_dir], &out_dir, &copy_options)?;

    let out_dir = env::var("OUT_DIR")?;

    // OUT_DIR is something like target/debug/build/<crate>/out
    // We want target/debug/ (3 levels up)
    let target_dir = Path::new(&out_dir)
        .ancestors()
        .nth(3)
        .expect("OUT_DIR doesn't have enough ancestors");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    let res_dir = Path::new(&manifest_dir).join("../res");

    let mut copy_options = CopyOptions::new();
    copy_options.overwrite = true;
    copy_items(&[&res_dir], target_dir, &copy_options)?;

    Ok(())
}
