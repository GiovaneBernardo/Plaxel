//! Editor shell: toolbar, status bar, dock layout and the tab dispatch.

use crate::EditorContext;
use crate::panels::{
    assets::{AssetEditorState, draw_asset_browser, draw_texture_explorer},
    console::{ConsoleState, draw_console},
    hierarchy::{HierarchyState, draw_hierarchy},
    inspector::draw_inspector,
    profiler::{ProfilerState, draw_performance_overlay, draw_profiler},
    render_graph::draw_render_graph,
    resources::{ResourcesState, draw_resources},
    viewport::draw_viewport_tab,
};
use crate::terrain_editor::{TerrainEditorState, draw_terrain_editor};
use crate::theme;
use egui::{RichText, Ui, WidgetText};
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};
use engine::ecs::entity::Entity;
use engine::renderer::ids::GraphPassId;
use game_types::octree::PlanetLodSettings;
use std::{fs, path::PathBuf};

const EDITOR_LAYOUT_PATH: &str = "editor_layout.ron";
const EDITOR_LAYOUT_VERSION: u32 = 6;
const LAYOUT_SAVE_INTERVAL: f64 = 0.75;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
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

impl EditorTab {
    const ALL_PANELS: [EditorTab; 11] = [
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
    ];

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

pub struct EditorUi {
    dock_state: DockState<EditorTab>,
    selected_entity: Option<Entity>,
    selected_render_node: Option<GraphPassId>,
    maximize_viewport: bool,
    floating_hierarchy: bool,
    floating_inspector: bool,
    floating_profiler: bool,
    style_applied: bool,
    dock_style: egui_dock::Style,
    last_layout_text: Option<String>,
    last_layout_save_time: f64,
    hierarchy: HierarchyState,
    console: ConsoleState,
    resources: ResourcesState,
    profiler: ProfilerState,
    assets: AssetEditorState,
    terrain: TerrainEditorState,
}

impl EditorUi {
    pub fn new() -> Self {
        let (dock_state, layout) = match EditorLayout::load() {
            Some(mut layout) => {
                if layout.version < EDITOR_LAYOUT_VERSION {
                    for tab in EditorTab::ALL_PANELS {
                        if !dock_has_tab(&layout.dock_state, tab) {
                            layout.dock_state.push_to_focused_leaf(tab);
                        }
                    }
                    layout.version = EDITOR_LAYOUT_VERSION;
                }
                let dock_state = layout.dock_state.clone();
                (dock_state, Some(layout))
            }
            None => (default_dock_state(), None),
        };

        Self {
            dock_state,
            selected_entity: None,
            selected_render_node: None,
            maximize_viewport: layout.as_ref().is_some_and(|l| l.maximize_viewport),
            floating_hierarchy: layout.as_ref().is_none_or(|l| l.floating_hierarchy),
            floating_inspector: layout.as_ref().is_none_or(|l| l.floating_inspector),
            floating_profiler: layout.as_ref().is_none_or(|l| l.floating_profiler),
            style_applied: false,
            dock_style: theme::dock_style(),
            last_layout_text: layout.as_ref().and_then(EditorLayout::to_ron),
            last_layout_save_time: 0.0,
            hierarchy: HierarchyState::default(),
            console: ConsoleState::new(),
            resources: ResourcesState::default(),
            profiler: ProfilerState::new(),
            assets: AssetEditorState::new(),
            terrain: TerrainEditorState::new(),
        }
    }

