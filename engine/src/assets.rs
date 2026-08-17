pub mod importer;
pub mod importers;
pub mod loader;
pub mod manager;
pub mod material;
pub mod plugin;
pub mod resources;
pub mod serializer;
pub mod server;

pub mod prelude {
    pub use super::{
        importer::{AssetImporter, AssetImporterRegistry, ImportContext, ImportedAsset},
        manager::{
            Asset, AssetCatalog, AssetEvent, AssetHeader, AssetId, AssetType, Assets, GpuAssets,
            Handle, UntypedHandle,
        },
        material::*,
        plugin::{AssetAppExt, AssetPlugin},
        server::{
            AssetLoadFailed, AssetLoader, AssetRegistration, AssetRegistry, AssetServer,
            LoadContext, LoadState,
        },
    };
}
