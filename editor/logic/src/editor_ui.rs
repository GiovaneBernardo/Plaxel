use crate::EditorContext;
use crate::terrain_editor::{TerrainEditorState, draw_terrain_editor};
use egui::{Color32, RichText, Ui, WidgetText};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use engine::math::vec3;
use engine::reflect::{self, PartialReflect, ReflectMut};
use engine::{
    assets::{
        importer::{AssetPayload, ImportedAsset},
        loader,
        manager::{AssetHeader, AssetType, Uuid},
        material::{Material, MaterialParameter, MaterialResource, MaterialValue},
        serializer,
    },
    core::components::core::TransformComponent,
    ecs::entity::Entity,
};
use game_types::octree::PlanetLodSettings;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Once,
};

const EDITOR_LAYOUT_PATH: &str = "editor_layout.ron";
const EDITOR_LAYOUT_VERSION: u32 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
enum EditorTab {
    Viewport,
    Hierarchy,
    Inspector,
    Console,
    Assets,
    Textures,
    RenderGraph,
    Resources,
    Profiler,
    Timeline,
    Physics,
    Terrain,
}

pub struct EditorUi {
    dock_state: DockState<EditorTab>,
    selected_entity: Option<Entity>,
    maximize_viewport: bool,
    floating_hierarchy: bool,
    floating_inspector: bool,
    floating_profiler: bool,
    performance_expanded: bool,
    profiler_paused: bool,
    paused_profiler_snapshot: Option<engine::profiling::ProfileSnapshot>,
    selected_render_node: Option<engine::renderer::ids::GraphPassId>,
    show_puffin_profiler: bool,
    style_applied: bool,
    last_layout_text: Option<String>,
    last_layout_save_time: f64,
    assets: AssetEditorState,
    terrain: TerrainEditorState,
}

impl EditorUi {
    pub fn new() -> Self {
        if let Some(mut layout) = EditorLayout::load() {
            if layout.version < EDITOR_LAYOUT_VERSION {
                if !dock_has_tab(&layout.dock_state, EditorTab::Assets) {
                    layout.dock_state.push_to_focused_leaf(EditorTab::Assets);
                }
                if !dock_has_tab(&layout.dock_state, EditorTab::Textures) {
                    layout.dock_state.push_to_focused_leaf(EditorTab::Textures);
                }
                if !dock_has_tab(&layout.dock_state, EditorTab::RenderGraph) {
                    layout
                        .dock_state
                        .push_to_focused_leaf(EditorTab::RenderGraph);
                }
                if !dock_has_tab(&layout.dock_state, EditorTab::Terrain) {
                    layout.dock_state.push_to_focused_leaf(EditorTab::Terrain);
                }
                if !dock_has_tab(&layout.dock_state, EditorTab::Resources) {
                    layout.dock_state.push_to_focused_leaf(EditorTab::Resources);
                }
                layout.version = EDITOR_LAYOUT_VERSION;
            }
            let last_layout_text = layout.to_ron();
            return Self {
                dock_state: layout.dock_state,
                selected_entity: None,
                maximize_viewport: layout.maximize_viewport,
                floating_hierarchy: layout.floating_hierarchy,
                floating_inspector: layout.floating_inspector,
                floating_profiler: layout.floating_profiler,
                performance_expanded: false,
                profiler_paused: false,
                paused_profiler_snapshot: None,
                selected_render_node: None,
                show_puffin_profiler: false,
                style_applied: false,
                last_layout_text,
                last_layout_save_time: 0.0,
                assets: AssetEditorState::new(),
                terrain: TerrainEditorState::new(),
            };
        }

        let mut dock_state = DockState::new(vec![EditorTab::Viewport]);
        let surface = dock_state.main_surface_mut();
        let [_viewport, hierarchy] =
            surface.split_left(NodeIndex::root(), 0.20, vec![EditorTab::Hierarchy]);
        let [_viewport, inspector] =
            surface.split_right(NodeIndex::root(), 0.24, vec![EditorTab::Inspector]);
        let [_viewport, bottom] = surface.split_below(
            NodeIndex::root(),
            0.24,
            vec![
                EditorTab::Console,
                EditorTab::Assets,
                EditorTab::Textures,
                EditorTab::RenderGraph,
                EditorTab::Resources,
                EditorTab::Profiler,
                EditorTab::Timeline,
                EditorTab::Physics,
                EditorTab::Terrain,
            ],
        );

        let _ = hierarchy;
        let _ = inspector;
        let _ = bottom;

        Self {
            dock_state,
            selected_entity: None,
            maximize_viewport: false,
            floating_hierarchy: true,
            floating_inspector: true,
            floating_profiler: true,
            performance_expanded: false,
            profiler_paused: false,
            paused_profiler_snapshot: None,
            selected_render_node: None,
            show_puffin_profiler: false,
            style_applied: false,
            last_layout_text: None,
            last_layout_save_time: 0.0,
            assets: AssetEditorState::new(),
            terrain: TerrainEditorState::new(),
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, state: &mut EditorContext<'_>) {
        self.show_impl(ctx, state);
    }

    #[inline(never)]
    fn show_impl(&mut self, ctx: &egui::Context, state: &mut EditorContext<'_>) {
        if !self.style_applied {
            engine::profile_scope!("editor.ui.apply_style");
            let mut apply_style = subsecond::HotFn::current(apply_editor_style);
            apply_style.call((ctx,));
            self.style_applied = true;
        }

        {
            engine::profile_scope!("editor.ui.sanitize_selection");
            let mut sanitize_selection = subsecond::HotFn::current(Self::sanitize_selection);
            sanitize_selection.call((self, state));
        }

        {
            engine::profile_scope!("editor.ui.top_toolbar");
            let mut top_toolbar = subsecond::HotFn::current(Self::top_toolbar);
            top_toolbar.call((self, ctx, state));
        }

        {
            engine::profile_scope!("editor.ui.status_bar");
            let mut status_bar = subsecond::HotFn::current(Self::status_bar);
            status_bar.call((&*self, ctx, &*state));
        }

        if self.maximize_viewport {
            {
                engine::profile_scope!("editor.ui.maximized_viewport");
                let mut show_maximized = subsecond::HotFn::current(Self::show_maximized_viewport);
                show_maximized.call((self, ctx, state));
            }

            self.show_performance_overlay(ctx, state);

            {
                engine::profile_scope!("editor.ui.save_layout");
                let mut save_layout = subsecond::HotFn::current(Self::save_layout_if_changed);
                save_layout.call((self, ctx));
            }
            return;
        }

        self.show_performance_overlay(ctx, state);

        {
            engine::profile_scope!("editor.ui.dock_area");
            egui::CentralPanel::default()
                .frame(egui::Frame::default().fill(Color32::TRANSPARENT))
                .show(ctx, |ui| {
                    let mut viewer = EditorTabViewer {
                        state,
                        selected_entity: &mut self.selected_entity,
                        selected_render_node: &mut self.selected_render_node,
                        show_puffin_profiler: &mut self.show_puffin_profiler,
                        profiler_paused: &mut self.profiler_paused,
                        paused_profiler_snapshot: &mut self.paused_profiler_snapshot,
                        assets: &mut self.assets,
                        terrain: &mut self.terrain,
                    };
                    let style = Style::from_egui(ui.style().as_ref());
                    DockArea::new(&mut self.dock_state)
                        .style(style)
                        .show_inside(ui, &mut viewer);
                });
        }

        {
            engine::profile_scope!("editor.ui.save_layout");
            let mut save_layout = subsecond::HotFn::current(Self::save_layout_if_changed);
            save_layout.call((self, ctx));
        }
    }

    fn top_toolbar(&mut self, ctx: &egui::Context, state: &mut EditorContext<'_>) {
        egui::TopBottomPanel::top("editor_top_toolbar")
            .exact_height(44.0)
            .frame(egui::Frame::default().fill(Color32::from_rgb(24, 27, 31)))
            .show(ctx, |ui| {
                ui.add_space(5.0);
                ui.horizontal_centered(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Plaxel")
                            .strong()
                            .color(Color32::from_rgb(230, 235, 240)),
                    );
                    ui.separator();
                    toolbar_button(ui, "New");
                    toolbar_button(ui, "Open");
                    toolbar_button(ui, "Save");
                    ui.separator();
                    toolbar_button(ui, "Select");
                    toolbar_button(ui, "Move");
                    toolbar_button(ui, "Rotate");
                    toolbar_button(ui, "Scale");
                    ui.separator();
                    self.panels_menu(ui);
                    ui.separator();
                    let play = ui.add_enabled(false, egui::Button::new("Play"));
                    play.on_disabled_hover_text("The simulation starts when the engine opens.");
                    let pause = ui.add_enabled(false, egui::Button::new("Pause"));
                    pause.on_disabled_hover_text("Not wired yet.");
                    ui.separator();
                    ui.label("Sim Speed");
                    let mut speed = 1.0_f32;
                    ui.add_enabled(
                        false,
                        egui::Slider::new(&mut speed, 0.0..=4.0).show_value(false),
                    );
                    ui.label("1.00x");
                    ui.separator();
                    if let Some(scene) = state.active_scene_mut()
                        && let Some(mut lod) =
                            scene.world_mut().get_resource_mut::<PlanetLodSettings>()
                    {
                        ui.label("LOD");
                        ui.add(
                            egui::Slider::new(&mut lod.strength, 0.25..=4.0)
                                .logarithmic(true)
                                .fixed_decimals(2),
                        )
                        .on_hover_text(
                            "Higher values keep detailed terrain farther from the camera",
                        );
                        ui.separator();
                    }
                    ui.toggle_value(&mut self.maximize_viewport, "Maximize Game");
                    ui.toggle_value(&mut self.floating_profiler, "Performance");
                    if self.maximize_viewport {
                        ui.toggle_value(&mut self.floating_hierarchy, "Hierarchy");
                        ui.toggle_value(&mut self.floating_inspector, "Inspector");
                    }
                });
            });
    }

    fn panels_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Panels", |ui| {
            for tab in [
                EditorTab::Hierarchy,
                EditorTab::Inspector,
                EditorTab::Console,
                EditorTab::Assets,
                EditorTab::Textures,
                EditorTab::RenderGraph,
                EditorTab::Resources,
                EditorTab::Profiler,
                EditorTab::Timeline,
                EditorTab::Physics,
                EditorTab::Terrain,
            ] {
                let is_open = self.has_tab(tab);
                if ui
                    .add_enabled(!is_open, egui::Button::new(tab.title_text()))
                    .on_disabled_hover_text("Panel is already open.")
                    .clicked()
                {
                    self.reopen_tab(tab);
                    ui.close();
                }
            }
        });
    }

    fn status_bar(&self, ctx: &egui::Context, state: &EditorContext<'_>) {
        egui::TopBottomPanel::bottom("editor_status_bar")
            .exact_height(28.0)
            .frame(egui::Frame::default().fill(Color32::from_rgb(20, 23, 27)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(8.0);
                    ui.colored_label(Color32::from_rgb(96, 210, 130), "Simulation running");
                    ui.separator();
                    if let Some(scene) = state.active_scene() {
                        let world = scene.world();
                        ui.label(format!("Entities {}", world.entities().alive_count()));
                    } else {
                        ui.label("No active scene");
                    }
                    ui.separator();
                    ui.label("Step 1/240");
                    ui.separator();
                    ui.label("Editor preview");
                });
            });
    }

    fn show_maximized_viewport(&mut self, ctx: &egui::Context, state: &mut EditorContext<'_>) {
        egui::Area::new(egui::Id::new("viewport_overlay_stats"))
            .fixed_pos(egui::pos2(16.0, 58.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(RichText::new("Game View").strong());
                    ui.label("Dock UI hidden");
                });
            });

        if self.floating_hierarchy {
            egui::Window::new("Hierarchy")
                .default_pos(egui::pos2(16.0, 96.0))
                .default_size(egui::vec2(290.0, 520.0))
                .resizable(true)
                .show(ctx, |ui| {
                    draw_hierarchy(ui, state, &mut self.selected_entity);
                });
        }

        if self.floating_inspector {
            egui::Window::new("Inspector")
                .default_pos(egui::pos2(980.0, 96.0))
                .default_size(egui::vec2(350.0, 560.0))
                .resizable(true)
                .show(ctx, |ui| {
                    draw_inspector(ui, state, &mut self.selected_entity);
                });
        }
    }

    fn show_performance_overlay(&mut self, ctx: &egui::Context, state: &EditorContext<'_>) {
        if !self.floating_profiler {
            return;
        }
        let live_snapshot = state.global_resources.profiler_snapshot.clone();
        let mut open = self.floating_profiler;
        let mut expanded = self.performance_expanded;
        egui::Window::new("Performance")
            .id(egui::Id::new("floating_performance_profiler"))
            .open(&mut open)
            .default_pos(egui::pos2(18.0, 92.0))
            .default_size(egui::vec2(430.0, 360.0))
            .resizable(true)
            .show(ctx, |ui| {
                draw_performance_hud(
                    ui,
                    &live_snapshot,
                    &mut expanded,
                    &mut self.profiler_paused,
                    &mut self.paused_profiler_snapshot,
                );
            });
        self.floating_profiler = open;
        self.performance_expanded = expanded;
    }

    fn sanitize_selection(&mut self, state: &EditorContext<'_>) {
        let Some(entity) = self.selected_entity else {
            return;
        };

        let alive = state
            .active_scene()
            .map(|scene| scene.world().entities().contains(entity))
            .unwrap_or(false);

        if !alive {
            self.selected_entity = None;
        }
    }

    fn save_layout_if_changed(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|input| input.time);
        if now - self.last_layout_save_time < 0.75 {
            return;
        }

        let layout = EditorLayout {
            version: EDITOR_LAYOUT_VERSION,
            dock_state: self.dock_state.clone(),
            maximize_viewport: self.maximize_viewport,
            floating_hierarchy: self.floating_hierarchy,
            floating_inspector: self.floating_inspector,
            floating_profiler: self.floating_profiler,
        };

        let Some(text) = layout.to_ron() else {
            return;
        };

        if self.last_layout_text.as_deref() == Some(text.as_str()) {
            return;
        }

        self.last_layout_save_time = now;
        if fs::write(EditorLayout::path(), text.as_bytes()).is_ok() {
            self.last_layout_text = Some(text);
        }
    }

    fn has_tab(&self, target: EditorTab) -> bool {
        self.dock_state
            .iter_surfaces()
            .any(|surface| surface.iter_all_tabs().any(|(_, tab)| *tab == target))
    }

    fn reopen_tab(&mut self, tab: EditorTab) {
        if !self.has_tab(tab) {
            self.dock_state.push_to_focused_leaf(tab);
            self.last_layout_text = None;
        }
    }
}

