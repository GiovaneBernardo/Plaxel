pub use crate::assets::loader;
use crate::renderer::RendererAPI;
use std::hash::Hash;
use std::hash::Hasher;
pub use std::path::Path;
use std::{collections::HashMap, fs};
pub use uuid::Uuid;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(bound = "")]
pub struct Handle<T> {
    pub uuid: Uuid,
    pub asset_type: AssetType,
    #[serde(skip)]
    pub _marker: std::marker::PhantomData<T>,
}

impl<T> std::fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle")
            .field("uuid", &self.uuid)
            .field("asset_type", &self.asset_type)
            .finish()
    }
}

impl<T> Copy for Handle<T> {}
impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.uuid == other.uuid
    }
}
impl<T> Eq for Handle<T> {}
impl<T> Hash for Handle<T> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.uuid.hash(h)
    }
}

#[derive(Copy, Clone, serde::Serialize, serde::Deserialize, std::fmt::Debug)]
pub enum AssetType {
    Material,
    Texture,
    Mesh,
    Prefab,
    Audio,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, std::fmt::Debug)]
pub struct AssetHeader {
    pub version: u16,
    pub uuid: Uuid,
    pub name: String,
    pub asset_type: AssetType,
    pub file_path: String,
    pub content_offset: u32,
    pub content_size: u64,
}

pub trait Asset {
    fn uuid(&self) -> Uuid;
}

pub struct AssetManager {
    pub headers: HashMap<Uuid, AssetHeader>,
    pub assets: HashMap<Uuid, Box<dyn Asset>>,
}

impl AssetManager {
    pub fn scan_folder(&mut self, folder: &Path) -> anyhow::Result<()> {
        for entry in fs::read_dir(folder)? {
            let path = entry?.path();
            if path.extension() == Some("plax".as_ref()) {
                let header = loader::load_header(&path).unwrap();
                self.headers.insert(header.uuid, header);
            }
        }
        Ok(())
    }

    pub fn load_assets(&mut self, ctx: &AssetContext) {
        let headers: Vec<_> = self.headers.values().cloned().collect();

        for header in &headers {
            loader::load_asset(self, ctx, header);
        }
    }
}

pub struct AssetContext {
    pub renderer_api: Box<dyn RendererAPI>,
}
