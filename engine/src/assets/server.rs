use std::{
    any::{Any, TypeId, type_name},
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use parking_lot::Mutex;

use crate::assets::manager::{Asset, AssetHeader, AssetId, AssetType, Handle, UntypedHandle};

type BoxedAsset = Box<dyn Any + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadState {
    NotLoaded,
    Queued,
    Loading,
    Loaded,
    Failed(Arc<str>),
}

#[derive(Clone, Debug)]
pub struct AssetLoadFailed {
    pub handle: UntypedHandle,
    pub path: PathBuf,
    pub error: Arc<str>,
}

/// Runtime decoder for one asset type. Game crates can implement and register
/// this trait without changing the engine.
pub trait AssetLoader: Send + Sync + 'static {
    type Asset: Asset;

    /// A persistent identifier for manifests and future cooked-file formats.
    /// Override this with a project-owned name if Rust module paths may change.
    fn type_name(&self) -> &'static str {
        type_name::<Self::Asset>()
    }

    fn extensions(&self) -> &[&'static str];

    fn load(&self, bytes: &[u8], context: &mut LoadContext) -> anyhow::Result<Self::Asset>;

    /// Used only to discover version-1 cooked assets whose headers still use
    /// the old enum. New formats should identify the loader by `type_name`.
    fn legacy_asset_type(&self) -> Option<AssetType> {
        None
    }
}

trait ErasedAssetLoader: Send + Sync {
    fn asset_type_id(&self) -> TypeId;
    fn asset_type_name(&self) -> &'static str;
    fn extensions(&self) -> &[&'static str];
    fn legacy_asset_type(&self) -> Option<AssetType>;
    fn load(&self, bytes: &[u8], context: &mut LoadContext) -> anyhow::Result<BoxedAsset>;
}

struct LoaderAdapter<L>(L);

impl<L: AssetLoader> ErasedAssetLoader for LoaderAdapter<L> {
    fn asset_type_id(&self) -> TypeId {
        TypeId::of::<L::Asset>()
    }

    fn asset_type_name(&self) -> &'static str {
        self.0.type_name()
    }

    fn extensions(&self) -> &[&'static str] {
        self.0.extensions()
    }

    fn legacy_asset_type(&self) -> Option<AssetType> {
        self.0.legacy_asset_type()
    }

    fn load(&self, bytes: &[u8], context: &mut LoadContext) -> anyhow::Result<BoxedAsset> {
        Ok(Box::new(self.0.load(bytes, context)?))
    }
}

#[derive(Clone, Debug)]
pub struct AssetRegistration {
    pub type_id: TypeId,
    pub type_name: &'static str,
    pub extensions: Vec<&'static str>,
}

#[derive(Default)]
pub struct AssetRegistry {
    loaders: Vec<Arc<dyn ErasedAssetLoader>>,
}

impl AssetRegistry {
    pub fn register<L: AssetLoader>(&mut self, loader: L) {
        let loader: Arc<dyn ErasedAssetLoader> = Arc::new(LoaderAdapter(loader));
        let type_id = loader.asset_type_id();
        let extensions = loader.extensions();
        self.loaders.retain(|registered| {
            registered.asset_type_id() != type_id
                || !registered
                    .extensions()
                    .iter()
                    .any(|extension| extensions.contains(extension))
        });
        self.loaders.push(loader);
    }

    pub fn registrations(&self) -> Vec<AssetRegistration> {
        self.loaders
            .iter()
            .map(|loader| AssetRegistration {
                type_id: loader.asset_type_id(),
                type_name: loader.asset_type_name(),
                extensions: loader.extensions().to_vec(),
            })
            .collect()
    }

