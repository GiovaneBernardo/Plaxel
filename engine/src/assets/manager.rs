pub use crate::assets::loader;
use crate::assets::server::AssetServer;
use crate::renderer::RendererAPI;
use std::any::Any;
use std::any::TypeId;
use std::hash::Hash;
use std::hash::Hasher;
pub use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
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

#[derive(
    Copy, Clone, serde::Serialize, serde::Deserialize, std::fmt::Debug, PartialEq, Eq, Hash,
)]
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
    #[serde(skip)]
    pub file_path: PathBuf,
    #[serde(skip)]
    pub content_offset: u32,
    #[serde(skip)]
    pub content_size: u64,
}

pub trait Asset {
    const ASSET_TYPE: AssetType;

    fn uuid(&self) -> Uuid;
}

pub struct Assets<T> {
    items: HashMap<Uuid, T>,
}

impl<T: Asset> Assets<T> {
    pub fn add(&mut self, asset: T) -> Option<&T> {
        let uuid = asset.uuid();
        self.items.insert(asset.uuid(), asset);
        return self.items.get(&uuid);
    }

    pub fn get(&self, uuid: &Uuid) -> Option<&T> {
        self.items.get(uuid)
    }

    pub fn get_mut(&mut self, uuid: &Uuid) -> Option<&mut T> {
        self.items.get_mut(uuid)
    }
}

pub struct UntypedHandle {
    pub uuid: Uuid,
    pub asset_type: AssetType,
    pub type_id: TypeId,
}

pub struct AssetRegistry {}

pub struct AssetManager {
    pub server: AssetServer,
    pub headers: HashMap<Uuid, AssetHeader>,
    pub storages: HashMap<TypeId, Box<dyn Any>>,
    pub names: HashMap<(AssetType, String), UntypedHandle>,
    pub paths: HashMap<PathBuf, Uuid>,
}

impl AssetManager {
    pub fn new() -> Self {
        Self {
            server: AssetServer {},
            headers: HashMap::new(),
            storages: HashMap::new(),
            names: HashMap::new(),
            paths: HashMap::new(),
        }
    }

    pub fn scan_folder(&mut self, folder: &Path) -> anyhow::Result<()> {
        for entry in fs::read_dir(folder)? {
            let path = entry?.path();
            if path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension == "plax" || extension.starts_with("plx"))
            {
                let header = loader::load_header(&path).unwrap();
                self.paths.insert(path.to_path_buf(), header.uuid);
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

    pub fn register_asset_type<T: 'static>(&mut self) {
        self.storages.insert(
            TypeId::of::<T>(),
            Box::new(Assets::<T> {
                items: HashMap::new(),
            }),
        );
    }

    pub fn assets<T: Asset + 'static>(&self) -> Option<&Assets<T>> {
        self.storages
            .get(&TypeId::of::<T>())?
            .downcast_ref::<Assets<T>>()
    }

    pub fn assets_mut<T: Asset + 'static>(&mut self) -> Option<&mut Assets<T>> {
        self.storages
            .get_mut(&TypeId::of::<T>())?
            .downcast_mut::<Assets<T>>()
    }

    pub fn add_asset<T: Asset + 'static>(&mut self, asset: T) -> Option<&T> {
        if !self.storages.contains_key(&TypeId::of::<T>()) {
            self.register_asset_type::<T>();
        }
        self.assets_mut::<T>().unwrap().add(asset)
    }

    pub fn get_by_uuid<T: Asset + 'static>(&self, uuid: Uuid) -> Option<&T> {
        self.assets::<T>()?.get(&uuid)
    }

    pub fn get<T: Asset + 'static>(&self, handle: Handle<T>) -> Option<&T> {
        self.assets::<T>()?.get(&handle.uuid)
    }

    pub fn get_mut<T: Asset + 'static>(&mut self, handle: Handle<T>) -> Option<&mut T> {
        self.assets_mut::<T>()?.get_mut(&handle.uuid)
    }

    pub fn get_by_name<T: Asset + 'static>(&self, name: &str) -> Option<&T> {
        let handle = self.handle::<T>(name)?;
        self.get(handle)
    }

    pub fn handle<T: Asset + 'static>(&self, name: &str) -> Option<Handle<T>> {
        let untyped = self.names.get(&(T::ASSET_TYPE, name.to_string()))?;

        Some(Handle {
            uuid: untyped.uuid,
            asset_type: T::ASSET_TYPE,
            _marker: std::marker::PhantomData,
        })
    }

    pub fn uuid_for_path(&self, path: &PathBuf) -> Option<&Uuid> {
        self.paths.get(path)
    }
}

pub struct AssetContext {
    pub renderer_api: Box<dyn RendererAPI>,
}
