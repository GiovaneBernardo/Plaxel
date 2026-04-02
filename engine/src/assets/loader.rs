use crate::assets::manager::AssetContext;
use crate::assets::manager::AssetHeader;
use crate::assets::manager::AssetManager;
use crate::assets::manager::AssetType;
use crate::assets::material::Material;
use std::fs::File;
use std::path::Path;
use uuid::Uuid;

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

pub fn load_asset(asset_manager: &mut AssetManager, ctx: &AssetContext, header: &AssetHeader) {
    let asset = match header.asset_type {
        //AssetType::Material => Box::new(Material::load(header.uuid, ctx)),
        _ => {
            panic!("Unsupported asset type: {:?}", header.asset_type);
        } //AssetType::Material => Box::new(Material::load(header.uuid)),
          //AssetType::Texture => Box::new(Texture::load(header.uuid)),
          //AssetType::Mesh => Box::new(Mesh::load(header.uuid)),
          //AssetType::Prefab => Box::new(Prefab::load(header.uuid)),
          //AssetType::Audio => Box::new(Audio::load(header.uuid)),
    };
    asset_manager.assets.insert(header.uuid, asset);
}
