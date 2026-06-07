// NOTE: Serializer gets the engine formats and turn them into files, it should'nt be used for outside formats like .fbx etc
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::assets::importer::{AssetPayload, ImportedAsset};
use crate::assets::manager::AssetHeader;

pub const MAGIC: &[u8] = b"PLAXEL_ASSET 1\n";
pub const BINARY_DELIMITER: &[u8] = b"\n---PLAXEL_BINARY---\n";

pub fn asset_extension(payload: &AssetPayload) -> &'static str {
    match payload {
        AssetPayload::Mesh(_) => "plxmesh",
        AssetPayload::Material(_) => "plxmat",
        AssetPayload::Texture(_) => "plxtex",
        AssetPayload::Custom { .. } => "plax",
    }
}

pub fn output_path_for(asset: &ImportedAsset, output_dir: &Path) -> PathBuf {
    let file_name = sanitize_file_name(&asset.header.name);
    output_dir
        .join(file_name)
        .with_extension(asset_extension(&asset.payload))
}

pub fn write_imported_asset(asset: &ImportedAsset, output_path: &Path) -> anyhow::Result<()> {
    let payload = bincode::serialize(&asset.payload)?;
    let header = AssetHeader {
        file_path: output_path.to_path_buf(),
        content_offset: 0,
        content_size: payload.len() as u64,
        ..asset.header.clone()
    };
    let header_text = ron::ser::to_string_pretty(
        &header,
        ron::ser::PrettyConfig::default().struct_names(true),
    )?;

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut file = File::create(output_path)?;
    file.write_all(MAGIC)?;
    file.write_all(header_text.as_bytes())?;
    file.write_all(BINARY_DELIMITER)?;
    file.write_all(&payload)?;

    Ok(())
}

fn sanitize_file_name(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect::<String>();

    if sanitized.is_empty() {
        "asset".to_string()
    } else {
        sanitized
    }
}
