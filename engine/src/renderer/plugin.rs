use crate::ecs::system::SystemContext;
use crate::prelude::*;

pub struct RendererPlugin;
impl Plugin for RendererPlugin {
    fn build(&self, app: &mut crate::App) {
        app.add_system(CoreSchedule::Startup, init_renderer)
            .add_legacy_system(CoreSchedule::RenderExtract, sync_render_database)
            .add_system(
                CoreSchedule::RenderPrepare,
                crate::core::window::resize_renderer_from_events,
            )
            .add_system(CoreSchedule::Render, render);
    }
}

fn init_renderer(mut globals: GlobalsMut) {
    let renderer = &mut globals.renderer;
    renderer.init();
}

fn render(mut globals: GlobalsMut) {
    if let Err(error) = globals.renderer.render() {
        log::error!("Unable to render: {error}");
    } else {
        globals.frame_capturer.finish_capture_after_frame();
    }
    globals.profiler_snapshot = crate::profiling::snapshot();
}

fn sync_render_database(
    ctx: &mut SystemContext<'_>,
    _commands: &mut crate::ecs::commands::Commands,
) {
    let globals = &mut ctx.globals;
    globals
        .renderer
        .sync_render_database(ctx.world, &globals.asset_manager);
}