impl Drop for EditorUi {
    fn drop(&mut self) {
        let layout = EditorLayout {
            version: EDITOR_LAYOUT_VERSION,
            dock_state: self.dock_state.clone(),
            maximize_viewport: self.maximize_viewport,
            floating_hierarchy: self.floating_hierarchy,
            floating_inspector: self.floating_inspector,
            floating_profiler: self.floating_profiler,
        };

        if let Some(text) = layout.to_ron() {
            let _ = fs::write(EditorLayout::path(), text.as_bytes());
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
struct EditorLayout {
    #[serde(default)]
    version: u32,
    dock_state: DockState<EditorTab>,
    maximize_viewport: bool,
    floating_hierarchy: bool,
    floating_inspector: bool,
    #[serde(default = "default_true")]
    floating_profiler: bool,
}

fn default_true() -> bool {
    true
}

fn dock_has_tab(dock_state: &DockState<EditorTab>, target: EditorTab) -> bool {
    dock_state
        .iter_surfaces()
        .any(|surface| surface.iter_all_tabs().any(|(_, tab)| *tab == target))
}

impl EditorLayout {
    fn path() -> PathBuf {
        PathBuf::from(EDITOR_LAYOUT_PATH)
    }

    fn load() -> Option<Self> {
        let text = fs::read_to_string(Self::path()).ok()?;
        ron::from_str(&text).ok()
    }

    fn to_ron(&self) -> Option<String> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).ok()
    }
}

impl EditorTab {
    fn title_text(self) -> &'static str {
        match self {
            EditorTab::Viewport => "Game",
            EditorTab::Hierarchy => "Hierarchy",
            EditorTab::Inspector => "Inspector",
            EditorTab::Console => "Console",
            EditorTab::Assets => "Assets",
            EditorTab::Textures => "Textures",
            EditorTab::RenderGraph => "Render Graph",
            EditorTab::Resources => "Resources",
            EditorTab::Profiler => "Profiler",
            EditorTab::Timeline => "Timeline",
            EditorTab::Physics => "Physics",
            EditorTab::Terrain => "Terrain",
        }
    }
}

struct AssetEditorState {
    current_dir: PathBuf,
    selected_path: Option<PathBuf>,
    material: Option<MaterialEditor>,
    status: Option<String>,
}

impl AssetEditorState {
    fn new() -> Self {
        Self {
            current_dir: project_root(),
            selected_path: None,
            material: None,
            status: None,
        }
    }

    fn select_path(&mut self, path: PathBuf, state: &EditorContext<'_>) {
        self.selected_path = Some(path.clone());
        self.status = None;
        self.material = None;

        if path.is_dir() {
            return;
        }

        if !is_asset_extension(&path, "plxmat") {
            return;
        }

        match MaterialEditor::load(path, state) {
            Ok(editor) => self.material = Some(editor),
            Err(error) => self.status = Some(format!("Unable to load material: {error}")),
        }
    }

    fn assign_texture_to_material(&mut self, texture_uuid: Uuid) {
        let Some(editor) = self.material.as_mut() else {
            self.status = Some("Select a material first.".to_string());
            return;
        };

        let target = editor.texture_pick_target.or_else(|| {
            editor
                .material
                .bindings
                .iter()
                .position(|binding| matches!(binding.resource, MaterialResource::Texture(_)))
        });

        match target {
            Some(index) => {
                editor.material.bindings[index].resource = MaterialResource::Texture(texture_uuid);
                editor.texture_pick_target = Some(index);
                editor.status = Some(format!(
                    "Assigned texture to {}.",
                    editor.material.bindings[index].name
                ));
            }
            None => {
                editor.status = Some("Material has no texture binding.".to_string());
            }
        }
    }
}

struct MaterialEditor {
    path: PathBuf,
    header: AssetHeader,
    material: Material,
    texture_pick_target: Option<usize>,
    status: Option<String>,
}

impl MaterialEditor {
    fn load(path: PathBuf, state: &EditorContext<'_>) -> anyhow::Result<Self> {
        let header = loader::load_header(&path)?;
        let payload = loader::load_payload(&path)?;
        let AssetPayload::Material(mut material) = payload else {
            anyhow::bail!("asset payload is not a material");
        };

        if let Some(loaded) = state
            .global_resources
            .asset_manager
            .get_by_uuid::<Material>(material.uuid)
        {
            material.material_index = loaded.material_index;
        }

        Ok(Self {
            path,
            header,
            material,
            texture_pick_target: None,
            status: None,
        })
    }

    fn save(&mut self, state: &mut EditorContext<'_>) {
        let imported = ImportedAsset {
            header: self.header.clone(),
            payload: AssetPayload::Material(self.material.clone()),
        };

        match serializer::write_imported_asset(&imported, &self.path) {
            Ok(()) => {
                let material_index = state
                    .global_resources
                    .renderer
                    .renderer_api
                    .upload_material_asset(&self.material, Some(self.material.material_index));
                self.material.material_index = material_index;
                state
                    .global_resources
                    .asset_manager
                    .paths
                    .insert(self.path.clone(), self.material.uuid);
                state
                    .global_resources
                    .asset_manager
                    .headers
                    .insert(self.material.uuid, self.header.clone());
                state
                    .global_resources
                    .asset_manager
                    .add_asset::<Material>(self.material.clone());
                self.status = Some("Saved material.".to_string());
            }
            Err(error) => {
                self.status = Some(format!("Save failed: {error}"));
            }
        }
    }
}

struct EditorTabViewer<'a, 'world> {
    state: &'a mut EditorContext<'world>,
    selected_entity: &'a mut Option<Entity>,
    selected_render_node: &'a mut Option<engine::renderer::ids::GraphPassId>,
    show_puffin_profiler: &'a mut bool,
    profiler_paused: &'a mut bool,
    paused_profiler_snapshot: &'a mut Option<engine::profiling::ProfileSnapshot>,
    assets: &'a mut AssetEditorState,
    terrain: &'a mut TerrainEditorState,
}

