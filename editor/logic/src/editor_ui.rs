use cgmath::vec3;
use egui::{Color32, RichText, Ui, WidgetText};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use engine::{
    core::components::{
        core::TransformComponent,
        physics::{BodyKind, ColliderComponent, ColliderShape, RigidBodyComponent},
        renderer::MeshRendererComponent,
    },
    ecs::entity::Entity,
};
use std::{fs, path::PathBuf};

const EDITOR_LAYOUT_PATH: &str = "editor_layout.ron";

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
enum EditorTab {
    Viewport,
    Hierarchy,
    Inspector,
    Console,
    Profiler,
    Timeline,
    Physics,
}

pub struct EditorUi {
    dock_state: DockState<EditorTab>,
    selected_entity: Option<Entity>,
    maximize_viewport: bool,
    floating_hierarchy: bool,
    floating_inspector: bool,
    style_applied: bool,
    last_layout_text: Option<String>,
    last_layout_save_time: f64,
}

impl EditorUi {
    pub fn new() -> Self {
        if let Some(layout) = EditorLayout::load() {
            let last_layout_text = layout.to_ron();
            return Self {
                dock_state: layout.dock_state,
                selected_entity: None,
                maximize_viewport: layout.maximize_viewport,
                floating_hierarchy: layout.floating_hierarchy,
                floating_inspector: layout.floating_inspector,
                style_applied: false,
                last_layout_text,
                last_layout_save_time: 0.0,
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
                EditorTab::Profiler,
                EditorTab::Timeline,
                EditorTab::Physics,
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
            style_applied: false,
            last_layout_text: None,
            last_layout_save_time: 0.0,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, state: &mut engine::State) {
        if !self.style_applied {
            apply_editor_style(ctx);
            self.style_applied = true;
        }

        self.sanitize_selection(state);
        self.top_toolbar(ctx);
        self.status_bar(ctx, state);

        if self.maximize_viewport {
            self.show_maximized_viewport(ctx, state);
            self.save_layout_if_changed(ctx);
            return;
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(Color32::TRANSPARENT))
            .show(ctx, |ui| {
                let mut viewer = EditorTabViewer {
                    state,
                    selected_entity: &mut self.selected_entity,
                };
                let style = Style::from_egui(ui.style().as_ref());
                DockArea::new(&mut self.dock_state)
                    .style(style)
                    .show_inside(ui, &mut viewer);
            });

        self.save_layout_if_changed(ctx);
    }

    fn top_toolbar(&mut self, ctx: &egui::Context) {
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
                    ui.toggle_value(&mut self.maximize_viewport, "Maximize Game");
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
                EditorTab::Profiler,
                EditorTab::Timeline,
                EditorTab::Physics,
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

    fn status_bar(&self, ctx: &egui::Context, state: &engine::State) {
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

    fn show_maximized_viewport(&mut self, ctx: &egui::Context, state: &mut engine::State) {
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

    fn sanitize_selection(&mut self, state: &engine::State) {
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
            dock_state: self.dock_state.clone(),
            maximize_viewport: self.maximize_viewport,
            floating_hierarchy: self.floating_hierarchy,
            floating_inspector: self.floating_inspector,
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
            dock_state: self.dock_state.clone(),
            maximize_viewport: self.maximize_viewport,
            floating_hierarchy: self.floating_hierarchy,
            floating_inspector: self.floating_inspector,
        };

        if let Some(text) = layout.to_ron() {
            let _ = fs::write(EditorLayout::path(), text.as_bytes());
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
struct EditorLayout {
    dock_state: DockState<EditorTab>,
    maximize_viewport: bool,
    floating_hierarchy: bool,
    floating_inspector: bool,
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
            EditorTab::Profiler => "Profiler",
            EditorTab::Timeline => "Timeline",
            EditorTab::Physics => "Physics",
        }
    }
}

struct EditorTabViewer<'a> {
    state: &'a mut engine::State,
    selected_entity: &'a mut Option<Entity>,
}

impl TabViewer for EditorTabViewer<'_> {
    type Tab = EditorTab;

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.title_text().into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab {
            EditorTab::Viewport => draw_viewport_tab(ui),
            EditorTab::Hierarchy => draw_hierarchy(ui, self.state, self.selected_entity),
            EditorTab::Inspector => draw_inspector(ui, self.state, self.selected_entity),
            EditorTab::Console => draw_console(ui),
            EditorTab::Profiler => draw_profiler(ui, self.state),
            EditorTab::Timeline => draw_timeline(ui),
            EditorTab::Physics => draw_physics(ui),
        }
    }

    fn clear_background(&self, tab: &Self::Tab) -> bool {
        !matches!(tab, EditorTab::Viewport)
    }

    fn scroll_bars(&self, tab: &Self::Tab) -> [bool; 2] {
        if matches!(tab, EditorTab::Viewport) {
            [false, false]
        } else {
            [true, true]
        }
    }

    fn is_closeable(&self, tab: &Self::Tab) -> bool {
        !matches!(tab, EditorTab::Viewport)
    }
}

fn draw_viewport_tab(ui: &mut Ui) {
    ui.add_space(10.0);
    egui::Frame::popup(ui.style()).show(ui, |ui| {
        ui.label(RichText::new("Game").strong());
        ui.label("Rendered behind the dock UI");
    });
}

fn draw_hierarchy(ui: &mut Ui, state: &mut engine::State, selected_entity: &mut Option<Entity>) {
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

fn draw_inspector(ui: &mut Ui, state: &mut engine::State, selected_entity: &mut Option<Entity>) {
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

    if let Some(mut transform) = world.get_mut::<TransformComponent>(entity) {
        component_header(ui, "Transform");
        inspector_grid(ui, "transform_inspector_grid", |ui| {
            vector3_row(ui, "Position", &mut transform.position);
            quaternion_row(ui, "Rotation", &mut transform.rotation);
            vector3_row(ui, "Scale", &mut transform.scale);
            vector3_row(ui, "Velocity", &mut transform.velocity);
        });
        ui.separator();
    }

    if let Some(mut body) = world.get_mut::<RigidBodyComponent>(entity) {
        component_header(ui, "Rigid Body");
        inspector_grid(ui, "rigid_body_inspector_grid", |ui| {
            body_kind_row(ui, &mut body.kind);
            scalar_row(ui, "Mass", &mut body.mass, 0.1);
            vector3_row(ui, "Velocity", &mut body.velocity);
        });
        ui.separator();
    }

    if let Some(mut collider) = world.get_mut::<ColliderComponent>(entity) {
        component_header(ui, "Collider");
        inspector_grid(ui, "collider_inspector_grid", |ui| {
            collider_shape_row(ui, &mut collider.shape);
            scalar_row(ui, "Restitution", &mut collider.restitution, 0.01);
            scalar_row(ui, "Friction", &mut collider.friction, 0.01);
        });
        ui.separator();
    }

    if world.get::<MeshRendererComponent>(entity).is_some() {
        component_header(ui, "Mesh Renderer");
        ui.label("Mesh and material handles are read-only for now.");
        ui.separator();
    }
}

fn draw_console(ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Console").strong());
        ui.separator();
        ui.label("No log stream wired yet.");
    });
    ui.separator();
    ui.label("Logs will appear here.");
}

fn draw_profiler(ui: &mut Ui, state: &engine::State) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Profiler").strong());
        ui.separator();
        if let Some(scene) = state.active_scene() {
            ui.label(format!(
                "Entities {}",
                scene.world().entities().alive_count()
            ));
        }
    });
    ui.separator();
    ui.label("Frame, render, physics, and job timings can be added here.");
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

