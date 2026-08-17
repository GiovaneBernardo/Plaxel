// NOTE: Loader uses engine formats and loads them into the GPU/CPU

use crate::assets::importer::LegacyAssetPayload;
use crate::assets::manager::AssetHeader;
use crate::assets::manager::AssetType;
use crate::assets::material::{Material, MaterialResource, TextureAsset};
use crate::assets::serializer::{BINARY_DELIMITER, MAGIC};
use crate::assets::server::{AssetLoader, LoadContext};
use crate::model::MeshAsset;
use std::io::{ErrorKind, Read};
use std::path::Path;

pub fn load_header(path: &Path) -> anyhow::Result<AssetHeader> {
    let mut file = std::fs::File::open(path)?;
    let bytes = read_through_header(&mut file)?;

    let (mut header, content_offset) = parse_text_header(&bytes)?;
    let file_len = file.metadata()?.len();
    header.file_path = path.to_path_buf();
    header.content_offset = content_offset as u32;
    header.content_size = file_len.saturating_sub(content_offset as u64);

    Ok(header)
}

fn read_through_header(reader: &mut impl Read) -> anyhow::Result<Vec<u8>> {
    let mut bytes = vec![0; MAGIC.len()];
    match reader.read_exact(&mut bytes) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::UnexpectedEof => {
            anyhow::bail!("invalid asset file magic");
        }
        Err(error) => return Err(error.into()),
    }

    if bytes.as_slice() != MAGIC {
        anyhow::bail!("invalid asset file magic");
    }

    let mut chunk = [0; 4096];
    loop {
        let previous_len = bytes.len();
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            anyhow::bail!("asset file is missing binary delimiter");
        }

        bytes.extend_from_slice(&chunk[..read]);

        // Preserve enough overlap to find a delimiter split across two reads,
        // while avoiding another scan of the entire accumulated header.
        let search_start = previous_len.saturating_sub(BINARY_DELIMITER.len() - 1);
        if find_bytes(&bytes[search_start..], BINARY_DELIMITER).is_some() {
            return Ok(bytes);
        }
    }
}

fn load_legacy_payload_bytes(bytes: &[u8]) -> anyhow::Result<LegacyAssetPayload> {
    let (_, content_offset) = parse_text_header(&bytes)?;
    Ok(bincode::deserialize(&bytes[content_offset..])?)
}

/// Deserializes the opaque payload used by version-2 cooked assets. This is
/// the helper custom loaders normally use after registering their extension.
pub fn deserialize_payload<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> anyhow::Result<T> {
    let (header, content_offset) = parse_text_header(bytes)?;
    if header.version < 2 {
        anyhow::bail!("typed payload decoding requires cooked asset version 2 or newer");
    }
    Ok(bincode::deserialize(&bytes[content_offset..])?)
}

fn decode_versioned<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
    legacy: impl FnOnce(LegacyAssetPayload) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let (header, _) = parse_text_header(bytes)?;
    if header.version >= 2 {
        deserialize_payload(bytes)
    } else {
        legacy(load_legacy_payload_bytes(bytes)?)
    }
}

pub fn load_material_payload(path: &Path) -> anyhow::Result<Material> {
    let bytes = std::fs::read(path)?;
    decode_versioned(&bytes, |payload| match payload {
        LegacyAssetPayload::Material(asset) => Ok(asset),
        _ => anyhow::bail!("cooked file does not contain a material"),
    })
}

fn parse_text_header(bytes: &[u8]) -> anyhow::Result<(AssetHeader, usize)> {
    if !bytes.starts_with(MAGIC) {
        anyhow::bail!("invalid asset file magic");
    }

    let header_start = MAGIC.len();
    let delimiter_start = find_bytes(&bytes[header_start..], BINARY_DELIMITER)
        .map(|index| header_start + index)
        .ok_or_else(|| anyhow::anyhow!("asset file is missing binary delimiter"))?;
    let content_offset = delimiter_start + BINARY_DELIMITER.len();
    let header_text = std::str::from_utf8(&bytes[header_start..delimiter_start])?;
    let header = ron::de::from_str::<AssetHeader>(header_text)?;

    Ok((header, content_offset))
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

pub struct CookedMeshLoader;

impl AssetLoader for CookedMeshLoader {
    type Asset = MeshAsset;

    fn extensions(&self) -> &[&'static str] {
        &["plxmesh"]
    }

    fn legacy_asset_type(&self) -> Option<AssetType> {
        Some(AssetType::Mesh)
    }

    fn load(&self, bytes: &[u8], context: &mut LoadContext) -> anyhow::Result<MeshAsset> {
        let asset = decode_versioned(bytes, |payload| match payload {
            LegacyAssetPayload::Mesh(asset) => Ok(asset),
            _ => anyhow::bail!("cooked file does not contain a mesh"),
        })?;
        {
            if let Some(material) = asset.material_uuid {
                context.load_dependency_by_id::<Material>(material);
            }
            Ok(asset)
        }
    }
}

pub struct CookedMaterialLoader;

impl AssetLoader for CookedMaterialLoader {
    type Asset = Material;

    fn extensions(&self) -> &[&'static str] {
        &["plxmat"]
    }

    fn legacy_asset_type(&self) -> Option<AssetType> {
        Some(AssetType::Material)
    }

    fn load(&self, bytes: &[u8], context: &mut LoadContext) -> anyhow::Result<Material> {
        let asset = decode_versioned(bytes, |payload| match payload {
            LegacyAssetPayload::Material(asset) => Ok(asset),
            _ => anyhow::bail!("cooked file does not contain a material"),
        })?;
        {
            for binding in &asset.bindings {
                match &binding.resource {
                    MaterialResource::Texture(id) => {
                        context.load_dependency_by_id::<TextureAsset>(*id);
                    }
                    MaterialResource::TextureArray(ids) => {
                        for id in ids {
                            context.load_dependency_by_id::<TextureAsset>(*id);
                        }
                    }
                    MaterialResource::Sampler(_) | MaterialResource::Buffer(_) => {}
                }
            }
            Ok(asset)
        }
    }
}

pub struct CookedTextureLoader;

impl AssetLoader for CookedTextureLoader {
    type Asset = TextureAsset;

    fn extensions(&self) -> &[&'static str] {
        &["plxtex"]
    }

    fn legacy_asset_type(&self) -> Option<AssetType> {
        Some(AssetType::Texture)
    }

    fn load(&self, bytes: &[u8], _context: &mut LoadContext) -> anyhow::Result<TextureAsset> {
        decode_versioned(bytes, |payload| match payload {
            LegacyAssetPayload::Texture(asset) => Ok(asset),
            _ => anyhow::bail!("cooked file does not contain a texture"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn rejects_non_assets_after_reading_only_the_magic_prefix() {
        let mut reader = Cursor::new(vec![b'x'; 64 * 1024]);

        let error = read_through_header(&mut reader).unwrap_err();

        assert_eq!(error.to_string(), "invalid asset file magic");
        assert_eq!(reader.position(), MAGIC.len() as u64);
    }

    #[test]
    fn finds_a_delimiter_split_across_read_chunks() {
        let delimiter_start = 4095;
        let mut input = MAGIC.to_vec();
        input.resize(delimiter_start, b'x');
        input.extend_from_slice(BINARY_DELIMITER);
        input.extend_from_slice(b"payload");
        let mut reader = Cursor::new(input);

        let bytes = read_through_header(&mut reader).unwrap();

        assert_eq!(find_bytes(&bytes, BINARY_DELIMITER), Some(delimiter_start));
    }
}