impl TabViewer for EditorTabViewer<'_, '_> {
    type Tab = EditorTab;

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.title_text().into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab {
            EditorTab::Viewport => {
                engine::profile_scope!("editor.ui.tab.viewport");
                let mut draw = subsecond::HotFn::current(draw_viewport_tab);
                draw.call((ui, &mut *self.state));
            }
            EditorTab::Hierarchy => {
                engine::profile_scope!("editor.ui.tab.hierarchy");
                let mut draw = subsecond::HotFn::current(draw_hierarchy);
                draw.call((ui, &mut *self.state, &mut *self.selected_entity));
            }
            EditorTab::Inspector => {
                engine::profile_scope!("editor.ui.tab.inspector");
                let mut draw = subsecond::HotFn::current(draw_inspector);
                draw.call((ui, &mut *self.state, &mut *self.selected_entity));
            }
            EditorTab::Console => {
                engine::profile_scope!("editor.ui.tab.console");
                let mut draw = subsecond::HotFn::current(draw_console);
                draw.call((ui,));
            }
            EditorTab::Assets => {
                engine::profile_scope!("editor.ui.tab.assets");
                let mut draw = subsecond::HotFn::current(draw_asset_browser);
                draw.call((ui, &mut *self.state, &mut *self.assets));
            }
            EditorTab::Textures => {
                engine::profile_scope!("editor.ui.tab.textures");
                let mut draw = subsecond::HotFn::current(draw_texture_explorer);
                draw.call((ui, &mut *self.state, &mut *self.assets));
            }
            EditorTab::RenderGraph => {
                engine::profile_scope!("editor.ui.tab.render_graph");
                let mut draw = subsecond::HotFn::current(draw_render_graph);
                draw.call((ui, &mut *self.state, &mut *self.selected_render_node));
            }
            EditorTab::Resources => {
                engine::profile_scope!("editor.ui.tab.resources");
                let mut draw = subsecond::HotFn::current(draw_resources);
                draw.call((ui, &mut *self.state));
            }
            EditorTab::Profiler => {
                engine::profile_scope!("editor.ui.tab.profiler");
                let mut draw = subsecond::HotFn::current(draw_profiler);
                draw.call((
                    ui,
                    &mut *self.state,
                    &mut *self.show_puffin_profiler,
                    &mut *self.profiler_paused,
                    &mut *self.paused_profiler_snapshot,
                ));
            }
            EditorTab::Timeline => {
                engine::profile_scope!("editor.ui.tab.timeline");
                let mut draw = subsecond::HotFn::current(draw_timeline);
                draw.call((ui,));
            }
            EditorTab::Physics => {
                engine::profile_scope!("editor.ui.tab.physics");
                let mut draw = subsecond::HotFn::current(draw_physics);
                draw.call((ui,));
            }
            EditorTab::Terrain => {
                engine::profile_scope!("editor.ui.tab.terrain");
                draw_terrain_editor(
                    ui,
                    &mut *self.terrain,
                    &mut *self.state,
                    *self.selected_entity,
                );
            }
        }
    }

    fn clear_background(&self, tab: &Self::Tab) -> bool {
        !matches!(tab, EditorTab::Viewport)
    }

    fn scroll_bars(&self, tab: &Self::Tab) -> [bool; 2] {
        if matches!(tab, EditorTab::Viewport | EditorTab::Terrain) {
            [false, false]
        } else {
            [true, true]
        }
    }

    fn is_closeable(&self, tab: &Self::Tab) -> bool {
        !matches!(tab, EditorTab::Viewport)
    }
}

fn draw_viewport_tab(ui: &mut Ui, state: &mut EditorContext<'_>) {
    ui.add_space(10.0);
    egui::Frame::popup(ui.style()).show(ui, |ui| {
        ui.label(RichText::new("Game").strong());
        ui.label("Rendered behind the dock UI");

        // Update input state of active world, this looks bad and is a hack, but no way I'll refactor it right now, it's not even that bad
        // And why is input state both a global resource and world resource?? makes no sense
        state
            .active_scene()
            .unwrap()
            .world()
            .get_resource_mut::<engine::core::input::InputState>()
            .unwrap()
            .is_mouse_over_game_view = ui.rect_contains_pointer(ui.max_rect());
    });
}

fn draw_hierarchy(
    ui: &mut Ui,
    state: &mut EditorContext<'_>,
    selected_entity: &mut Option<Entity>,
) {
    ui.horizontal(|ui| {
        if ui.button("New Entity").clicked() {
            if let Some(scene) = state.active_scene_mut() {
                *selected_entity = Some(scene.world_mut().spawn());
            }
        }

        if ui.button("Spawn 100").clicked() {
            if let Some(scene) = state.active_scene_mut() {
                let world = scene.world_mut();
                for _ in 0..100 {
                    let entity = world.spawn();
                    world.insert(entity, default_transform());
                }
            }
        }
    });

    ui.separator();

    let Some(scene) = state.active_scene_mut() else {
        ui.label("No active scene.");
        return;
    };

    let world = scene.world_mut();
    let entities: Vec<_> = world.entities().iter_alive().collect();

    egui::ScrollArea::vertical().show(ui, |ui| {
        for entity in entities {
            let selected = *selected_entity == Some(entity);
            let label = format!("Entity {}:{}", entity.index(), entity.generation());
            let response = ui.selectable_label(selected, label);
            if response.clicked() {
                *selected_entity = Some(entity);
            }
        }
    });
}

fn draw_inspector(
    ui: &mut Ui,
    state: &mut EditorContext<'_>,
    selected_entity: &mut Option<Entity>,
) {
    let Some(entity) = *selected_entity else {
        ui.vertical_centered(|ui| {
            ui.add_space(24.0);
            ui.label("Select an entity in the hierarchy.");
        });
        return;
    };

    let Some(scene) = state.active_scene_mut() else {
        ui.label("No active scene.");
        return;
    };

    let world = scene.world_mut();
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("Entity {}", entity.index())).strong());
        ui.label(format!("generation {}", entity.generation()));
    });

    ui.separator();

    ui.horizontal(|ui| {
        if ui.button("Add Transform").clicked() {
            world.insert(entity, default_transform());
        }
        if ui.button("Despawn").clicked() {
            if world.despawn(entity) {
                *selected_entity = None;
            }
        }
    });

    ui.separator();
    world.for_each_reflected_component_mut(entity, |type_name, value| {
        component_header(ui, &display_type_name(type_name));
        draw_reflected_value(ui, value, 0);
        ui.separator();
    });
}

fn draw_resources(ui: &mut Ui, state: &mut EditorContext<'_>) {
    ui.label(RichText::new("Live Resources").strong());
    ui.small("Values are edited directly; ignored runtime fields remain hidden.");
    ui.separator();

    egui::CollapsingHeader::new("Global Resources")
        .default_open(true)
        .show(ui, |ui| {
            state
                .global_resources
                .for_each_reflected_mut(|name, value| reflected_field(ui, name, value, 0));
        });

    ui.separator();
    let Some(scene) = state.active_scene_mut() else {
        ui.label("No active scene.");
        return;
    };
    component_header(ui, "Active World Resources");
    scene.world_mut().for_each_resource_mut(|type_name, value| {
        let label = display_type_name(type_name);
        if let Some(value) = value {
            egui::CollapsingHeader::new(label)
                .default_open(false)
                .show(ui, |ui| {
                    draw_reflected_value(ui, value, 0);
                });
        } else {
            ui.horizontal(|ui| {
                ui.label(label);
                ui.weak("opaque runtime resource");
            });
        }
    });
}

fn display_type_name(type_name: &str) -> String {
    type_name
        .rsplit("::")
        .next()
        .unwrap_or(type_name)
        .trim_end_matches('>')
        .to_string()
}

fn reflected_field(
    ui: &mut Ui,
    label: impl Into<String>,
    value: &mut dyn PartialReflect,
    depth: usize,
) {
    let label = label.into();
    if is_reflected_leaf(value) {
        ui.horizontal(|ui| {
            ui.set_min_width(260.0);
            ui.label(pretty_field_name(&label));
            draw_reflected_value(ui, value, depth + 1);
        });
    } else {
        egui::CollapsingHeader::new(pretty_field_name(&label))
            .default_open(depth == 0)
            .show(ui, |ui| draw_reflected_value(ui, value, depth + 1));
    }
}

fn pretty_field_name(name: &str) -> String {
    let mut result = String::new();
    for (index, part) in name.split('_').filter(|part| !part.is_empty()).enumerate() {
        if index > 0 {
            result.push(' ');
        }
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            result.push(first.to_ascii_uppercase());
            result.extend(chars);
        }
    }
    (!result.is_empty())
        .then_some(result)
        .unwrap_or_else(|| name.to_string())
}

fn is_reflected_leaf(value: &dyn PartialReflect) -> bool {
    value.try_downcast_ref::<bool>().is_some()
        || value.try_downcast_ref::<String>().is_some()
        || value.try_downcast_ref::<f32>().is_some()
        || value.try_downcast_ref::<f64>().is_some()
        || value.try_downcast_ref::<i8>().is_some()
        || value.try_downcast_ref::<i16>().is_some()
        || value.try_downcast_ref::<i32>().is_some()
        || value.try_downcast_ref::<i64>().is_some()
        || value.try_downcast_ref::<u8>().is_some()
        || value.try_downcast_ref::<u16>().is_some()
        || value.try_downcast_ref::<u32>().is_some()
        || value.try_downcast_ref::<u64>().is_some()
        || value.try_downcast_ref::<engine::math::Vec2>().is_some()
        || value.try_downcast_ref::<engine::math::Vec3>().is_some()
        || value.try_downcast_ref::<engine::math::Vec4>().is_some()
        || value.try_downcast_ref::<engine::math::DVec3>().is_some()
        || value.try_downcast_ref::<engine::math::Quat>().is_some()
        || value
            .try_downcast_ref::<engine::reflect::RuntimeCounter>()
            .is_some()
        || matches!(value.reflect_kind(), reflect::ReflectKind::Opaque)
}

