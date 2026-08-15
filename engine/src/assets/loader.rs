// NOTE: Loader uses engine formats and loads them into the GPU/CPU

use crate::assets::importer::AssetPayload;
use crate::assets::manager::AssetContext;
use crate::assets::manager::AssetHeader;
use crate::assets::manager::AssetManager;
use crate::assets::manager::AssetType;
use crate::assets::serializer::{BINARY_DELIMITER, MAGIC};
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
