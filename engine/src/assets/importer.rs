// NOTE: Importer converts outside formats into the engine formats
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::{
    assets::{
        manager::{AssetHeader, AssetManager},
        material::{Material, TextureAsset},
    },
    model::MeshAsset,
};

pub trait AssetImporter {
    fn id(&self) -> &'static str;
    fn version(&self) -> u32;
    fn extensions(&self) -> &[&'static str];
    fn import(&self, source: &Path, ctx: &ImportContext) -> anyhow::Result<Vec<ImportedAsset>>;
}

pub struct ImportContext<'a> {
    pub project_root: &'a Path,
    pub source_root: &'a Path,
    pub asset_root: &'a Path,

    pub source_path: &'a Path,
    //pub source_hash: [u8; 32],
    pub manager: &'a AssetManager,
    pub settings: &'a ImportSettings,
}

pub struct ImportSettings {
    pub force_reimport: bool,
    pub generate_mipmaps: bool,
    pub ignored_platform: TargetPlatform,
}

pub enum TargetPlatform {
    Desktop,
    Wasm,
    None,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ImportedAsset {
    pub header: AssetHeader,
    pub payload: AssetPayload,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub enum AssetPayload {
    Mesh(MeshAsset),
    Material(Material),
    Texture(TextureAsset),
    Custom { type_name: String, bytes: Vec<u8> },
}

impl<'a> ImportContext<'a> {
    pub fn relative_source_path(&self, path: &Path) -> anyhow::Result<String> {
        Ok(path
            .strip_prefix(self.source_root)?
            .to_string_lossy()
            .replace('\\', "/"))
    }

    pub fn stable_asset_path(
        &self,
        source_path: &Path,
        sub_asset: Option<&str>,
    ) -> anyhow::Result<String> {
        let mut path = self.relative_source_path(source_path)?;
        if let Some(sub) = sub_asset {
            path.push('#');
            path.push_str(sub);
        }
        Ok(path)
    }

    pub fn asset_uuid(&self, stable_path: &PathBuf) -> Option<&Uuid> {
        self.manager.uuid_for_path(stable_path)
    }
}
