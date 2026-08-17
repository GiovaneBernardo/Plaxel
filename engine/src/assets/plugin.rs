use plaxel_reflect::GetPath;

use crate::{
    App,
    assets::{
        importer::{
            AssetImporter, AssetImporterRegistry, ImportContext, ImportSettings, TargetPlatform,
        },
        importers::obj_importer::ObjImporter,
        loader::{self, CookedMaterialLoader, CookedMeshLoader, CookedTextureLoader},
        manager::{Asset, AssetCatalog, AssetEvent, Assets},
        serializer,
        server::{AssetLoadFailed, AssetLoader, AssetServer},
    },
    core::window::FileDropped,
    ecs::{
        event::{EventReader, EventWriter},
        plugin::Plugin,
        resource::{Res, ResMut},
        schedule::CoreSchedule,
        system::Globals,
    },
};

pub struct AssetPlugin;

impl Plugin for AssetPlugin {
    fn build(&self, app: &mut App) {
        if !app.world.contains_resource::<AssetServer>() {
            app.insert_opaque_resource(AssetServer::default());
        }
        if !app.world.contains_resource::<AssetCatalog>() {
            app.insert_opaque_resource(AssetCatalog::default());
        }
        if !app.world.contains_resource::<AssetImporterRegistry>() {
            app.insert_opaque_resource(AssetImporterRegistry::default());
        }

        app.add_event::<AssetLoadFailed>()
            .register_asset_importer(ObjImporter)
            .register_asset_loader(CookedTextureLoader)
            .register_asset_loader(CookedMaterialLoader)
            .register_asset_loader(CookedMeshLoader)
            .add_system(CoreSchedule::Startup, scan_default_asset_catalog)
            .add_system(CoreSchedule::PreUpdate, dispatch_asset_jobs)
            .add_system(CoreSchedule::PreUpdate, log_asset_failures)
            .add_system(CoreSchedule::Update, handle_file_drop);
    }
}

/// Bevy-style asset registration on `App`. `init_asset` installs the typed CPU
/// storage, event stream, and commit system exactly once.
pub trait AssetAppExt {
    fn init_asset<T: Asset>(&mut self) -> &mut Self;
    fn register_asset_loader<L: AssetLoader>(&mut self, loader: L) -> &mut Self;
    fn register_asset_importer<I: AssetImporter>(&mut self, importer: I) -> &mut Self;
}

impl AssetAppExt for App {
    fn init_asset<T: Asset>(&mut self) -> &mut Self {
        if self.world.contains_resource::<Assets<T>>() {
            return self;
        }

        self.insert_opaque_resource(Assets::<T>::default())
            .add_event::<AssetEvent<T>>()
            .add_system(CoreSchedule::PreUpdate, commit_asset_jobs::<T>)
    }

    fn register_asset_loader<L: AssetLoader>(&mut self, loader: L) -> &mut Self {
        self.init_asset::<L::Asset>();
        let server = self.world.get_resource::<AssetServer>().unwrap_or_else(|| {
            panic!("AssetPlugin must be added before registering an asset loader")
        });
        server.register_loader(loader);
        drop(server);
        self
    }

    fn register_asset_importer<I: AssetImporter>(&mut self, importer: I) -> &mut Self {
        let mut registry = self
            .world
            .get_resource_mut::<AssetImporterRegistry>()
            .unwrap_or_else(|| {
                panic!("AssetPlugin must be added before registering an asset importer")
            });
        registry.register(importer);
        drop(registry);
        self
    }
}

fn scan_default_asset_catalog(server: Res<AssetServer>, mut catalog: ResMut<AssetCatalog>) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../res/imported");
    if let Err(error) = catalog.scan_folder(&root) {
        log::warn!(
            "failed to scan asset catalog at {}: {error:#}",
            root.display()
        );
        return;
    }

    for header in catalog.headers.values() {
        server.register_cooked_path(header, header.file_path.clone());
    }
}

fn dispatch_asset_jobs(server: Res<AssetServer>, globals: Globals) {
    for job in server.take_requests() {
        let name = job.name();
        if globals
            .job_system
            .spawn_named(name, move || job.run())
            .is_err()
        {
            log::error!("asset job system is shutting down; load could not be dispatched");
        }
    }
}

fn log_asset_failures(mut failures: crate::ecs::event::EventReader<AssetLoadFailed>) {
    for failure in failures.read() {
        log::error!(
            "failed to load asset `{}` from {}: {}",
            failure.handle.type_name,
            failure.path.display(),
            failure.error
        );
    }
}

fn commit_asset_jobs<T: Asset>(
    server: Res<AssetServer>,
    mut assets: ResMut<Assets<T>>,
    mut events: EventWriter<AssetEvent<T>>,
    mut failures: EventWriter<AssetLoadFailed>,
) {
    for completed in server.take_ready::<T>() {
        match completed.result {
            Ok(Some(asset)) => {
                let existed = assets.contains(completed.handle);
                assets.insert(completed.handle.uuid, asset);
                let version = assets.version(completed.handle).unwrap();
                server.committed(completed.handle);
                if existed {
                    events.send(AssetEvent::Modified {
                        handle: completed.handle,
                        version,
                    });
                } else {
                    events.send(AssetEvent::Added {
                        handle: completed.handle,
                        version,
                    });
                }
            }
            Ok(None) => {
                assets.remove(completed.handle);
                server.unloaded(completed.handle);
                events.send(AssetEvent::Removed {
                    handle: completed.handle,
                });
            }
            Err(error) => {
                server.failed(completed.handle, error.clone());
                failures.send(AssetLoadFailed {
                    handle: completed.handle.untyped(),
                    path: completed.path,
                    error,
                });
            }
        }
    }
}

// Handle dropped files for importing them
fn handle_file_drop(
    mut events: EventReader<FileDropped>,
    mut catalog: ResMut<AssetCatalog>,
    mut server: ResMut<AssetServer>,
) {
    for event in events.read() {
        println!("Dropped: {}", event.path.display());

        if event.path.extension().is_some_and(|ext| ext == "obj") {
            let imported = ObjImporter
                .import(
                    &event.path,
                    &ImportContext {
                        asset_root: &event.path,
                        catalog: &catalog,
                        project_root: &event.path,
                        settings: &ImportSettings {
                            force_reimport: true,
                            generate_mipmaps: true,
                            ignored_platform: TargetPlatform::None,
                        },
                        source_path: &event.path,
                        source_root: &event.path,
                    },
                )
                .unwrap();

            for asset in imported {
                let output = serializer::output_path_for(&asset, &event.path.parent().unwrap());
                serializer::write_imported_asset(&asset, &output).unwrap();

                let header = loader::load_header(&output).unwrap();
                catalog.paths.insert(output.clone(), header.uuid);
                catalog.headers.insert(header.uuid, header.clone());
                server.register_cooked_path(&header, output);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::manager::AssetId;

    struct TestAsset(AssetId);

    impl Asset for TestAsset {
        fn uuid(&self) -> AssetId {
            self.0
        }
    }

    #[test]
    fn init_asset_is_idempotent() {
        let mut app = App::new();
        app.add_plugin(AssetPlugin)
            .init_asset::<TestAsset>()
            .init_asset::<TestAsset>();
        assert!(app.world.contains_resource::<Assets<TestAsset>>());
    }
}