fn draw_reflected_value(ui: &mut Ui, value: &mut dyn PartialReflect, depth: usize) -> bool {
    macro_rules! drag_number {
        ($ty:ty, $speed:expr) => {
            if let Some(number) = value.try_downcast_mut::<$ty>() {
                return ui.add(egui::DragValue::new(number).speed($speed)).changed();
            }
        };
    }

    if let Some(value) = value.try_downcast_mut::<bool>() {
        return ui.checkbox(value, "").changed();
    }
    if let Some(value) = value.try_downcast_mut::<String>() {
        return ui.text_edit_singleline(value).changed();
    }
    drag_number!(f32, 0.01);
    drag_number!(f64, 0.01);
    drag_number!(i8, 1.0);
    drag_number!(i16, 1.0);
    drag_number!(i32, 1.0);
    drag_number!(i64, 1.0);
    drag_number!(u8, 1.0);
    drag_number!(u16, 1.0);
    drag_number!(u32, 1.0);
    drag_number!(u64, 1.0);

    macro_rules! vector_editor {
        ($ty:ty, $($field:ident),+ $(,)?) => {
            if let Some(vector) = value.try_downcast_mut::<$ty>() {
                let mut changed = false;
                ui.horizontal(|ui| {
                    $(changed |= ui.add(egui::DragValue::new(&mut vector.$field).speed(0.01).prefix(concat!(stringify!($field), " "))).changed();)+
                });
                return changed;
            }
        };
    }
    vector_editor!(engine::math::Vec2, x, y);
    vector_editor!(engine::math::Vec3, x, y, z);
    vector_editor!(engine::math::Vec4, x, y, z, w);
    vector_editor!(engine::math::DVec3, x, y, z);
    vector_editor!(engine::math::Quat, x, y, z, w);

    if let Some(counter) = value.try_downcast_ref::<engine::reflect::RuntimeCounter>() {
        ui.monospace(counter.get().to_string());
        return false;
    }

    match value.reflect_mut() {
        ReflectMut::Struct(value) => {
            let mut changed = false;
            for index in 0..value.field_len() {
                let name = value.name_at(index).unwrap_or("field").to_string();
                if let Some(field) = value.field_at_mut(index) {
                    reflected_field(ui, name, field, depth);
                    changed = true;
                }
            }
            changed
        }
        ReflectMut::TupleStruct(value) => {
            for index in 0..value.field_len() {
                if let Some(field) = value.field_mut(index) {
                    reflected_field(ui, index.to_string(), field, depth);
                }
            }
            true
        }
        ReflectMut::Tuple(value) => {
            for index in 0..value.field_len() {
                if let Some(field) = value.field_mut(index) {
                    reflected_field(ui, index.to_string(), field, depth);
                }
            }
            true
        }
        ReflectMut::List(value) => {
            for index in 0..value.len() {
                if let Some(item) = value.get_mut(index) {
                    reflected_field(ui, format!("[{index}]"), item, depth);
                }
            }
            true
        }
        ReflectMut::Array(value) => {
            for index in 0..value.len() {
                if let Some(item) = value.get_mut(index) {
                    reflected_field(ui, format!("[{index}]"), item, depth);
                }
            }
            true
        }
        ReflectMut::Enum(value) => {
            let current_variant = value.variant_name().to_string();
            let unit_variants = value
                .get_represented_enum_info()
                .map(|info| {
                    (0..info.variant_len())
                        .filter_map(|index| info.variant_at(index))
                        .filter(|variant| matches!(variant, reflect::enums::VariantInfo::Unit(_)))
                        .map(|variant| variant.name())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if unit_variants.len() > 1 {
                egui::ComboBox::from_id_salt((value.reflect_type_path(), depth))
                    .selected_text(&current_variant)
                    .show_ui(ui, |ui| {
                        for variant in unit_variants {
                            if ui
                                .selectable_label(current_variant == variant, variant)
                                .clicked()
                            {
                                let patch = reflect::enums::DynamicEnum::new(variant, ());
                                let _ = value.try_apply(&patch);
                            }
                        }
                    });
            } else {
                ui.monospace(&current_variant);
            }
            for index in 0..value.field_len() {
                let name = value
                    .name_at(index)
                    .map(str::to_string)
                    .unwrap_or_else(|| index.to_string());
                if let Some(field) = value.field_at_mut(index) {
                    reflected_field(ui, name, field, depth);
                }
            }
            true
        }
        ReflectMut::Map(value) => {
            ui.small(format!("{} map entries", value.len()));
            for (index, (key, entry)) in value.iter().take(64).enumerate() {
                ui.monospace(format!("[{index}] {key:?}: {entry:?}"));
            }
            if value.len() > 64 {
                ui.weak(format!("… {} more entries", value.len() - 64));
            }
            false
        }
        ReflectMut::Set(value) => {
            ui.small(format!("{} set entries", value.len()));
            for (index, entry) in value.iter().take(64).enumerate() {
                ui.monospace(format!("[{index}] {entry:?}"));
            }
            if value.len() > 64 {
                ui.weak(format!("… {} more entries", value.len() - 64));
            }
            false
        }
        ReflectMut::Opaque(value) => {
            ui.monospace(format!(
                "{} (opaque)",
                display_type_name(value.reflect_type_path())
            ));
            false
        }
    }
}

fn draw_console(ui: &mut Ui) {
    static CONSOLE_TAB_CONNECTED: Once = Once::new();
    CONSOLE_TAB_CONNECTED.call_once(|| {
        engine::logging::record_console_entry(
            engine::logging::ConsoleLevel::Info,
            "editor",
            "Console tab connected to log buffer",
        );
    });

    let entries = engine::logging::console_entries();
    let error_count = entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.level,
                engine::logging::ConsoleLevel::Error | engine::logging::ConsoleLevel::Panic
            )
        })
        .count();
    let warn_count = entries
        .iter()
        .filter(|entry| matches!(entry.level, engine::logging::ConsoleLevel::Warn))
        .count();

    ui.horizontal(|ui| {
        ui.label(RichText::new("Console").strong());
        ui.separator();
        ui.label(format!(
            "{} entries  {} warnings  {} errors",
            entries.len(),
            warn_count,
            error_count
        ));
        ui.separator();
        if ui.button("Clear").clicked() {
            engine::logging::clear_console_entries();
        }
    });
    ui.separator();

    if entries.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(24.0);
            ui.label("No log entries yet.");
        });
        return;
    }

    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for entry in entries {
                ui.horizontal_wrapped(|ui| {
                    ui.monospace(format!("#{:04}", entry.sequence));
                    ui.colored_label(
                        console_level_color(entry.level),
                        RichText::new(console_level_label(entry.level)).monospace(),
                    );
                    ui.label(RichText::new(entry.target).color(Color32::from_rgb(150, 165, 180)));
                    ui.label(RichText::new(entry.message).monospace());
                });
            }
        });
}

fn console_level_label(level: engine::logging::ConsoleLevel) -> &'static str {
    match level {
        engine::logging::ConsoleLevel::Trace => "TRACE",
        engine::logging::ConsoleLevel::Debug => "DEBUG",
        engine::logging::ConsoleLevel::Info => "INFO ",
        engine::logging::ConsoleLevel::Warn => "WARN ",
        engine::logging::ConsoleLevel::Error => "ERROR",
        engine::logging::ConsoleLevel::Panic => "PANIC",
    }
}

fn console_level_color(level: engine::logging::ConsoleLevel) -> Color32 {
    match level {
        engine::logging::ConsoleLevel::Trace => Color32::from_rgb(130, 145, 160),
        engine::logging::ConsoleLevel::Debug => Color32::from_rgb(140, 165, 190),
        engine::logging::ConsoleLevel::Info => Color32::from_rgb(120, 200, 150),
        engine::logging::ConsoleLevel::Warn => Color32::from_rgb(230, 185, 90),
        engine::logging::ConsoleLevel::Error | engine::logging::ConsoleLevel::Panic => {
            Color32::from_rgb(235, 105, 95)
        }
    }
}

fn draw_asset_browser(ui: &mut Ui, state: &mut EditorContext<'_>, assets: &mut AssetEditorState) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Assets").strong());
        ui.separator();
        if ui.button("Project Root").clicked() {
            assets.current_dir = project_root();
        }
        if ui
            .add_enabled(
                assets.current_dir.parent().is_some(),
                egui::Button::new("Up"),
            )
            .clicked()
        {
            if let Some(parent) = assets.current_dir.parent() {
                assets.current_dir = parent.to_path_buf();
            }
        }
        ui.label(relative_display(&assets.current_dir));
    });
    ui.separator();

    let current_dir = assets.current_dir.clone();
    ui.columns(2, |columns| {
        columns[0].vertical(|ui| {
            draw_asset_tiles(ui, state, assets, &current_dir);
        });
        columns[1].vertical(|ui| {
            draw_selected_asset_inspector(ui, state, assets);
        });
    });
}

fn draw_asset_tiles(
    ui: &mut Ui,
    state: &mut EditorContext<'_>,
    assets: &mut AssetEditorState,
    current_dir: &Path,
) {
    let mut entries = match fs::read_dir(current_dir) {
        Ok(entries) => entries.filter_map(Result::ok).collect::<Vec<_>>(),
        Err(error) => {
            ui.colored_label(
                Color32::from_rgb(230, 120, 100),
                format!("Unable to read folder: {error}"),
            );
            return;
        }
    };

    entries.sort_by_key(|entry| {
        let path = entry.path();
        (
            !path.is_dir(),
            path.file_name()
                .map(|name| name.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default(),
        )
    });

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for entry in entries {
                let path = entry.path();
                let selected = assets.selected_path.as_ref() == Some(&path);
                let response = ui.add_sized(
                    [126.0, 62.0],
                    egui::Button::new(asset_tile_label(state, &path)).selected(selected),
                );

                if response.double_clicked() && path.is_dir() {
                    assets.current_dir = path;
                } else if response.clicked() {
                    assets.select_path(path, state);
                }
            }
        });
    });
}

fn draw_selected_asset_inspector(
    ui: &mut Ui,
    state: &mut EditorContext<'_>,
    assets: &mut AssetEditorState,
) {
    ui.label(RichText::new("Asset Inspector").strong());
    ui.separator();

    let Some(path) = assets.selected_path.clone() else {
        ui.label("Select an asset or folder.");
        return;
    };

    ui.label(relative_display(&path));
    if path.is_dir() {
        ui.label("Folder");
        return;
    }

    if let Some(message) = &assets.status {
        ui.label(message);
    }

    match assets.material.as_mut() {
        Some(editor) if editor.path == path => draw_material_editor(ui, state, editor),
        _ => draw_generic_asset_info(ui, state, &path),
    }
}

fn draw_generic_asset_info(ui: &mut Ui, state: &EditorContext<'_>, path: &Path) {
    match loader::load_header(path) {
        Ok(header) => {
            inspector_grid(ui, "generic_asset_info", |ui| {
                readonly_row(ui, "Name", &header.name);
                readonly_row(ui, "Type", &format!("{:?}", header.asset_type));
                readonly_row(ui, "Uuid", &header.uuid.to_string());
                readonly_row(ui, "Loaded", &asset_loaded_text(state, &header));
            });
        }
        Err(_) => {
            ui.label("Raw file");
        }
    }
}

fn draw_material_editor(ui: &mut Ui, state: &mut EditorContext<'_>, editor: &mut MaterialEditor) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Material").strong());
        if ui.button("Save").clicked() {
            editor.save(state);
        }
    });

    if let Some(message) = &editor.status {
        ui.label(message);
    }

    ui.separator();
    inspector_grid(ui, "material_core_grid", |ui| {
        readonly_row(ui, "Uuid", &editor.material.uuid.to_string());
        if let Some(pass) = editor
            .material
            .technique
            .pass_mut(engine::renderer::ids::material_passes::FORWARD_OPAQUE)
        {
            text_row(ui, "Shader", &mut pass.pipeline.shader);
        }
        readonly_row(ui, "Gpu Index", &editor.material.material_index.to_string());
    });

    ui.separator();
    ui.label(RichText::new("Bindings").strong());
    for (index, binding) in editor.material.bindings.iter_mut().enumerate() {
        ui.group(|ui| {
            inspector_grid(ui, format!("material_binding_{index}"), |ui| {
                text_row(ui, "Name", &mut binding.name);
                u32_row(ui, "Group", &mut binding.group);
                u32_row(ui, "Binding", &mut binding.binding);
                material_resource_row(ui, "Resource", &mut binding.resource);
            });
            ui.horizontal(|ui| {
                if ui.button("Pick Texture").clicked() {
                    editor.texture_pick_target = Some(index);
                }
                if matches!(binding.resource, MaterialResource::Texture(_))
                    && ui.button("Clear Texture").clicked()
                {
                    binding.resource = MaterialResource::Texture(Uuid::nil());
                }
            });
        });
    }

    ui.separator();
    ui.label(RichText::new("Parameters").strong());
    for (index, parameter) in editor.material.parameters.iter_mut().enumerate() {
        draw_material_parameter(ui, index, parameter);
    }
}

