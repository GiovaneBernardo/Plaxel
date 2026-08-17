// NOTE: Importer converts outside formats into the engine formats
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use uuid::Uuid;

use crate::{
    assets::{
        manager::{AssetCatalog, AssetHeader},
        material::{Material, TextureAsset},
    },
    model::MeshAsset,
};

pub trait AssetImporter: Send + Sync + 'static {
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
    pub catalog: &'a AssetCatalog,
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
    pub extension: String,
    pub payload: Vec<u8>,
}

#[derive(Default)]
pub struct AssetImporterRegistry {
    importers: Vec<Arc<dyn AssetImporter>>,
}

impl AssetImporterRegistry {
    pub fn register<I: AssetImporter>(&mut self, importer: I) {
        let id = importer.id();
        self.importers.retain(|registered| registered.id() != id);
        self.importers.push(Arc::new(importer));
    }

    pub fn importer_for(&self, path: &Path) -> Option<Arc<dyn AssetImporter>> {
        let extension = path.extension()?.to_str()?;
        self.importers
            .iter()
            .find(|importer| {
                importer
                    .extensions()
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(extension))
            })
            .cloned()
    }

    pub fn len(&self) -> usize {
        self.importers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.importers.is_empty()
    }
}

impl ImportedAsset {
    pub fn from_asset<T: serde::Serialize>(
        mut header: AssetHeader,
        type_name: impl Into<String>,
        extension: impl Into<String>,
        asset: &T,
    ) -> anyhow::Result<Self> {
        header.version = 2;
        header.type_name = Some(type_name.into());
        Ok(Self {
            header,
            extension: extension.into(),
            payload: bincode::serialize(asset)?,
        })
    }
}

/// Decoder representation for files produced before the extensible importer
/// format. Never use this enum when defining a new asset type.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) enum LegacyAssetPayload {
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

    pub fn asset_uuid(&self, stable_path: &PathBuf) -> Option<Uuid> {
        self.catalog.uuid_for_path(stable_path)
    }
}