    pub fn show(&mut self, ui: &mut Ui, state: &mut EditorContext<'_>) {
        if !self.style_applied {
            engine::profile_scope!("editor.ui.apply_style");
            let mut apply_style = subsecond::HotFn::current(theme::apply_editor_style);
            apply_style.call((ui.ctx(),));
            self.style_applied = true;
        }

        self.sanitize_selection(state);

        {
            engine::profile_scope!("editor.ui.top_toolbar");
            let mut top_toolbar = subsecond::HotFn::current(Self::top_toolbar);
            top_toolbar.call((self, ui, state));
        }

        {
            engine::profile_scope!("editor.ui.status_bar");
            let mut status_bar = subsecond::HotFn::current(Self::status_bar);
            status_bar.call((&*self, ui, &*state));
        }

        self.show_floating_windows(ui, state);

        if !self.maximize_viewport {
            engine::profile_scope!("editor.ui.dock_area");
            let dock_style = self.dock_style.clone();
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    let mut viewer = EditorTabViewer {
                        state,
                        selected_entity: &mut self.selected_entity,
                        selected_render_node: &mut self.selected_render_node,
                        hierarchy: &mut self.hierarchy,
                        console: &mut self.console,
                        resources: &mut self.resources,
                        profiler: &mut self.profiler,
                        assets: &mut self.assets,
                        terrain: &mut self.terrain,
                    };
                    DockArea::new(&mut self.dock_state)
                        .style(dock_style)
                        .show_add_buttons(false)
                        .show_leaf_close_all_buttons(false)
                        .show_leaf_collapse_buttons(false)
                        .show_tab_name_on_hover(true)
                        .show_inside(ui, &mut viewer);
                });
        }