fn draw_texture_explorer(
    ui: &mut Ui,
    state: &mut EditorContext<'_>,
    assets: &mut AssetEditorState,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Loaded Textures").strong());
        ui.separator();
        ui.label("Pick a texture for the selected material binding.");
    });
    ui.separator();

    let textures = loaded_texture_headers(state);
    if textures.is_empty() {
        ui.label("No loaded texture assets found.");
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for header in textures {
                ui.group(|ui| {
                    ui.set_min_width(180.0);
                    ui.label("[TEX]");
                    ui.label(&header.name);
                    ui.small(header.uuid.to_string());
                    if ui.button("Use").clicked() {
                        assets.assign_texture_to_material(header.uuid);
                    }
                });
            }
        });
    });
}

struct RenderNodeSummary {
    index: engine::renderer::ids::GraphPassId,
    name: &'static str,
    enabled: bool,
    input_textures: Vec<&'static str>,
    output_textures: Vec<String>,
    color_attachments: Vec<&'static str>,
    depth_attachment: Option<&'static str>,
}

fn draw_render_graph(
    ui: &mut Ui,
    state: &mut EditorContext<'_>,
    selected_render_node: &mut Option<engine::renderer::ids::GraphPassId>,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Render Graph").strong());
        ui.separator();
        let graph = &state.global_resources.renderer.render_graph;
        ui.label(format!("{} nodes", graph.nodes.len()));
        if !graph.compiled {
            ui.colored_label(Color32::from_rgb(230, 180, 90), "not compiled");
        }
    });
    ui.separator();

    let summaries = render_node_summaries(state);
    if summaries.is_empty() {
        ui.label("No render nodes.");
        return;
    }

    if selected_render_node
        .is_none_or(|selected| !summaries.iter().any(|summary| summary.index == selected))
    {
        *selected_render_node = summaries.first().map(|summary| summary.index);
    }

    ui.columns(2, |columns| {
        columns[0].vertical(|ui| {
            ui.label(RichText::new("Nodes").strong());
            ui.separator();
            for summary in &summaries {
                ui.horizontal(|ui| {
                    let can_disable = summary.index != crate::EGUI_NODE_INDEX;
                    let mut enabled = summary.enabled;
                    let checkbox =
                        ui.add_enabled(can_disable, egui::Checkbox::new(&mut enabled, ""));
                    if checkbox.changed() {
                        state
                            .global_resources
                            .renderer
                            .render_graph
                            .set_node_enabled(summary.index, enabled);
                    }
                    checkbox.on_disabled_hover_text("The editor UI node must stay enabled.");

                    let selected = *selected_render_node == Some(summary.index);
                    let label = format!("#{} {}", summary.index, summary.name);
                    if ui.selectable_label(selected, label).clicked() {
                        *selected_render_node = Some(summary.index);
                    }
                });
            }
        });

        columns[1].vertical(|ui| {
            if let Some(index) = *selected_render_node {
                draw_render_node_inspector(ui, state, index, &summaries);
            }
        });
    });
}

fn render_node_summaries(state: &EditorContext<'_>) -> Vec<RenderNodeSummary> {
    let graph = &state.global_resources.renderer.render_graph;
    graph
        .nodes
        .iter()
        .map(|(index, node)| {
            let descriptor = node.describe_pass();
            RenderNodeSummary {
                index: *index,
                name: descriptor.name,
                enabled: graph.is_node_enabled(*index),
                input_textures: descriptor.input_textures,
                output_textures: descriptor
                    .output_textures
                    .into_iter()
                    .map(output_texture_label)
                    .collect(),
                color_attachments: descriptor
                    .color_attachments
                    .iter()
                    .map(|attachment| attachment.name)
                    .collect(),
                depth_attachment: descriptor
                    .depth_attachment
                    .map(|attachment| attachment.name),
            }
        })
        .collect()
}

fn draw_render_node_inspector(
    ui: &mut Ui,
    state: &mut EditorContext<'_>,
    index: engine::renderer::ids::GraphPassId,
    summaries: &[RenderNodeSummary],
) {
    let Some(summary) = summaries.iter().find(|summary| summary.index == index) else {
        ui.label("Select a render node.");
        return;
    };

    ui.horizontal(|ui| {
        ui.label(RichText::new(summary.name).strong());
        ui.label(format!("node {}", summary.index));
    });
    ui.separator();

    inspector_grid(ui, format!("render_node_core_{index}"), |ui| {
        readonly_row(
            ui,
            "Enabled",
            if summary.enabled { "true" } else { "false" },
        );
        readonly_row(
            ui,
            "Inputs",
            &comma_list(summary.input_textures.iter().copied()),
        );
        readonly_row(
            ui,
            "Outputs",
            &comma_list(summary.output_textures.iter().map(String::as_str)),
        );
        readonly_row(
            ui,
            "Color",
            &comma_list(summary.color_attachments.iter().copied()),
        );
        readonly_row(ui, "Depth", summary.depth_attachment.unwrap_or("none"));
    });

    ui.separator();
    let graph = &mut state.global_resources.renderer.render_graph;
    let Some((_, node)) = graph
        .nodes
        .iter_mut()
        .find(|(node_index, _)| *node_index == index)
    else {
        return;
    };

    component_header(ui, "Editable Fields");
    if let Some(value) = node.reflect_mut() {
        draw_reflected_value(ui, value, 0);
    } else {
        ui.label("This node does not expose editable uniforms yet.");
    }
}

fn output_texture_label(output: engine::renderer::OutputTexture) -> String {
    match output {
        engine::renderer::OutputTexture::Create(slot) => format!("create {}", slot.name),
        engine::renderer::OutputTexture::WriteTo(name) => format!("write {name}"),
    }
}

fn comma_list<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    let text = items.into_iter().collect::<Vec<_>>().join(", ");
    if text.is_empty() {
        "none".to_string()
    } else {
        text
    }
}

fn draw_profiler(
    ui: &mut Ui,
    state: &mut EditorContext<'_>,
    _show_puffin_profiler: &mut bool,
    profiler_paused: &mut bool,
    paused_snapshot: &mut Option<engine::profiling::ProfileSnapshot>,
) {
    let live_snapshot = state.global_resources.profiler_snapshot.clone();

    ui.horizontal(|ui| {
        ui.label(RichText::new("Profiler").strong());
        ui.separator();
        profiler_pause_button(ui, &live_snapshot, profiler_paused, paused_snapshot);
        if ui.button("Capture GPU Frame").clicked() {
            state.global_resources.frame_capturer.request_capture();
        }
        ui.separator();
        ui.label(if *profiler_paused { "paused" } else { "live" });
        ui.separator();
        ui.label(format!(
            "Tracy {}",
            if live_snapshot.tracy_enabled {
                "compiled"
            } else {
                "off"
            }
        ));
        ui.label(format!(
            "Puffin {}",
            if live_snapshot.puffin_enabled {
                "compiled"
            } else {
                "off"
            }
        ));
        ui.separator();
        if let Some(scene) = state.active_scene() {
            ui.label(format!(
                "Entities {}",
                scene.world().entities().alive_count()
            ));
        }
    });
    ui.separator();

    let snapshot = displayed_profiler_snapshot(&live_snapshot, *profiler_paused, paused_snapshot);

    draw_live_cpu_profiler(ui, snapshot);
    ui.separator();
    draw_gpu_profiler(ui, &snapshot.gpu, true);
    ui.separator();
    draw_cpu_profiler(ui, &snapshot.cpu);
    ui.separator();

    if snapshot.frames.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(24.0);
            ui.label("No profiler frames recorded yet.");
        });
        return;
    }

    ui.horizontal(|ui| {
        metric_pill(
            ui,
            "Latest",
            snapshot
                .latest_frame
                .as_ref()
                .map(|frame| ms_text(frame.total_us))
                .unwrap_or_else(|| "0.00 ms".to_string()),
        );
        metric_pill(ui, "Average", ms_text(snapshot.average_frame_us));
        metric_pill(ui, "Max", ms_text(snapshot.max_frame_us));
        metric_pill(ui, "Frames", snapshot.frames.len().to_string());
    });

    ui.add_space(8.0);
    draw_frame_time_graph(ui, &snapshot.frames);
    ui.add_space(8.0);

    ui.columns(2, |columns| {
        columns[0].vertical(|ui| {
            ui.label(RichText::new("Scopes").strong());
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(280.0)
                .show(ui, |ui| {
                    egui::Grid::new("profiler_scope_grid")
                        .num_columns(4)
                        .spacing(egui::vec2(12.0, 4.0))
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Name").strong());
                            ui.label(RichText::new("Calls").strong());
                            ui.label(RichText::new("Total").strong());
                            ui.label(RichText::new("Max").strong());
                            ui.end_row();

                            for scope in snapshot.latest_scopes.iter().take(48) {
                                ui.label(scope.name.as_str());
                                ui.label(scope.calls.to_string());
                                ui.label(ms_text(scope.total_us));
                                ui.label(ms_text(scope.max_us));
                                ui.end_row();
                            }
                        });
                });
        });

        columns[1].vertical(|ui| {
            ui.label(RichText::new("Counters").strong());
            ui.separator();
            if let Some(frame) = &snapshot.latest_frame {
                egui::Grid::new("profiler_counter_grid")
                    .num_columns(2)
                    .spacing(egui::vec2(12.0, 4.0))
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new("Name").strong());
                        ui.label(RichText::new("Value").strong());
                        ui.end_row();
                        for counter in &frame.counters {
                            ui.label(counter.name.as_str());
                            ui.label(format!("{:.0}", counter.value));
                            ui.end_row();
                        }
                    });
            }
        });
    });
}

#[derive(Default)]
struct LiveCoreRow {
    intervals: Vec<(f64, f64)>,
    activities: std::collections::HashMap<String, f64>,
    migrations: u32,
}

