use std::{
    any::TypeId,
    collections::HashMap,
    hash::{Hash, Hasher},
    marker::PhantomData,
    path::{Path, PathBuf},
};

pub use uuid::Uuid;

/// Stable identity shared by serialized references and the runtime asset stores.
pub type AssetId = Uuid;

/// A cheap, typed reference to an asset.
///
/// The marker is compile-time only: serialized handles contain just the stable
/// UUID, so runtime type registration is not coupled to a closed asset enum.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(bound = "")]
pub struct Handle<T> {
    pub uuid: AssetId,
    #[serde(skip)]
    pub _marker: PhantomData<fn() -> T>,
}

impl<T: 'static> Handle<T> {
    pub const fn new(uuid: AssetId) -> Self {
        Self {
            uuid,
            _marker: PhantomData,
        }
    }

    pub const fn id(self) -> AssetId {
        self.uuid
    }

    pub fn untyped(self) -> UntypedHandle {
        UntypedHandle {
            uuid: self.uuid,
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
        }
    }
}

impl<T> std::fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Handle").field(&self.uuid).finish()
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
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.uuid.hash(state);
    }
}

/// Marker implemented by every CPU-side runtime asset.
///
/// `uuid` is part of the current engine data model and lets generated assets
/// retain their identity when inserted. It does not select a storage or loader.
pub trait Asset: Send + Sync + 'static {
    fn uuid(&self) -> AssetId;
}

#[derive(Debug)]
struct AssetEntry<T> {
    value: T,
    version: u64,
}

/// Type-specific CPU asset storage kept as a normal world resource.
pub struct Assets<T: Asset> {
    items: HashMap<AssetId, AssetEntry<T>>,
    revision: u64,
}

/// Renderer-owned map from CPU asset handles to prepared GPU representations.
/// Put this in `Renderer::render_resources`, not in the ECS world. A renderer
/// or game plugin decides what `G` is and updates it from `AssetEvent<A>`.
pub struct GpuAssets<A: Asset, G: Send + Sync + 'static> {
    items: HashMap<AssetId, G>,
    marker: PhantomData<fn() -> A>,
}

impl<A: Asset, G: Send + Sync + 'static> Default for GpuAssets<A, G> {
    fn default() -> Self {
        Self {
            items: HashMap::new(),
            marker: PhantomData,
        }
    }
}

impl<A: Asset, G: Send + Sync + 'static> GpuAssets<A, G> {
    pub fn insert(&mut self, handle: Handle<A>, prepared: G) -> Option<G> {
        self.items.insert(handle.uuid, prepared)
    }

    pub fn get(&self, handle: Handle<A>) -> Option<&G> {
        self.items.get(&handle.uuid)
    }

    pub fn get_mut(&mut self, handle: Handle<A>) -> Option<&mut G> {
        self.items.get_mut(&handle.uuid)
    }

    pub fn remove(&mut self, handle: Handle<A>) -> Option<G> {
        self.items.remove(&handle.uuid)
    }

    pub fn contains(&self, handle: Handle<A>) -> bool {
        self.items.contains_key(&handle.uuid)
    }
}

impl<T: Asset> Default for Assets<T> {
    fn default() -> Self {
        Self {
            items: HashMap::new(),
            revision: 0,
        }
    }
}

impl<T: Asset> Assets<T> {
    pub fn insert(&mut self, id: AssetId, asset: T) -> Option<T> {
        self.revision = self.revision.wrapping_add(1);
        let version = self
            .items
            .get(&id)
            .map_or(1, |entry| entry.version.wrapping_add(1));
        self.items
            .insert(
                id,
                AssetEntry {
                    value: asset,
                    version,
                },
            )
            .map(|entry| entry.value)
    }

    pub fn add(&mut self, asset: T) -> Handle<T> {
        let handle = Handle::new(asset.uuid());
        self.insert(handle.uuid, asset);
        handle
    }

    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        self.get_by_id(handle.uuid)
    }

    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        self.get_mut_by_id(handle.uuid)
    }

    pub fn get_by_id(&self, id: AssetId) -> Option<&T> {
        self.items.get(&id).map(|entry| &entry.value)
    }

    pub fn get_mut_by_id(&mut self, id: AssetId) -> Option<&mut T> {
        let entry = self.items.get_mut(&id)?;
        self.revision = self.revision.wrapping_add(1);
        entry.version = entry.version.wrapping_add(1);
        Some(&mut entry.value)
    }

    pub fn version(&self, handle: Handle<T>) -> Option<u64> {
        self.items.get(&handle.uuid).map(|entry| entry.version)
    }

    pub fn contains(&self, handle: Handle<T>) -> bool {
        self.items.contains_key(&handle.uuid)
    }

    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        let removed = self.items.remove(&handle.uuid).map(|entry| entry.value);
        if removed.is_some() {
            self.revision = self.revision.wrapping_add(1);
        }
        removed
    }

    pub fn iter(&self) -> impl Iterator<Item = (Handle<T>, &T)> {
        self.items
            .iter()
            .map(|(id, entry)| (Handle::new(*id), &entry.value))
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UntypedHandle {
    pub uuid: AssetId,
    pub type_id: TypeId,
    pub type_name: &'static str,
}

impl UntypedHandle {
    pub fn typed<T: Asset>(self) -> Option<Handle<T>> {
        (self.type_id == TypeId::of::<T>()).then(|| Handle::new(self.uuid))
    }
}

/// Events emitted when a typed CPU store changes. Renderer preparation and
/// gameplay readers have independent cursors, so neither consumes the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetEvent<T: Asset> {
    Added { handle: Handle<T>, version: u64 },
    Modified { handle: Handle<T>, version: u64 },
    Removed { handle: Handle<T> },
}

/// The enum remains only in version-1 cooked-file metadata. Runtime type
/// registration and handles do not depend on it.
#[derive(Copy, Clone, serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Hash)]
pub enum AssetType {
    Material,
    Texture,
    Mesh,
    Prefab,
    Audio,
    Custom,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
pub struct AssetHeader {
    pub version: u16,
    pub uuid: AssetId,
    pub name: String,
    pub asset_type: AssetType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    #[serde(skip)]
    pub file_path: PathBuf,
    #[serde(skip)]
    pub content_offset: u32,
    #[serde(skip)]
    pub content_size: u64,
}

/// Editor-facing index of cooked files. This is metadata only; assets live in
/// `Assets<T>` world resources and are loaded by `AssetServer`.
#[derive(Default)]
pub struct AssetCatalog {
    pub headers: HashMap<AssetId, AssetHeader>,
    pub paths: HashMap<PathBuf, AssetId>,
}

impl AssetCatalog {
    pub fn scan_folder(&mut self, folder: &Path) -> anyhow::Result<()> {
        if !folder.exists() {
            return Ok(());
        }
        self.scan_recursive(folder)
    }

    fn scan_recursive(&mut self, folder: &Path) -> anyhow::Result<()> {
        for entry in std::fs::read_dir(folder)? {
            let path = entry?.path();
            if path.is_dir() {
                self.scan_recursive(&path)?;
                continue;
            }
            if path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension == "plax" || extension.starts_with("plx"))
            {
                let header = crate::assets::loader::load_header(&path)?;
                self.paths.insert(path, header.uuid);
                self.headers.insert(header.uuid, header);
            }
        }
        Ok(())
    }

    pub fn uuid_for_path(&self, path: &Path) -> Option<AssetId> {
        self.paths.get(path).copied()
    }
}