        engine::profile_scope!("editor.ui.save_layout");
        self.save_layout_if_changed(ui.ctx());
    }

    fn top_toolbar(&mut self, ui: &mut Ui, state: &mut EditorContext<'_>) {
        egui::Panel::top("editor_top_toolbar")
            .exact_size(theme::TOOLBAR_HEIGHT)
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_DEEP)
                    .inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(
                        RichText::new("◆ PLAXEL")
                            .strong()
                            .size(12.0)
                            .color(theme::ACCENT),
                    );
                    ui.separator();

                    for label in ["New", "Open", "Save"] {
                        ui.add_enabled(false, egui::Button::new(label))
                            .on_disabled_hover_text(format!("{label} scene — not wired yet"));
                    }
                    ui.separator();

                    // Flat, front facing glyphs only: the pictorial emoji in the
                    // bundled fonts are drawn at an angle.
                    for (label, hint) in [
                        ("↖", "Select"),
                        ("✜", "Move"),
                        ("⟳", "Rotate"),
                        ("⛶", "Scale"),
                    ] {
                        ui.add_enabled(false, egui::Button::new(label))
                            .on_disabled_hover_text(format!("{hint} gizmo — not wired yet"));
                    }
                    ui.separator();

                    ui.add_enabled(false, egui::Button::new("▶"))
                        .on_disabled_hover_text("The simulation starts with the engine.");
                    ui.add_enabled(false, egui::Button::new("⏸"))
                        .on_disabled_hover_text("Pausing the simulation is not wired yet.");
                    ui.separator();

                    if let Some(scene) = state.active_scene_mut()
                        && let Some(mut lod) =
                            scene.world_mut().get_resource_mut::<PlanetLodSettings>()
                    {
                        ui.label(RichText::new("LOD").color(theme::TEXT_DIM));
                        ui.add(
                            egui::Slider::new(&mut lod.strength, 0.25..=4.0)
                                .logarithmic(true)
                                .fixed_decimals(2),
                        )
                        .on_hover_text("Higher values keep detailed terrain farther away");
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.toggle_value(&mut self.floating_profiler, "Performance");
                        ui.toggle_value(&mut self.maximize_viewport, "Maximize");
                        if self.maximize_viewport {
                            ui.toggle_value(&mut self.floating_hierarchy, "Hierarchy");
                            ui.toggle_value(&mut self.floating_inspector, "Inspector");
                        }
                        self.panels_menu(ui);
                    });
                });
            });
    }

    fn panels_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Panels", |ui| {
            for tab in EditorTab::ALL_PANELS {
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

    fn status_bar(&self, ui: &mut Ui, state: &EditorContext<'_>) {
        let snapshot = &state.global_resources.profiler_snapshot;
        let cpu_ms = snapshot
            .latest_frame
            .as_ref()
            .map_or(0.0, |frame| frame.total_us / 1000.0);
        let gpu_ms = snapshot
            .gpu
            .latest_frame
            .as_ref()
            .map_or(0.0, |frame| frame.summed_pass_ms);

        egui::Panel::bottom("editor_status_bar")
            .exact_size(theme::STATUS_BAR_HEIGHT)
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_DEEP)
                    .inner_margin(egui::Margin::symmetric(8, 2)),
            )
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("●").color(theme::SUCCESS));
                    ui.label(RichText::new("Simulation running").color(theme::TEXT_DIM));
                    ui.separator();

                    match state.active_scene() {
                        Some(scene) => {
                            ui.label(
                                RichText::new(format!(
                                    "{} entities",
                                    scene.world().entities().alive_count()
                                ))
                                .color(theme::TEXT_DIM),
                            );
                        }
                        None => {
                            ui.label(RichText::new("no active scene").color(theme::WARN));
                        }
                    }

                    if let Some(entity) = self.selected_entity {
                        ui.separator();
                        ui.label(
                            RichText::new(format!(
                                "selected ◈ {}:{}",
                                entity.index(),
                                entity.generation()
                            ))
                            .color(theme::TEXT_DIM),
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{:.0} fps",
                                if cpu_ms > 0.0 { 1000.0 / cpu_ms } else { 0.0 }
                            ))
                            .monospace()
                            .color(theme::TEXT_DIM),
                        );
                        ui.separator();
                        ui.label(
                            RichText::new(format!("GPU {gpu_ms:5.2} ms"))
                                .monospace()
                                .color(theme::ms_color(gpu_ms)),
                        );
                        ui.label(
                            RichText::new(format!("CPU {cpu_ms:5.2} ms"))
                                .monospace()
                                .color(theme::ms_color(cpu_ms)),
                        );
                    });
                });
            });
    }

    fn show_floating_windows(&mut self, ui: &mut Ui, state: &mut EditorContext<'_>) {
        let ctx = ui.ctx().clone();

        if self.floating_profiler {
            engine::profile_scope!("editor.ui.performance_overlay");
            let live = state.global_resources.profiler_snapshot.clone();
            let mut open = true;
            egui::Window::new("Performance")
                .id(egui::Id::new("floating_performance_profiler"))
                .open(&mut open)
                .default_pos(egui::pos2(18.0, 92.0))
                .default_size(egui::vec2(420.0, 340.0))
                .resizable(true)
                .show(&ctx, |ui| {
                    draw_performance_overlay(ui, &live, &mut self.profiler);
                });
            self.floating_profiler = open;
        }

        if !self.maximize_viewport {
            return;
        }

        if self.floating_hierarchy {
            egui::Window::new("Hierarchy")
                .default_pos(egui::pos2(16.0, 96.0))
                .default_size(egui::vec2(300.0, 520.0))
                .resizable(true)
                .show(&ctx, |ui| {
                    draw_hierarchy(ui, state, &mut self.hierarchy, &mut self.selected_entity);
                });
        }

        if self.floating_inspector {
            egui::Window::new("Inspector")
                .default_pos(egui::pos2(980.0, 96.0))
                .default_size(egui::vec2(360.0, 560.0))
                .resizable(true)
                .show(&ctx, |ui| {
                    draw_inspector(ui, state, &mut self.selected_entity);
                });
        }
    }

    fn sanitize_selection(&mut self, state: &EditorContext<'_>) {
        let Some(entity) = self.selected_entity else {
            return;
        };

        let alive = state
            .active_scene()
            .is_some_and(|scene| scene.world().entities().contains(entity));
        if !alive {
            self.selected_entity = None;
        }
    }

    fn save_layout_if_changed(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|input| input.time);
        if now - self.last_layout_save_time < LAYOUT_SAVE_INTERVAL {
            return;
        }
        self.last_layout_save_time = now;

        let Some(text) = self.layout().to_ron() else {
            return;
        };
        if self.last_layout_text.as_deref() == Some(text.as_str()) {
            return;
        }

        if fs::write(EditorLayout::path(), text.as_bytes()).is_ok() {
            self.last_layout_text = Some(text);
        }
    }

    fn layout(&self) -> EditorLayout {
        EditorLayout {
            version: EDITOR_LAYOUT_VERSION,
            dock_state: self.dock_state.clone(),
            maximize_viewport: self.maximize_viewport,
            floating_hierarchy: self.floating_hierarchy,
            floating_inspector: self.floating_inspector,
            floating_profiler: self.floating_profiler,
        }
    }

    fn has_tab(&self, target: EditorTab) -> bool {
        dock_has_tab(&self.dock_state, target)
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
        if let Some(text) = self.layout().to_ron() {
            let _ = fs::write(EditorLayout::path(), text.as_bytes());
        }
    }
}

