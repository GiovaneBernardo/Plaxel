// NOTE: Serializer gets the engine formats and turn them into files, it should'nt be used for outside formats like .fbx etc
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::assets::importer::ImportedAsset;
use crate::assets::manager::AssetHeader;

pub const MAGIC: &[u8] = b"PLAXEL_ASSET 1\n";
pub const BINARY_DELIMITER: &[u8] = b"\n---PLAXEL_BINARY---\n";

pub fn output_path_for(asset: &ImportedAsset, output_dir: &Path) -> PathBuf {
    let file_name = sanitize_file_name(&asset.header.name);
    output_dir.join(file_name).with_extension(&asset.extension)
}

pub fn write_imported_asset(asset: &ImportedAsset, output_path: &Path) -> anyhow::Result<()> {
    let header = AssetHeader {
        file_path: output_path.to_path_buf(),
        content_offset: 0,
        content_size: asset.payload.len() as u64,
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
    file.write_all(&asset.payload)?;

    Ok(())
}

pub fn write_asset<T: serde::Serialize>(
    header: AssetHeader,
    type_name: impl Into<String>,
    extension: impl Into<String>,
    asset: &T,
    output_path: &Path,
) -> anyhow::Result<()> {
    let imported = ImportedAsset::from_asset(header, type_name, extension, asset)?;
    write_imported_asset(&imported, output_path)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{loader, manager::AssetType};

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct CustomAsset {
        value: u32,
    }

    #[test]
    fn version_two_assets_round_trip_without_the_legacy_payload_enum() {
        let id = uuid::Uuid::new_v4();
        let path = std::env::temp_dir().join(format!("plaxel-{id}.plax"));
        let header = AssetHeader {
            version: 0,
            uuid: id,
            name: "custom".into(),
            asset_type: AssetType::Custom,
            type_name: None,
            file_path: PathBuf::new(),
            content_offset: 0,
            content_size: 0,
        };

        write_asset(
            header,
            "example.custom_asset",
            "plax",
            &CustomAsset { value: 42 },
            &path,
        )
        .unwrap();

        let stored_header = loader::load_header(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let stored: CustomAsset = loader::deserialize_payload(&bytes).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(stored_header.version, 2);
        assert_eq!(
            stored_header.type_name.as_deref(),
            Some("example.custom_asset")
        );
        assert_eq!(stored, CustomAsset { value: 42 });
    }
}
