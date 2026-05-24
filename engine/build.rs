use anyhow::*;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=../res");

    let out_dir = env::var("OUT_DIR")?;
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    let res_dir = Path::new(&manifest_dir).join("../res");

    generate_embedded_shaders(&res_dir, Path::new(&out_dir))?;

    Ok(())
}

fn generate_embedded_shaders(res_dir: &Path, out_dir: &Path) -> Result<()> {
    let shaders_dir = res_dir.join("shaders");
    let mut shaders = Vec::new();

    if shaders_dir.exists() {
        for entry in glob::glob(&format!("{}/**/*.wgsl", shaders_dir.display()))? {
            let path = entry?;
            let relative_path = path
                .strip_prefix(res_dir)?
                .to_string_lossy()
                .replace('\\', "/");

            shaders.push((relative_path, path));
        }
    }

    shaders.sort_by(|a, b| a.0.cmp(&b.0));

    let mut source = String::from(
        "pub fn embedded_shader_source(file_name: &str) -> Option<&'static str> {\n    match file_name {\n",
    );

    for (relative_path, path) in shaders {
        source.push_str("        ");
        source.push_str(&format!("{relative_path:?}"));
        source.push_str(" => Some(include_str!(");
        source.push_str(&format!("{:?}", normalize_path_for_include(&path)));
        source.push_str(")),\n");
    }

    source.push_str("        _ => None,\n    }\n}\n");
    fs::write(out_dir.join("embedded_shaders.rs"), source)?;
    Ok(())
}

fn normalize_path_for_include(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| PathBuf::from(path))
        .to_string_lossy()
        .replace('\\', "/")
}
