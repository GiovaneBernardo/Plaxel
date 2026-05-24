// NOTE: Loader uses engine formats and loads them into the GPU/CPU

use crate::assets::manager::AssetContext;
use crate::assets::manager::AssetHeader;
use crate::assets::manager::AssetManager;
use std::fs::File;
use std::path::Path;

pub fn load_header(path: &Path) -> anyhow::Result<AssetHeader> {
    let mut file = File::open(path)?;
    let header: AssetHeader = bincode::deserialize_from(&mut file)?;
    Ok(AssetHeader {
        version: header.version,
        uuid: header.uuid,
        name: header.name,
        asset_type: header.asset_type,
        file_path: path.to_owned().to_string_lossy().to_string(),
        content_offset: header.content_offset,
        content_size: header.content_size,
    })
}

pub fn load_asset(_asset_manager: &mut AssetManager, _ctx: &AssetContext, _header: &AssetHeader) {
    //let asset = match header.asset_type {
    //    _ => {
    //        panic!("Unsupported asset type: {:?}", header.asset_type);
    //    }
    //};
    //asset_manager.assets.insert(header.uuid, asset);
}