    fn loader_for(&self, type_id: TypeId, path: &Path) -> Option<Arc<dyn ErasedAssetLoader>> {
        let extension = path.extension()?.to_str()?;
        self.loaders
            .iter()
            .find(|loader| {
                loader.asset_type_id() == type_id
                    && loader
                        .extensions()
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(extension))
            })
            .cloned()
    }

    fn type_for_legacy(&self, asset_type: AssetType) -> Option<(TypeId, &'static str)> {
        self.loaders
            .iter()
            .find(|loader| loader.legacy_asset_type() == Some(asset_type))
            .map(|loader| (loader.asset_type_id(), loader.asset_type_name()))
    }

    fn type_for_name(&self, type_name: &str) -> Option<(TypeId, &'static str)> {
        self.loaders
            .iter()
            .find(|loader| loader.asset_type_name() == type_name)
            .map(|loader| (loader.asset_type_id(), loader.asset_type_name()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct AssetKey {
    type_id: TypeId,
    id: AssetId,
}

impl AssetKey {
    fn typed<T: Asset>(handle: Handle<T>) -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            id: handle.uuid,
        }
    }
}

struct LoadRequest {
    key: AssetKey,
    generation: u64,
    type_name: &'static str,
    path: PathBuf,
    loader: Arc<dyn ErasedAssetLoader>,
}

struct CompletedLoad {
    key: AssetKey,
    generation: u64,
    type_name: &'static str,
    path: PathBuf,
    result: Result<Option<BoxedAsset>, Arc<str>>,
    dependencies: Vec<AssetKey>,
}

#[derive(Default)]
struct ServerState {
    registry: AssetRegistry,
    paths: HashMap<(TypeId, PathBuf), AssetId>,
    reverse_paths: HashMap<AssetKey, PathBuf>,
    names: HashMap<(TypeId, String), AssetId>,
    states: HashMap<AssetKey, LoadState>,
    generations: HashMap<AssetKey, u64>,
    requests: Vec<LoadRequest>,
    completed: HashMap<TypeId, Vec<CompletedLoad>>,
}

#[derive(Clone, Default)]
pub struct AssetServer {
    state: Arc<Mutex<ServerState>>,
}

impl AssetServer {
    pub fn register_loader<L: AssetLoader>(&self, loader: L) {
        self.state.lock().registry.register(loader);
    }

    pub fn registrations(&self) -> Vec<AssetRegistration> {
        self.state.lock().registry.registrations()
    }

    pub fn register_cooked_path(&self, header: &AssetHeader, path: impl Into<PathBuf>) {
        let path = normalize_path(&path.into());
        let mut state = self.state.lock();
        let registered_type = header
            .type_name
            .as_deref()
            .and_then(|name| state.registry.type_for_name(name))
            .or_else(|| state.registry.type_for_legacy(header.asset_type));
        let Some((type_id, _)) = registered_type else {
            return;
        };
        let key = AssetKey {
            type_id,
            id: header.uuid,
        };
        state.paths.insert((type_id, path.clone()), header.uuid);
        state.reverse_paths.insert(key, path);
        state
            .names
            .insert((type_id, header.name.clone()), header.uuid);
    }

    /// Starts an asynchronous request and returns immediately. Repeated loads
    /// of the same typed path reuse the handle and in-flight job.
    pub fn load<T: Asset>(&self, path: impl AsRef<Path>) -> Handle<T> {
        let path = normalize_path(path.as_ref());
        let type_id = TypeId::of::<T>();
        let mut state = self.state.lock();
        let id = state
            .paths
            .get(&(type_id, path.clone()))
            .copied()
            .unwrap_or_else(AssetId::new_v4);
        let handle = Handle::new(id);
        let key = AssetKey::typed(handle);

        if matches!(
            state.states.get(&key),
            Some(LoadState::Queued | LoadState::Loading | LoadState::Loaded)
        ) {
            return handle;
        }

        state.paths.insert((type_id, path.clone()), id);
        state.reverse_paths.insert(key, path.clone());
        let generation = next_generation(&mut state, key);
        let Some(loader) = state.registry.loader_for(type_id, &path) else {
            let error: Arc<str> = format!(
                "no loader registered for `{}` at {}",
                type_name::<T>(),
                path.display()
            )
            .into();
            state.states.insert(key, LoadState::Failed(error.clone()));
            state
                .completed
                .entry(type_id)
                .or_default()
                .push(CompletedLoad {
                    key,
                    generation,
                    type_name: type_name::<T>(),
                    path,
                    result: Err(error),
                    dependencies: Vec::new(),
                });
            return handle;
        };

        state.states.insert(key, LoadState::Queued);
        state.requests.push(LoadRequest {
            key,
            generation,
            type_name: loader.asset_type_name(),
            path,
            loader,
        });
        handle
    }

    pub fn load_named<T: Asset>(&self, name: &str) -> Option<Handle<T>> {
        let state = self.state.lock();
        let id = state.names.get(&(TypeId::of::<T>(), name.to_owned()))?;
        let path = state.reverse_paths.get(&AssetKey {
            type_id: TypeId::of::<T>(),
            id: *id,
        })?;
        let path = path.clone();
        drop(state);
        Some(self.load(path))
    }

    pub fn load_by_id<T: Asset>(&self, id: AssetId) -> Option<Handle<T>> {
        let path = self
            .state
            .lock()
            .reverse_paths
            .get(&AssetKey {
                type_id: TypeId::of::<T>(),
                id,
            })?
            .clone();
        Some(self.load(path))
    }

    /// Queues a generated or procedurally updated asset for main-thread commit.
    pub fn add<T: Asset>(&self, asset: T) -> Handle<T> {
        let handle = Handle::new(asset.uuid());
        let key = AssetKey::typed(handle);
        let mut state = self.state.lock();
        let generation = next_generation(&mut state, key);
        state.states.insert(key, LoadState::Loading);
        state
            .completed
            .entry(TypeId::of::<T>())
            .or_default()
            .push(CompletedLoad {
                key,
                generation,
                type_name: type_name::<T>(),
                path: PathBuf::new(),
                result: Ok(Some(Box::new(asset))),
                dependencies: Vec::new(),
            });
        handle
    }

    /// Queues removal from the typed CPU store. Renderer cleanup observes the
    /// resulting `AssetEvent::Removed` on its normal preparation schedule.
    pub fn remove<T: Asset>(&self, handle: Handle<T>) {
        let key = AssetKey::typed(handle);
        let mut state = self.state.lock();
        let generation = next_generation(&mut state, key);
        let path = state.reverse_paths.get(&key).cloned().unwrap_or_default();
        state
            .completed
            .entry(TypeId::of::<T>())
            .or_default()
            .push(CompletedLoad {
                key,
                generation,
                type_name: type_name::<T>(),
                path,
                result: Ok(None),
                dependencies: Vec::new(),
            });
    }

    pub fn reload<T: Asset>(&self, handle: Handle<T>) -> bool {
        let path = {
            let mut state = self.state.lock();
            let key = AssetKey::typed(handle);
            let Some(path) = state.reverse_paths.get(&key).cloned() else {
                return false;
            };
            state.states.remove(&key);
            path
        };
        self.load::<T>(path);
        true
    }

    pub fn load_state<T: Asset>(&self, handle: Handle<T>) -> LoadState {
        self.state
            .lock()
            .states
            .get(&AssetKey::typed(handle))
            .cloned()
            .unwrap_or(LoadState::NotLoaded)
    }

    pub fn is_loaded<T: Asset>(&self, handle: Handle<T>) -> bool {
        self.load_state(handle) == LoadState::Loaded
    }

    pub fn path<T: Asset>(&self, handle: Handle<T>) -> Option<PathBuf> {
        self.state
            .lock()
            .reverse_paths
            .get(&AssetKey::typed(handle))
            .cloned()
    }

    pub(crate) fn take_requests(&self) -> Vec<AssetJob> {
        let mut state = self.state.lock();
        let requests = std::mem::take(&mut state.requests);
        requests
            .into_iter()
            .map(|request| {
                state.states.insert(request.key, LoadState::Loading);
                AssetJob {
                    server: self.clone(),
                    request: Some(request),
                }
            })
            .collect()
    }

    fn complete(&self, completed: CompletedLoad) {
        self.state
            .lock()
            .completed
            .entry(completed.key.type_id)
            .or_default()
            .push(completed);
    }

    pub(crate) fn take_ready<T: Asset>(&self) -> Vec<TypedAssetResult<T>> {
        let type_id = TypeId::of::<T>();
        let mut state = self.state.lock();
        let pending = state.completed.remove(&type_id).unwrap_or_default();
        let mut ready = Vec::new();
        let mut waiting = Vec::new();

        for mut completed in pending {
            if state.generations.get(&completed.key).copied().unwrap_or(0) != completed.generation {
                continue;
            }
            if completed.result.is_ok() {
                let failed_dependency = completed.dependencies.iter().find_map(|dependency| {
                    match state.states.get(dependency) {
                        Some(LoadState::Failed(error)) => Some(Arc::clone(error)),
                        _ => None,
                    }
                });
                if let Some(error) = failed_dependency {
                    completed.result = Err(format!("asset dependency failed: {error}").into());
                } else if !completed.dependencies.iter().all(|dependency| {
                    matches!(state.states.get(dependency), Some(LoadState::Loaded))
                }) {
                    waiting.push(completed);
                    continue;
                }
            }

            let result = match completed.result {
                Ok(Some(asset)) => asset
                    .downcast::<T>()
                    .map_err(|_| {
                        anyhow::anyhow!(
                            "loader for `{}` returned the wrong runtime type",
                            completed.type_name
                        )
                    })
                    .and_then(|asset| {
                        if asset.uuid() != completed.key.id {
                            anyhow::bail!(
                                "loader for `{}` returned UUID {}, expected {}",
                                completed.type_name,
                                asset.uuid(),
                                completed.key.id
                            );
                        }
                        Ok(Some(*asset))
                    })
                    .map_err(|error: anyhow::Error| Arc::<str>::from(format!("{error:#}"))),
                Ok(None) => Ok(None),
                Err(error) => Err(error),
            };
            ready.push(TypedAssetResult {
                handle: Handle::new(completed.key.id),
                path: completed.path,
                result,
            });
        }

        if !waiting.is_empty() {
            state.completed.entry(type_id).or_default().extend(waiting);
        }
        ready
    }

    pub(crate) fn committed<T: Asset>(&self, handle: Handle<T>) {
        self.state
            .lock()
            .states
            .insert(AssetKey::typed(handle), LoadState::Loaded);
    }

    pub(crate) fn failed<T: Asset>(&self, handle: Handle<T>, error: Arc<str>) {
        self.state
            .lock()
            .states
            .insert(AssetKey::typed(handle), LoadState::Failed(error));
    }

    pub(crate) fn unloaded<T: Asset>(&self, handle: Handle<T>) {
        self.state.lock().states.remove(&AssetKey::typed(handle));
    }
}

pub struct LoadContext {
    server: AssetServer,
    asset_id: AssetId,
    asset_path: PathBuf,
    dependencies: Vec<AssetKey>,
}

impl LoadContext {
    fn new(server: AssetServer, asset_id: AssetId, asset_path: PathBuf) -> Self {
        Self {
            server,
            asset_id,
            asset_path,
            dependencies: Vec::new(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.asset_path
    }

    pub fn id(&self) -> AssetId {
        self.asset_id
    }

    pub fn load_dependency<T: Asset>(&mut self, path: impl AsRef<Path>) -> Handle<T> {
        let path = if path.as_ref().is_absolute() {
            path.as_ref().to_path_buf()
        } else {
            self.asset_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(path)
        };
        let handle = self.server.load::<T>(path);
        self.dependencies.push(AssetKey::typed(handle));
        handle
    }

    pub fn depend_on<T: Asset>(&mut self, handle: Handle<T>) {
        self.dependencies.push(AssetKey::typed(handle));
    }

    pub fn load_dependency_by_id<T: Asset>(&mut self, id: AssetId) -> Option<Handle<T>> {
        let handle = self.server.load_by_id::<T>(id)?;
        self.depend_on(handle);
        Some(handle)
    }
}

pub(crate) struct AssetJob {
    server: AssetServer,
    request: Option<LoadRequest>,
}

impl AssetJob {
    pub(crate) fn name(&self) -> String {
        let request = self.request.as_ref().unwrap();
        format!("asset.load:{}", request.path.display())
    }

    pub(crate) fn run(mut self) {
        let request = self.request.take().unwrap();
        let mut context =
            LoadContext::new(self.server.clone(), request.key.id, request.path.clone());
        let result = std::fs::read(&request.path)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| request.loader.load(&bytes, &mut context))
            .map_err(|error| Arc::<str>::from(format!("{error:#}")));
        self.server.complete(CompletedLoad {
            key: request.key,
            generation: request.generation,
            type_name: request.type_name,
            path: request.path,
            result: result.map(Some),
            dependencies: context.dependencies,
        });
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut normalized = PathBuf::new();
    let absolute = path.is_absolute();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let can_pop = normalized
                    .file_name()
                    .is_some_and(|name| name != std::ffi::OsStr::new(".."));
                if can_pop {
                    normalized.pop();
                } else if !absolute {
                    normalized.push("..");
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

fn next_generation(state: &mut ServerState, key: AssetKey) -> u64 {
    let generation = state.generations.entry(key).or_default();
    *generation = generation.wrapping_add(1);
    *generation
}

pub(crate) struct TypedAssetResult<T: Asset> {
    pub handle: Handle<T>,
    pub path: PathBuf,
    pub result: Result<Option<T>, Arc<str>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TextAsset {
        id: AssetId,
        text: String,
    }

    impl Asset for TextAsset {
        fn uuid(&self) -> AssetId {
            self.id
        }
    }

    struct TextLoader;

    impl AssetLoader for TextLoader {
        type Asset = TextAsset;

        fn extensions(&self) -> &[&'static str] {
            &["txt"]
        }

        fn load(&self, bytes: &[u8], context: &mut LoadContext) -> anyhow::Result<TextAsset> {
            Ok(TextAsset {
                id: context.id(),
                text: std::str::from_utf8(bytes)?.to_owned(),
            })
        }
    }

    #[test]
    fn registration_is_open_to_external_asset_types() {
        let server = AssetServer::default();
        server.register_loader(TextLoader);
        let registrations = server.registrations();
        assert_eq!(registrations.len(), 1);
        assert_eq!(registrations[0].type_id, TypeId::of::<TextAsset>());
        assert_eq!(registrations[0].extensions, vec!["txt"]);
    }

    #[test]
    fn generated_assets_commit_through_the_typed_queue() {
        let server = AssetServer::default();
        let id = AssetId::new_v4();
        let handle = server.add(TextAsset {
            id,
            text: "hello".into(),
        });
        let mut completed = server.take_ready::<TextAsset>();
        let loaded = completed.pop().unwrap().result.unwrap().unwrap();
        assert_eq!(handle.uuid, id);
        assert_eq!(loaded.text, "hello");
    }

    #[test]
    fn stale_worker_results_cannot_overwrite_a_newer_asset() {
        let server = AssetServer::default();
        server.register_loader(TextLoader);
        let handle = server.load::<TextAsset>("missing.txt");
        let stale_job = server.take_requests().pop().unwrap();
        server.add(TextAsset {
            id: handle.uuid,
            text: "newest".into(),
        });
        stale_job.run();

        let mut completed = server.take_ready::<TextAsset>();
        assert_eq!(completed.len(), 1);
        let loaded = completed.pop().unwrap().result.unwrap().unwrap();
        assert_eq!(loaded.text, "newest");
    }
}