fn live_core_rows(
    snapshot: &engine::profiling::ProfileSnapshot,
) -> (f64, std::collections::BTreeMap<u32, LiveCoreRow>) {
    let Some(frame) = snapshot.latest_frame.as_ref() else {
        return (0.0, std::collections::BTreeMap::new());
    };
    let frame_us = frame.total_us.max(1.0);
    let mut rows = std::collections::BTreeMap::<u32, LiveCoreRow>::new();
    for scope in &frame.scopes {
        let Some(core) = scope.processor_start else {
            continue;
        };
        let row = rows.entry(core).or_default();
        let start = scope.start_us.clamp(0.0, frame_us);
        let end = (scope.start_us + scope.duration_us).clamp(start, frame_us);
        if scope.name != "frame.total" && end > start {
            row.intervals.push((start, end));
        }
        *row.activities.entry(scope.name.clone()).or_default() += scope.duration_us;
        if scope.processor_end.is_some_and(|end_core| end_core != core) {
            row.migrations += 1;
        }
    }
    (frame_us, rows)
}

fn interval_union_us(intervals: &mut [(f64, f64)]) -> f64 {
    if intervals.is_empty() {
        return 0.0;
    }
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut total = 0.0;
    let (mut start, mut end) = intervals[0];
    for &(next_start, next_end) in &intervals[1..] {
        if next_start <= end {
            end = end.max(next_end);
        } else {
            total += end - start;
            (start, end) = (next_start, next_end);
        }
    }
    total + end - start
}

fn top_activities(row: &LiveCoreRow, count: usize) -> String {
    let mut activities = row.activities.iter().collect::<Vec<_>>();
    activities.sort_by(|a, b| b.1.total_cmp(a.1));
    let text = activities
        .into_iter()
        .filter(|(name, _)| name.as_str() != "frame.total")
        .take(count)
        .map(|(name, duration)| format!("{} {:.2}ms", compact_scope_name(name), duration / 1000.0))
        .collect::<Vec<_>>()
        .join(" · ");
    if text.is_empty() {
        "idle / uninstrumented".to_string()
    } else {
        text
    }
}

fn compact_scope_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

fn draw_cpu_core_rows(ui: &mut Ui, snapshot: &engine::profiling::ProfileSnapshot, detailed: bool) {
    let (frame_us, mut rows) = live_core_rows(snapshot);
    let logical_cores = std::thread::available_parallelism().map_or(1, usize::from);
    if frame_us <= 0.0 {
        ui.label("No live CPU frame recorded yet.");
        return;
    }

    egui::Grid::new(if detailed {
        "live_cpu_core_grid"
    } else {
        "floating_cpu_core_grid"
    })
    .num_columns(3)
    .spacing(egui::vec2(10.0, 3.0))
    .striped(true)
    .show(ui, |ui| {
        for core in 0..logical_cores {
            let row = rows.entry(core as u32).or_default();
            let busy = interval_union_us(&mut row.intervals).min(frame_us);
            let utilization = (busy / frame_us) as f32;
            ui.monospace(format!("CPU {core:02}"));
            ui.add(
                egui::ProgressBar::new(utilization)
                    .desired_width(if detailed { 150.0 } else { 100.0 })
                    .text(format!("{:>5.1}%", utilization * 100.0)),
            );
            let activity = top_activities(row, if detailed { 4 } else { 2 });
            let label = ui.label(activity);
            if row.migrations > 0 {
                label.on_hover_text(format!(
                    "{} scope(s) migrated to another logical CPU before ending",
                    row.migrations
                ));
            }
            ui.end_row();
        }
    });
}

fn draw_live_cpu_profiler(ui: &mut Ui, snapshot: &engine::profiling::ProfileSnapshot) {
    egui::CollapsingHeader::new(RichText::new("Live CPU cores and threads").strong())
        .default_open(true)
        .show(ui, |ui| {
            ui.small(
                "Updated from the latest frame. Core assignment is observed at scope entry; the OS may migrate threads while a scope runs.",
            );
            draw_cpu_core_rows(ui, snapshot, true);

            let Some(frame) = snapshot.latest_frame.as_ref() else {
                return;
            };
            let mut threads = std::collections::BTreeMap::<
                (String, u64),
                std::collections::HashMap<String, f64>,
            >::new();
            for scope in &frame.scopes {
                *threads
                    .entry((scope.thread_name.clone(), scope.thread_id))
                    .or_default()
                    .entry(scope.name.clone())
                    .or_default() += scope.duration_us;
            }
            egui::CollapsingHeader::new("Thread work this frame")
                .default_open(true)
                .show(ui, |ui| {
                    for ((thread_name, thread_id), activities) in threads {
                        let row = LiveCoreRow {
                            activities,
                            ..Default::default()
                        };
                        ui.horizontal(|ui| {
                            ui.monospace(format!("{thread_name} [{thread_id:08x}]"));
                            ui.label(top_activities(&row, 5));
                        });
                    }
                });
        });
}

fn draw_gpu_profiler(
    ui: &mut Ui,
    snapshot: &engine::profiling::gpu::GpuProfileSnapshot,
    detailed: bool,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Live GPU passes").strong());
        ui.separator();
        ui.label(format!(
            "timestamps {}",
            if snapshot.timestamp_supported {
                "on"
            } else {
                "unsupported"
            }
        ));
        ui.label(format!(
            "pipeline statistics {}",
            if snapshot.pipeline_statistics_supported {
                "on"
            } else {
                "unsupported"
            }
        ));
    });
    let Some(frame) = snapshot.latest_frame.as_ref() else {
        ui.label("Waiting for asynchronous GPU query results.");
        return;
    };
    ui.small(format!(
        "GPU frame {} · summed pass time {:.3} ms · asynchronous readback",
        frame.index, frame.summed_pass_ms
    ));

    egui::Grid::new(if detailed {
        "gpu_pass_profile_grid"
    } else {
        "floating_gpu_pass_profile_grid"
    })
    .num_columns(if detailed { 7 } else { 2 })
    .spacing(egui::vec2(10.0, 3.0))
    .striped(true)
    .show(ui, |ui| {
        ui.label(RichText::new("Pass").strong());
        ui.label(RichText::new("GPU ms").strong());
        if detailed {
            for header in ["Vertex", "Clip in", "Prims out", "Fragment", "Compute"] {
                ui.label(RichText::new(header).strong());
            }
        }
        ui.end_row();
        for pass in &frame.passes {
            ui.label(compact_scope_name(&pass.name))
                .on_hover_text(&pass.name);
            ui.monospace(
                pass.duration_ms
                    .map(|duration| format!("{duration:.3}"))
                    .unwrap_or_else(|| "—".to_string()),
            );
            if detailed {
                let statistics = pass.statistics.unwrap_or_default();
                for value in [
                    statistics.vertex_shader_invocations,
                    statistics.clipper_invocations,
                    statistics.clipper_primitives_out,
                    statistics.fragment_shader_invocations,
                    statistics.compute_shader_invocations,
                ] {
                    ui.monospace(value.to_string());
                }
            }
            ui.end_row();
        }
    });
}

fn draw_performance_hud(
    ui: &mut Ui,
    live_snapshot: &engine::profiling::ProfileSnapshot,
    expanded: &mut bool,
    profiler_paused: &mut bool,
    paused_snapshot: &mut Option<engine::profiling::ProfileSnapshot>,
) {
    ui.horizontal(|ui| {
        profiler_pause_button(ui, live_snapshot, profiler_paused, paused_snapshot);
        ui.separator();
        ui.toggle_value(expanded, if *expanded { "Compact" } else { "Expand" });
        ui.separator();
        let snapshot =
            displayed_profiler_snapshot(live_snapshot, *profiler_paused, paused_snapshot);
        let cpu_ms = snapshot
            .latest_frame
            .as_ref()
            .map_or(0.0, |frame| frame.total_us / 1000.0);
        let gpu_ms = snapshot
            .gpu
            .latest_frame
            .as_ref()
            .map_or(0.0, |frame| frame.summed_pass_ms);
        ui.monospace(format!("CPU {cpu_ms:.2} ms"));
        ui.monospace(format!("GPU Σ {gpu_ms:.2} ms"));
    });
    let snapshot = displayed_profiler_snapshot(live_snapshot, *profiler_paused, paused_snapshot);
    ui.separator();
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.label(RichText::new("Logical CPUs").strong());
        draw_cpu_core_rows(ui, snapshot, *expanded);
        ui.separator();
        draw_gpu_profiler(ui, &snapshot.gpu, *expanded);
    });
}

fn profiler_pause_button(
    ui: &mut Ui,
    live_snapshot: &engine::profiling::ProfileSnapshot,
    paused: &mut bool,
    paused_snapshot: &mut Option<engine::profiling::ProfileSnapshot>,
) {
    if ui
        .button(if *paused { "Resume" } else { "Pause" })
        .clicked()
    {
        *paused = !*paused;
        *paused_snapshot = paused.then(|| live_snapshot.clone());
    }
}

fn displayed_profiler_snapshot<'a>(
    live_snapshot: &'a engine::profiling::ProfileSnapshot,
    paused: bool,
    paused_snapshot: &'a mut Option<engine::profiling::ProfileSnapshot>,
) -> &'a engine::profiling::ProfileSnapshot {
    if paused {
        paused_snapshot.get_or_insert_with(|| live_snapshot.clone())
    } else {
        *paused_snapshot = None;
        live_snapshot
    }
}