fn default_dock_state() -> DockState<EditorTab> {
    let mut dock_state = DockState::new(vec![EditorTab::Viewport]);
    let surface = dock_state.main_surface_mut();
    let [_viewport, _hierarchy] =
        surface.split_left(NodeIndex::root(), 0.18, vec![EditorTab::Hierarchy]);
    let [_viewport, _inspector] = surface.split_right(
        NodeIndex::root(),
        0.22,
        vec![EditorTab::Inspector, EditorTab::Resources],
    );
    let [_viewport, _bottom] = surface.split_below(
        NodeIndex::root(),
        0.28,
        vec![
            EditorTab::Console,
            EditorTab::Assets,
            EditorTab::Textures,
            EditorTab::RenderGraph,
            EditorTab::Profiler,
            EditorTab::Timeline,
            EditorTab::Physics,
            EditorTab::Terrain,
        ],
    );
    dock_state
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
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

struct EditorTabViewer<'a, 'world> {
    state: &'a mut EditorContext<'world>,
    selected_entity: &'a mut Option<Entity>,
    selected_render_node: &'a mut Option<GraphPassId>,
    hierarchy: &'a mut HierarchyState,
    console: &'a mut ConsoleState,
    resources: &'a mut ResourcesState,
    profiler: &'a mut ProfilerState,
    assets: &'a mut AssetEditorState,
    terrain: &'a mut TerrainEditorState,
}

impl TabViewer for EditorTabViewer<'_, '_> {
    type Tab = EditorTab;

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.title_text().into()
    }

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(*tab)
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
                draw.call((
                    ui,
                    &mut *self.state,
                    &mut *self.hierarchy,
                    &mut *self.selected_entity,
                ));
            }
            EditorTab::Inspector => {
                engine::profile_scope!("editor.ui.tab.inspector");
                let mut draw = subsecond::HotFn::current(draw_inspector);
                draw.call((ui, &mut *self.state, &mut *self.selected_entity));
            }
            EditorTab::Console => {
                engine::profile_scope!("editor.ui.tab.console");
                let mut draw = subsecond::HotFn::current(draw_console);
                draw.call((ui, &mut *self.console));
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
                draw.call((ui, &mut *self.state, &mut *self.resources));
            }
            EditorTab::Profiler => {
                engine::profile_scope!("editor.ui.tab.profiler");
                let mut draw = subsecond::HotFn::current(draw_profiler);
                draw.call((ui, &mut *self.state, &mut *self.profiler));
            }
            EditorTab::Timeline => {
                engine::profile_scope!("editor.ui.tab.timeline");
                theme::empty_state(ui, "◌", "Simulation timeline controls are not built yet.");
            }
            EditorTab::Physics => {
                engine::profile_scope!("editor.ui.tab.physics");
                theme::empty_state(ui, "⚛", "Physics debug metrics are not built yet.");
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

    /// Panels scroll their own content, which keeps their toolbars pinned.
    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, false]
    }

    fn is_closeable(&self, tab: &Self::Tab) -> bool {
        !matches!(tab, EditorTab::Viewport)
    }
}