fn inspector_grid(ui: &mut Ui, id: impl std::hash::Hash, add_rows: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing(egui::vec2(12.0, 6.0))
        .striped(false)
        .show(ui, add_rows);
}

fn field_label(ui: &mut Ui, label: &str) {
    ui.add_sized(
        [92.0, 20.0],
        egui::Label::new(RichText::new(label).color(Color32::from_rgb(185, 194, 202))),
    );
}

fn vector3_row(ui: &mut Ui, label: &str, value: &mut cgmath::Vector3<f32>) {
    field_label(ui, label);
    ui.horizontal(|ui| {
        drag_value(ui, &mut value.x, 0.1, "X ");
        drag_value(ui, &mut value.y, 0.1, "Y ");
        drag_value(ui, &mut value.z, 0.1, "Z ");
    });
    ui.end_row();
}

fn quaternion_row(ui: &mut Ui, label: &str, value: &mut cgmath::Quaternion<f32>) {
    field_label(ui, label);
    ui.horizontal(|ui| {
        drag_value(ui, &mut value.s, 0.01, "W ");
        drag_value(ui, &mut value.v.x, 0.01, "X ");
        drag_value(ui, &mut value.v.y, 0.01, "Y ");
        drag_value(ui, &mut value.v.z, 0.01, "Z ");
    });
    ui.end_row();
}

fn scalar_row(ui: &mut Ui, label: &str, value: &mut f32, speed: f64) {
    field_label(ui, label);
    drag_value(ui, value, speed, "");
    ui.end_row();
}

fn body_kind_row(ui: &mut Ui, value: &mut BodyKind) {
    field_label(ui, "Kind");
    egui::ComboBox::from_id_salt("rigid_body_kind")
        .selected_text(match value {
            BodyKind::Dynamic => "Dynamic",
            BodyKind::Fixed => "Fixed",
            BodyKind::Kinematic => "Kinematic",
        })
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(matches!(value, BodyKind::Dynamic), "Dynamic")
                .clicked()
            {
                *value = BodyKind::Dynamic;
            }
            if ui
                .selectable_label(matches!(value, BodyKind::Fixed), "Fixed")
                .clicked()
            {
                *value = BodyKind::Fixed;
            }
            if ui
                .selectable_label(matches!(value, BodyKind::Kinematic), "Kinematic")
                .clicked()
            {
                *value = BodyKind::Kinematic;
            }
        });
    ui.end_row();
}

fn collider_shape_row(ui: &mut Ui, shape: &mut ColliderShape) {
    match shape {
        ColliderShape::Sphere { radius } => scalar_row(ui, "Sphere Radius", radius, 0.05),
        ColliderShape::Cuboid { half_extents } => {
            vector3_row(ui, "Half Extents", half_extents);
        }
        ColliderShape::Trimesh { vertices, indices } => {
            field_label(ui, "Shape");
            ui.label(format!(
                "Trimesh: {} vertices, {} triangles",
                vertices.len(),
                indices.len()
            ));
            ui.end_row();
        }
    }
}

fn drag_value(ui: &mut Ui, value: &mut f32, speed: f64, prefix: &'static str) {
    ui.add_sized(
        [64.0, 20.0],
        egui::DragValue::new(value)
            .speed(speed)
            .prefix(prefix)
            .max_decimals(3),
    );
}

fn toolbar_button(ui: &mut Ui, label: &str) {
    let _ = ui.add_enabled(false, egui::Button::new(label));
}

fn default_transform() -> TransformComponent {
    TransformComponent {
        position: vec3(0.0, 10.0, 0.0),
        rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
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