fn draw_cpu_profiler(ui: &mut Ui, snapshot: &engine::profiling::cpu::CpuProfileSnapshot) {
    use engine::profiling::cpu::CpuCaptureState;

    egui::CollapsingHeader::new(RichText::new("Automatic CPU Sampling").strong())
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                match snapshot.state {
                    CpuCaptureState::Capturing | CpuCaptureState::Processing => {
                        if ui.button("Stop capture").clicked() {
                            engine::profiling::cpu::stop_capture();
                        }
                    }
                    _ if snapshot.supported => {
                        for seconds in [1, 3, 5] {
                            if ui.button(format!("Capture {seconds}s")).clicked() {
                                if let Err(error) = engine::profiling::cpu::start_capture(
                                    std::time::Duration::from_secs(seconds),
                                ) {
                                    log::error!("Could not start CPU capture: {error}");
                                }
                            }
                        }
                        if snapshot.total_samples > 0 && ui.button("Clear CPU capture").clicked() {
                            engine::profiling::cpu::clear_capture();
                        }
                    }
                    _ => {}
                }

                ui.separator();
                ui.label(snapshot.status.as_str());
            });

            if snapshot.state == CpuCaptureState::Capturing {
                let requested = snapshot.requested_duration.as_secs_f32().max(0.001);
                ui.add(
                    egui::ProgressBar::new(
                        (snapshot.elapsed.as_secs_f32() / requested).clamp(0.0, 1.0),
                    )
                    .show_percentage()
                    .text(format!(
                        "{:.1} / {:.1} seconds",
                        snapshot.elapsed.as_secs_f32(),
                        requested
                    )),
                );
            }

            if snapshot.total_samples == 0 {
                ui.label(
                    "This profiler requires no scope markers. It samples native Rust, crate, \
                     Windows, and driver call stacks wherever matching symbols are available.",
                );
                return;
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                metric_pill(ui, "CPU samples", snapshot.total_samples.to_string());
                metric_pill(ui, "Stacks", snapshot.distinct_stacks.to_string());
                metric_pill(
                    ui,
                    "Duration",
                    format!("{:.2} s", snapshot.elapsed.as_secs_f64()),
                );
                metric_pill(
                    ui,
                    "Est. CPU/frame",
                    format!(
                        "{:.2} ms",
                        estimated_cpu_ms_per_frame(snapshot, snapshot.total_samples)
                    ),
                )
                .response
                .on_hover_text(
                    "Estimated processor time per engine frame across all sampled threads. \
                     This can exceed wall-clock frame time when threads run in parallel.",
                );
                metric_pill(ui, "Frames", snapshot.captured_frames.to_string());
                metric_pill(
                    ui,
                    "Source locations",
                    format!(
                        "{} / {}",
                        snapshot.source_location_addresses, snapshot.symbolized_addresses
                    ),
                )
                .response
                .on_hover_text("Sampled instruction addresses resolved to both a source file and line.");
            });
            ui.add_space(6.0);

            egui::CollapsingHeader::new("Function hotspots (Self CPU)")
                .default_open(true)
                .show(ui, |ui| {
                    egui::ScrollArea::both()
                        .id_salt("cpu_function_hotspots")
                        .max_height(360.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Grid::new("cpu_function_hotspot_grid")
                                .num_columns(6)
                                .striped(true)
                                .spacing(egui::vec2(12.0, 4.0))
                                .show(ui, |ui| {
                                    ui.label(RichText::new("Self").strong());
                                    ui.label(RichText::new("Self ms/f").strong());
                                    ui.label(RichText::new("Total").strong());
                                    ui.label(RichText::new("Total ms/f").strong());
                                    ui.label(RichText::new("Function").strong());
                                    ui.label(RichText::new("Source / module").strong());
                                    ui.end_row();

                                    for hotspot in snapshot.functions.iter().take(300) {
                                        ui.monospace(cpu_percent(
                                            hotspot.self_samples,
                                            snapshot.total_samples,
                                        ));
                                        ui.monospace(format!(
                                            "{:.3}",
                                            estimated_cpu_ms_per_frame(
                                                snapshot,
                                                hotspot.self_samples
                                            )
                                        ));
                                        ui.monospace(cpu_percent(
                                            hotspot.inclusive_samples,
                                            snapshot.total_samples,
                                        ));
                                        ui.monospace(format!(
                                            "{:.3}",
                                            estimated_cpu_ms_per_frame(
                                                snapshot,
                                                hotspot.inclusive_samples
                                            )
                                        ));
                                        ui.label(hotspot.function.as_str())
                                            .on_hover_text(hotspot.function.as_str());
                                        let location = cpu_location(
                                            hotspot.file.as_deref(),
                                            hotspot.line,
                                            &hotspot.module,
                                        );
                                        ui.label(location)
                                            .on_hover_text(hotspot.file.as_deref().unwrap_or(""));
                                        ui.end_row();
                                    }
                                });
                        });
                });

            egui::CollapsingHeader::new("Top-down call tree").show(ui, |ui| {
                egui::ScrollArea::both()
                    .id_salt("cpu_call_tree")
                    .max_height(420.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for node in snapshot.call_tree.iter().take(64) {
                            draw_cpu_call_tree_node(ui, node, snapshot, 0);
                        }
                    });
            });

            egui::CollapsingHeader::new("Bottom-up callers").show(ui, |ui| {
                ui.label(
                    "Starts at the code that was executing, then expands toward the callers \
                     responsible for it.",
                );
                egui::ScrollArea::both()
                    .id_salt("cpu_bottom_up")
                    .max_height(420.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for node in snapshot.bottom_up.iter().take(64) {
                            draw_cpu_call_tree_node(ui, node, snapshot, 0);
                        }
                    });
            });

            egui::CollapsingHeader::new(format!(
                "Hot source lines ({})",
                snapshot.source_lines.len()
            ))
            .show(ui, |ui| {
                if snapshot.source_lines.is_empty() {
                    ui.label(
                        "No source lines were resolved. Use a build with debug information and keep the matching PDB beside the executable or in a Cargo target profile directory.",
                    );
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt("cpu_source_lines")
                    .max_height(320.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("cpu_source_line_grid")
                            .num_columns(5)
                            .striped(true)
                            .spacing(egui::vec2(12.0, 4.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new("Self").strong());
                                ui.label(RichText::new("Self ms/f").strong());
                                ui.label(RichText::new("Total").strong());
                                ui.label(RichText::new("Total ms/f").strong());
                                ui.label(RichText::new("Source line").strong());
                                ui.end_row();
                                for source in snapshot.source_lines.iter().take(300) {
                                    ui.monospace(cpu_percent(
                                        source.self_samples,
                                        snapshot.total_samples,
                                    ));
                                    ui.monospace(format!(
                                        "{:.3}",
                                        estimated_cpu_ms_per_frame(snapshot, source.self_samples)
                                    ));
                                    ui.monospace(cpu_percent(
                                        source.inclusive_samples,
                                        snapshot.total_samples,
                                    ));
                                    ui.monospace(format!(
                                        "{:.3}",
                                        estimated_cpu_ms_per_frame(
                                            snapshot,
                                            source.inclusive_samples
                                        )
                                    ));
                                    ui.label(format!(
                                        "{}:{}",
                                        short_source_path(&source.file),
                                        source.line
                                    ))
                                    .on_hover_text(source.file.as_str());
                                    ui.end_row();
                                }
                            });
                    });
            });

            egui::CollapsingHeader::new(format!(
                "Source-file costs ({})",
                snapshot.source_files.len()
            ))
            .show(ui, |ui| {
                if snapshot.source_files.is_empty() {
                    ui.label(
                        "No source files were resolved. The capture still contains function and module costs.",
                    );
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt("cpu_source_files")
                    .max_height(300.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("cpu_source_file_grid")
                            .num_columns(5)
                            .striped(true)
                            .spacing(egui::vec2(12.0, 4.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new("Self").strong());
                                ui.label(RichText::new("Self ms/f").strong());
                                ui.label(RichText::new("Total").strong());
                                ui.label(RichText::new("Total ms/f").strong());
                                ui.label(RichText::new("Source file").strong());
                                ui.end_row();
                                for source in snapshot.source_files.iter().take(300) {
                                    ui.monospace(cpu_percent(
                                        source.self_samples,
                                        snapshot.total_samples,
                                    ));
                                    ui.monospace(format!(
                                        "{:.3}",
                                        estimated_cpu_ms_per_frame(snapshot, source.self_samples)
                                    ));
                                    ui.monospace(cpu_percent(
                                        source.inclusive_samples,
                                        snapshot.total_samples,
                                    ));
                                    ui.monospace(format!(
                                        "{:.3}",
                                        estimated_cpu_ms_per_frame(
                                            snapshot,
                                            source.inclusive_samples
                                        )
                                    ));
                                    ui.label(short_source_path(&source.file))
                                        .on_hover_text(source.file.as_str());
                                    ui.end_row();
                                }
                            });
                    });
            });

            egui::CollapsingHeader::new("Sampled threads").show(ui, |ui| {
                for thread in &snapshot.threads {
                    ui.horizontal(|ui| {
                        ui.monospace(cpu_percent(thread.samples, snapshot.total_samples));
                        ui.label(format!("Windows thread {}", thread.thread_id));
                    });
                }
            });
        });
}

fn draw_cpu_call_tree_node(
    ui: &mut Ui,
    node: &engine::profiling::cpu::CpuCallTreeNode,
    snapshot: &engine::profiling::cpu::CpuProfileSnapshot,
    depth: usize,
) {
    let mut chain = vec![node];
    while chain.len() + depth < 48 {
        let tail = *chain.last().unwrap();
        if tail.self_samples > 0 || tail.children.len() != 1 {
            break;
        }
        chain.push(&tail.children[0]);
    }
    let tail = *chain.last().unwrap();

    if tail.children.is_empty() || depth + chain.len() >= 48 {
        draw_cpu_call_chain(ui, &chain, snapshot);
        return;
    }

    let label = cpu_call_tree_label(node, snapshot);
    egui::CollapsingHeader::new(label)
        .id_salt((
            depth,
            node.function.as_str(),
            node.inclusive_samples,
            node.self_samples,
        ))
        .show(ui, |ui| {
            if chain.len() > 1 {
                ui.group(|ui| {
                    ui.label(
                        RichText::new(format!("Single path · {} frames", chain.len() - 1))
                            .small()
                            .color(Color32::from_rgb(145, 158, 171)),
                    );
                    for chained_node in chain.iter().skip(1) {
                        draw_cpu_call_tree_row(ui, chained_node, snapshot);
                    }
                });
            }
            for child in tail.children.iter().take(128) {
                draw_cpu_call_tree_node(ui, child, snapshot, depth + chain.len());
            }
        });
}

fn draw_cpu_call_chain(
    ui: &mut Ui,
    chain: &[&engine::profiling::cpu::CpuCallTreeNode],
    snapshot: &engine::profiling::cpu::CpuProfileSnapshot,
) {
    if chain.len() == 1 {
        draw_cpu_call_tree_row(ui, chain[0], snapshot);
        return;
    }

    ui.group(|ui| {
        ui.label(
            RichText::new(format!("Single path · {} frames", chain.len()))
                .small()
                .color(Color32::from_rgb(145, 158, 171)),
        );
        for node in chain {
            draw_cpu_call_tree_row(ui, node, snapshot);
        }
    });
}

fn draw_cpu_call_tree_row(
    ui: &mut Ui,
    node: &engine::profiling::cpu::CpuCallTreeNode,
    snapshot: &engine::profiling::cpu::CpuProfileSnapshot,
) {
    let response = ui.label(cpu_call_tree_label(node, snapshot));
    response.on_hover_text(cpu_call_tree_hover(node));
}

fn cpu_call_tree_label(
    node: &engine::profiling::cpu::CpuCallTreeNode,
    snapshot: &engine::profiling::cpu::CpuProfileSnapshot,
) -> String {
    let location = cpu_location(node.file.as_deref(), node.line, &node.module);
    format!(
        "{} total · {:.3} ms/f   {} self · {:.3} ms/f   {}   — {}",
        cpu_percent(node.inclusive_samples, snapshot.total_samples),
        estimated_cpu_ms_per_frame(snapshot, node.inclusive_samples),
        cpu_percent(node.self_samples, snapshot.total_samples),
        estimated_cpu_ms_per_frame(snapshot, node.self_samples),
        node.function,
        location
    )
}

