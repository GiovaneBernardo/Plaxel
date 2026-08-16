pub extern crate bevy_reflect as plaxel_reflect;

pub mod editor;
pub mod editor_ui;
pub mod egui_node;
pub mod hierarchy;
pub mod terrain_editor;
pub mod viewport_context_menu;

use egui_node::EguiRenderNode;
use engine::{
    ecs::{
        commands::Commands,
        event::{Events, ManualEventReader},
        plugin::Plugin,
        schedule::CoreSchedule,
        system::SystemContext,
        world::World,
    },
    global_resources::GlobalResources,
};
use viewport_context_menu::{EditorSpawnRequests, editor_spawn_system};

pub const EGUI_NODE_INDEX: engine::renderer::ids::GraphPassId =
    engine::renderer::ids::graph_passes::EGUI;

/// The editor's exclusive view of the ECS runtime. It deliberately borrows
/// the new App world/globals instead of owning a second engine state.
pub struct EditorContext<'world> {
    pub world: &'world mut World,
    pub global_resources: &'world mut GlobalResources,
}

pub struct EditorSceneRef<'world> {
    world: &'world World,
}

impl<'world> EditorSceneRef<'world> {
    pub fn world(&self) -> &World {
        self.world
    }
}

pub struct EditorSceneMut<'world> {
    world: &'world mut World,
}

impl<'world> EditorSceneMut<'world> {
    pub fn world_mut(self) -> &'world mut World {
        self.world
    }
}

impl EditorContext<'_> {
    pub fn active_scene(&self) -> Option<EditorSceneRef<'_>> {
        Some(EditorSceneRef { world: self.world })
    }

    pub fn active_scene_mut(&mut self) -> Option<EditorSceneMut<'_>> {
        Some(EditorSceneMut { world: self.world })
    }

    pub fn spawn_dropped_obj(
        &mut self,
        _path: &std::path::Path,
        _position: &engine::math::Vec3,
    ) -> anyhow::Result<()> {
        anyhow::bail!("direct OBJ spawning has not yet been moved into the asset plugin")
    }
}

struct EditorWindowEventReader(ManualEventReader<winit::event::WindowEvent>);

impl Default for EditorWindowEventReader {
    fn default() -> Self {
        Self(ManualEventReader::default())
    }
}

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut engine::App) {
        app.insert_resource(EditorSpawnRequests::default())
            .insert_opaque_resource(EditorWindowEventReader::default())
            .add_named_legacy_system(
                CoreSchedule::Startup,
                "editor.initialize",
                initialize_editor,
            )
            .add_named_legacy_system(CoreSchedule::Update, "editor.spawn", editor_spawn_system)
            .add_named_legacy_system(CoreSchedule::PostUpdate, "editor.ui", update_editor);
    }
}

fn initialize_editor(state: &mut SystemContext<'_>, _commands: &mut Commands) {
    engine::profiling::init(true);
    log::info!("Editor logic profiling initialized");

    let egui_node = EguiRenderNode::new();
    state
        .globals
        .renderer
        .render_graph
        .nodes
        .push((EGUI_NODE_INDEX, Box::new(egui_node)));
    state.globals.renderer.render_graph.compile(
        &mut state.globals.renderer.render_resources,
        state.globals.renderer.renderer_api.as_mut(),
    );
}

fn update_editor(state: &mut SystemContext<'_>, _commands: &mut Commands) {
    engine::profile_scope!("editor.update");

    let events = {
        let Some(mut reader) = state.world.get_resource_mut::<EditorWindowEventReader>() else {
            return;
        };
        let Some(events) = state
            .world
            .get_resource::<Events<winit::event::WindowEvent>>()
        else {
            return;
        };
        reader.0.read(&events).cloned().collect::<Vec<_>>()
    };

    let mut context = EditorContext {
        world: state.world,
        global_resources: state.globals,
    };
    let taken_node = {
        engine::profile_scope!("editor.egui.take_node");
        context
            .global_resources
            .renderer
            .render_graph
            .take_node(EGUI_NODE_INDEX)
    };
    if let Some(mut taken_node) = taken_node {
        if let Some(egui_node) = taken_node.as_any_mut().downcast_mut::<EguiRenderNode>() {
            engine::profile_scope!("editor.egui.process");
            egui_node.process(&mut context, &events);
        }
        {
            engine::profile_scope!("editor.egui.return_node");
            context
                .global_resources
                .renderer
                .render_graph
                .return_node(taken_node);
        }
    }
}

#[cfg(test)]
mod plugin_tests {
    use super::*;

    #[test]
    fn editor_plugin_initializes_with_the_new_app_world() {
        let mut app = engine::App::new();
        app.add_plugin(engine::PlaxelDefaultPlugin)
            .add_plugin(EditorPlugin);

        app.initialize_schedules();

        assert!(app.world.contains_resource::<EditorSpawnRequests>());
        assert!(
            app.schedules
                .schedules
                .get(&CoreSchedule::PostUpdate)
                .is_some_and(|schedule| schedule.system_accesses().count() >= 2)
        );
    }
}
