// NOTE: Loader uses engine formats and loads them into the GPU/CPU

use crate::assets::importer::AssetPayload;
use crate::assets::manager::AssetContext;
use crate::assets::manager::AssetHeader;
use crate::assets::manager::AssetManager;
use crate::assets::manager::AssetType;
use crate::assets::serializer::{BINARY_DELIMITER, MAGIC};
use std::io::Read;
use std::path::Path;

pub fn load_header(path: &Path) -> anyhow::Result<AssetHeader> {
    let mut file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    let mut chunk = [0; 4096];

    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            break;
        }

        bytes.extend_from_slice(&chunk[..read]);
        if find_bytes(&bytes, BINARY_DELIMITER).is_some() {
            break;
        }
    }

    let (mut header, content_offset) = parse_text_header(&bytes)?;
    let file_len = file.metadata()?.len();
    header.file_path = path.to_path_buf();
    header.content_offset = content_offset as u32;
    header.content_size = file_len.saturating_sub(content_offset as u64);

    Ok(header)
}

pub fn load_payload(path: &Path) -> anyhow::Result<AssetPayload> {
    let bytes = std::fs::read(path)?;
    let (_, content_offset) = parse_text_header(&bytes)?;
    Ok(bincode::deserialize(&bytes[content_offset..])?)
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

pub fn load_asset(_asset_manager: &mut AssetManager, _ctx: &AssetContext, _header: &AssetHeader) {
    //let asset = match header.asset_type {
    //    _ => {
    //        panic!("Unsupported asset type: {:?}", header.asset_type);
    //    }
    //};
    //asset_manager.assets.insert(header.uuid, asset);
}

trait AssetLoader {
    type Asset;

    fn asset_type(&self) -> AssetType;
    fn load(&self, header: &AssetHeader, payload: &[u8]) -> anyhow::Result<Self::Asset>;
}

//trait AssetReader {
//    fn read_header(&self, id: AssetId) -> anyhow::Result<AssetHeader>;
//    fn read_payload(&self, id: AssetId) -> anyhow::Result<Vec<u8>>;
//}
