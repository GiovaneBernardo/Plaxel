use crate::ecs::system::SystemContext;
use crate::model::MeshAsset;
use crate::prelude::*;

pub struct RendererPlugin;
impl Plugin for RendererPlugin {
    fn build(&self, app: &mut crate::App) {
        if !app.world.contains_resource::<AssetServer>() {
            app.add_plugin(AssetPlugin);
        }
        app.add_system(CoreSchedule::Startup, init_renderer)
            .add_legacy_system(CoreSchedule::RenderExtract, sync_render_database)
            .add_system(
                CoreSchedule::RenderPrepare,
                crate::core::window::resize_renderer_from_events,
            )
            .add_system(CoreSchedule::RenderPrepare, prepare_texture_assets)
            .add_system(CoreSchedule::RenderPrepare, prepare_material_assets)
            .add_system(CoreSchedule::RenderPrepare, prepare_mesh_assets)
            .add_system(CoreSchedule::Render, render);
    }
}

fn prepare_texture_assets(
    mut events: EventReader<AssetEvent<TextureAsset>>,
    assets: Res<Assets<TextureAsset>>,
    mut globals: GlobalsMut,
) {
    for event in events.read() {
        let handle = match event {
            AssetEvent::Added { handle, .. } | AssetEvent::Modified { handle, .. } => *handle,
            AssetEvent::Removed { handle } => {
                if let Some(prepared) = globals
                    .renderer
                    .render_resources
                    .get_mut::<GpuAssets<TextureAsset, TextureHandle>>()
                {
                    prepared.remove(*handle);
                }
                continue;
            }
        };
        if let Some(texture) = assets.get(handle) {
            let prepared_texture = globals
                .renderer
                .renderer_api
                .upload_texture_asset(texture, None);
            globals
                .renderer
                .render_resources
                .get_mut::<GpuAssets<TextureAsset, TextureHandle>>()
                .unwrap()
                .insert(handle, prepared_texture);
        }
    }
}

fn prepare_material_assets(
    mut events: EventReader<AssetEvent<Material>>,
    mut assets: ResMut<Assets<Material>>,
    mut globals: GlobalsMut,
) {
    for event in events.read() {
        let (handle, previous_index) = match event {
            AssetEvent::Added { handle, .. } => (*handle, None),
            AssetEvent::Modified { handle, .. } => {
                let index = assets.get(*handle).map(|material| material.material_index);
                (*handle, index)
            }
            AssetEvent::Removed { handle } => {
                if let Some(prepared) = globals
                    .renderer
                    .render_resources
                    .get_mut::<GpuAssets<Material, u32>>()
                {
                    prepared.remove(*handle);
                }
                continue;
            }
        };
        let Some(material) = assets.get_mut(handle) else {
            continue;
        };
        material.material_index = globals
            .renderer
            .renderer_api
            .upload_material_asset(material, previous_index);
        globals
            .renderer
            .render_resources
            .get_mut::<GpuAssets<Material, u32>>()
            .unwrap()
            .insert(handle, material.material_index);
    }
}

fn prepare_mesh_assets(
    mut events: EventReader<AssetEvent<MeshAsset>>,
    assets: Res<Assets<MeshAsset>>,
    mut globals: GlobalsMut,
) {
    for event in events.read() {
        match event {
            AssetEvent::Added { handle, .. } | AssetEvent::Modified { handle, .. } => {
                if let Some(mesh) = assets.get(*handle) {
                    globals.renderer.renderer_api.upload_mesh_asset(mesh);
                    globals
                        .renderer
                        .render_resources
                        .get_mut::<GpuAssets<MeshAsset, ()>>()
                        .unwrap()
                        .insert(*handle, ());
                }
            }
            AssetEvent::Removed { handle } => {
                globals.renderer.renderer_api.remove_mesh_asset(*handle);
                globals
                    .renderer
                    .render_resources
                    .get_mut::<GpuAssets<MeshAsset, ()>>()
                    .unwrap()
                    .remove(*handle);
            }
        }
    }
}

fn init_renderer(mut globals: GlobalsMut) {
    let renderer = &mut globals.renderer;
    renderer.init();
    renderer
        .render_resources
        .insert(GpuAssets::<TextureAsset, TextureHandle>::default());
    renderer
        .render_resources
        .insert(GpuAssets::<Material, u32>::default());
    renderer
        .render_resources
        .insert(GpuAssets::<MeshAsset, ()>::default());
}

fn render(mut globals: GlobalsMut) {
    if let Err(error) = globals.renderer.render() {
        log::error!("Unable to render: {error}");
    } else {
        globals.frame_capturer.finish_capture_after_frame();
    }
    globals.profiler_snapshot = crate::profiling::shared_snapshot();
}

fn sync_render_database(
    ctx: &mut SystemContext<'_>,
    _commands: &mut crate::ecs::commands::Commands,
) {
    let globals = &mut ctx.globals;
    globals.renderer.sync_render_database(ctx.world);
}