fn cpu_call_tree_hover(node: &engine::profiling::cpu::CpuCallTreeNode) -> String {
    match (&node.file, node.line) {
        (Some(file), Some(line)) => format!("{}\n{file}:{line}\n{}", node.function, node.module),
        (Some(file), None) => format!("{}\n{file}\n{}", node.function, node.module),
        (None, _) => format!("{}\n{}", node.function, node.module),
    }
}

fn estimated_cpu_ms_per_frame(
    snapshot: &engine::profiling::cpu::CpuProfileSnapshot,
    samples: u64,
) -> f64 {
    samples as f64 * snapshot.sample_interval.as_secs_f64() * 1_000.0
        / snapshot.captured_frames.max(1) as f64
}

fn cpu_percent(samples: u64, total_samples: u64) -> String {
    format!(
        "{:6.2}%",
        samples as f64 * 100.0 / total_samples.max(1) as f64
    )
}

fn cpu_location(file: Option<&str>, line: Option<u32>, module: &str) -> String {
    match (file, line) {
        (Some(file), Some(line)) => format!("{}:{line}", short_source_path(file)),
        (Some(file), None) => short_source_path(file),
        (None, _) if !module.is_empty() => module.to_string(),
        _ => "unknown".to_string(),
    }
}

fn short_source_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    for marker in ["/engine/", "/editor/", "/game/", "/src/"] {
        if let Some(index) = normalized.rfind(marker) {
            return normalized[index + 1..].to_string();
        }
    }
    normalized
}

fn metric_pill(ui: &mut Ui, label: &str, value: String) -> egui::InnerResponse<()> {
    ui.group(|ui| {
        ui.set_min_width(112.0);
        ui.label(RichText::new(label).color(Color32::from_rgb(165, 176, 188)));
        ui.label(RichText::new(value).strong());
    })
}

fn draw_frame_time_graph(ui: &mut Ui, frames: &[engine::profiling::FrameSample]) {
    let desired_size = egui::vec2(ui.available_width(), 96.0);
    let (rect, _) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_rgb(18, 21, 25));

    let max_us = frames
        .iter()
        .map(|frame| frame.total_us)
        .fold(16_666.0_f64, f64::max);
    let bar_width = (rect.width() / frames.len().max(1) as f32).max(1.0);

    for (index, frame) in frames.iter().enumerate() {
        let height = ((frame.total_us / max_us) as f32 * rect.height()).clamp(1.0, rect.height());
        let x0 = rect.left() + index as f32 * bar_width;
        let x1 = (x0 + bar_width - 1.0).min(rect.right());
        let y0 = rect.bottom() - height;
        let color = if frame.total_us > 33_333.0 {
            Color32::from_rgb(230, 105, 95)
        } else if frame.total_us > 16_666.0 {
            Color32::from_rgb(230, 185, 90)
        } else {
            Color32::from_rgb(96, 190, 135)
        };
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, rect.bottom())),
            0.0,
            color,
        );
    }
}

fn ms_text(us: f64) -> String {
    format!("{:.2} ms", us / 1000.0)
}

fn draw_timeline(ui: &mut Ui) {
    ui.label(RichText::new("Timeline").strong());
    ui.separator();
    ui.label("Simulation timeline controls are placeholders.");
}

fn draw_physics(ui: &mut Ui) {
    ui.label(RichText::new("Physics").strong());
    ui.separator();
    ui.label("Physics debug metrics are placeholders.");
}

fn component_header(ui: &mut Ui, title: &str) {
    ui.label(
        RichText::new(title)
            .strong()
            .color(Color32::from_rgb(220, 230, 238)),
    );
}

fn inspector_grid<R>(
    ui: &mut Ui,
    id: impl std::hash::Hash,
    add_rows: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing(egui::vec2(12.0, 6.0))
        .striped(false)
        .show(ui, add_rows)
}

fn field_label(ui: &mut Ui, label: &str) {
    ui.add_sized(
        [92.0, 20.0],
        egui::Label::new(RichText::new(label).color(Color32::from_rgb(185, 194, 202))),
    );
}

fn scalar_row(ui: &mut Ui, label: &str, value: &mut f32, speed: f64) {
    field_label(ui, label);
    drag_value(ui, value, speed, "");
    ui.end_row();
}

fn drag_value(ui: &mut Ui, value: &mut f32, speed: f64, prefix: &'static str) {
    let speed = speed.max(f64::from(value.abs()) * 0.01);
    ui.add_sized(
        [64.0, 20.0],
        egui::DragValue::new(value)
            .speed(speed)
            .prefix(prefix)
            .max_decimals(3),
    );
}

fn readonly_row(ui: &mut Ui, label: &str, value: &str) {
    field_label(ui, label);
    ui.label(value);
    ui.end_row();
}

fn text_row(ui: &mut Ui, label: &str, value: &mut String) {
    field_label(ui, label);
    ui.add_sized([220.0, 20.0], egui::TextEdit::singleline(value));
    ui.end_row();
}

fn u32_row(ui: &mut Ui, label: &str, value: &mut u32) {
    field_label(ui, label);
    ui.add_sized([72.0, 20.0], egui::DragValue::new(value).speed(1.0));
    ui.end_row();
}

fn bool_row(ui: &mut Ui, label: &str, value: &mut bool) {
    field_label(ui, label);
    ui.checkbox(value, "");
    ui.end_row();
}

fn material_resource_row(ui: &mut Ui, label: &str, resource: &mut MaterialResource) {
    field_label(ui, label);
    match resource {
        MaterialResource::Texture(uuid) => {
            let mut text = uuid.to_string();
            if ui
                .add_sized([300.0, 20.0], egui::TextEdit::singleline(&mut text))
                .lost_focus()
            {
                if let Ok(parsed) = Uuid::parse_str(text.trim()) {
                    *uuid = parsed;
                }
            }
        }
        MaterialResource::TextureArray(uuids) => {
            ui.label(format!("TextureArray [{} textures]", uuids.len()));
        }
        MaterialResource::Sampler(_) => {
            ui.label("Sampler");
        }
        MaterialResource::Buffer(uuid) => {
            ui.label(format!("Buffer {uuid}"));
        }
    }
    ui.end_row();
}

fn draw_material_parameter(ui: &mut Ui, index: usize, parameter: &mut MaterialParameter) {
    ui.group(|ui| {
        inspector_grid(ui, format!("material_parameter_{index}"), |ui| {
            text_row(ui, "Name", &mut parameter.name);
            match &mut parameter.value {
                MaterialValue::Float(value) => scalar_row(ui, "Float", value, 0.01),
                MaterialValue::Vec2(value) => float_array_row(ui, "Vec2", value),
                MaterialValue::Vec3(value) => float_array_row(ui, "Vec3", value),
                MaterialValue::Vec4(value) => float_array_row(ui, "Vec4", value),
                MaterialValue::Int(value) => int_row(ui, "Int", value),
                MaterialValue::Uint(value) => u32_row(ui, "Uint", value),
                MaterialValue::Bool(value) => bool_row(ui, "Bool", value),
            }
        });
    });
}

fn float_array_row<const N: usize>(ui: &mut Ui, label: &str, value: &mut [f32; N]) {
    field_label(ui, label);
    ui.horizontal(|ui| {
        for item in value {
            drag_value(ui, item, 0.01, "");
        }
    });
    ui.end_row();
}

fn int_row(ui: &mut Ui, label: &str, value: &mut i32) {
    field_label(ui, label);
    ui.add_sized([72.0, 20.0], egui::DragValue::new(value).speed(1.0));
    ui.end_row();
}

fn asset_tile_label(state: &EditorContext<'_>, path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();

    if path.is_dir() {
        return format!("[DIR]\n{name}");
    }

    if is_compiled_asset(path)
        && let Ok(header) = loader::load_header(path)
    {
        let icon = match header.asset_type {
            AssetType::Material => "[MAT]",
            AssetType::Texture => {
                if state
                    .global_resources
                    .renderer
                    .renderer_api
                    .is_texture_asset_uploaded(header.uuid)
                {
                    "[TEX*]"
                } else {
                    "[TEX]"
                }
            }
            AssetType::Mesh => "[MESH]",
            AssetType::Prefab => "[PREF]",
            AssetType::Audio => "[AUD]",
        };
        return format!("{icon}\n{name}");
    }

    let icon = match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" | "jpg" | "jpeg" | "tga" => "[IMG]",
        "wgsl" => "[SHD]",
        "obj" | "gltf" | "glb" => "[SRC]",
        _ => "[FILE]",
    };
    format!("{icon}\n{name}")
}

fn asset_loaded_text(state: &EditorContext<'_>, header: &AssetHeader) -> String {
    match header.asset_type {
        AssetType::Texture => state
            .global_resources
            .renderer
            .renderer_api
            .is_texture_asset_uploaded(header.uuid)
            .to_string(),
        AssetType::Material => state
            .global_resources
            .asset_manager
            .get_by_uuid::<Material>(header.uuid)
            .is_some()
            .to_string(),
        _ => "unknown".to_string(),
    }
}

fn loaded_texture_headers(state: &EditorContext<'_>) -> Vec<AssetHeader> {
    let mut textures = state
        .global_resources
        .asset_manager
        .headers
        .values()
        .filter(|header| {
            header.asset_type == AssetType::Texture
                && state
                    .global_resources
                    .renderer
                    .renderer_api
                    .is_texture_asset_uploaded(header.uuid)
        })
        .cloned()
        .collect::<Vec<_>>();
    textures.sort_by(|a, b| a.name.cmp(&b.name));
    textures
}

fn is_asset_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|candidate| candidate.to_str())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(extension))
}

fn is_compiled_asset(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "plxmesh" | "plxmat" | "plxtex" | "plax"
            )
        })
}

fn project_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn relative_display(path: &Path) -> String {
    path.strip_prefix(project_root())
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

fn toolbar_button(ui: &mut Ui, label: &str) {
    let _ = ui.add_enabled(false, egui::Button::new(label));
}

fn default_transform() -> TransformComponent {
    TransformComponent {
        position: vec3(0.0, 10.0, 0.0),
        rotation: engine::math::Quat::IDENTITY,
        scale: vec3(1.0, 1.0, 1.0),
        velocity: vec3(0.0, 0.0, 0.0),
    }
}

fn apply_editor_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = Color32::from_rgb(20, 23, 27);
    style.visuals.window_fill = Color32::from_rgb(24, 27, 31);
    style.visuals.extreme_bg_color = Color32::from_rgb(12, 14, 16);
    style.visuals.faint_bg_color = Color32::from_rgb(28, 32, 37);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(38, 43, 49);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(49, 56, 64);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(44, 91, 140);
    style.visuals.selection.bg_fill = Color32::from_rgb(38, 95, 150);
    ctx.set_style(style);
}
