use std::{fs, path::PathBuf};

use egui::{Color32, RichText, TextureHandle, TextureOptions, Ui};
use engine::{
    core::components::core::{CameraComponent, TransformComponent},
    ecs::{entity::Entity, query::Query},
    math::{DVec3, dvec3},
};
use game_types::planet::Planet;
use game_types::terrain::terrain_field::{
    TerrainFieldChannel, TerrainFieldGraph, TerrainFieldLayer, TerrainFieldMask,
    TerrainFieldOperation, TerrainFieldSource, TerrainGraphApplyQueue, TerrainGraphApplyRequest,
    TerrainNoiseDomain, TerrainNoiseKind, TerrainNoiseNode,
};

const HISTORY_LIMIT: usize = 96;

#[derive(Clone, Copy, Debug)]
struct TerrainCameraLocation {
    direction: DVec3,
    altitude: f64,
    preview_surface_height: f64,
    runtime_surface_height: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreviewMode {
    Map,
    Globe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreviewResolution {
    Draft,
    Standard,
    High,
}

impl PreviewResolution {
    fn map_size(self) -> [usize; 2] {
        match self {
            Self::Draft => [256, 128],
            Self::Standard => [512, 256],
            Self::High => [1024, 512],
        }
    }

    fn globe_size(self) -> usize {
        match self {
            Self::Draft => 256,
            Self::Standard => 384,
            Self::High => 640,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Draft => "Draft",
            Self::Standard => "Standard",
            Self::High => "High",
        }
    }
}

pub struct TerrainEditorState {
    graph: TerrainFieldGraph,
    path: Option<PathBuf>,
    selected_layer: Option<u64>,
    preview_channel: TerrainFieldChannel,
    preview_mode: PreviewMode,
    preview_resolution: PreviewResolution,
    map_texture: Option<TextureHandle>,
    globe_texture: Option<TextureHandle>,
    dirty_preview: bool,
    dirty_document: bool,
    auto_preview: bool,
    last_edit_time: f64,
    preview_minimum: f64,
    preview_maximum: f64,
    probe_longitude: f64,
    probe_latitude: f64,
    globe_yaw: f64,
    globe_pitch: f64,
    status: Option<String>,
    undo: Vec<TerrainFieldGraph>,
    redo: Vec<TerrainFieldGraph>,
}

impl TerrainEditorState {
    pub fn new() -> Self {
        let graph = TerrainFieldGraph::default();
        Self {
            selected_layer: graph.layers.first().map(|layer| layer.id),
            graph,
            path: None,
            preview_channel: TerrainFieldChannel::Height,
            preview_mode: PreviewMode::Map,
            preview_resolution: PreviewResolution::Draft,
            map_texture: None,
            globe_texture: None,
            dirty_preview: true,
            dirty_document: false,
            auto_preview: true,
            last_edit_time: 0.0,
            preview_minimum: 0.0,
            preview_maximum: 1.0,
            probe_longitude: 0.0,
            probe_latitude: 0.0,
            globe_yaw: 0.0,
            globe_pitch: 0.15,
            status: None,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    fn new_document(&mut self) {
        self.push_undo(self.graph.clone());
        self.graph = TerrainFieldGraph::default();
        self.path = None;
        self.selected_layer = self.graph.layers.first().map(|layer| layer.id);
        self.dirty_document = false;
        self.dirty_preview = true;
        self.status = Some("Created a new graph".to_string());
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Plaxel terrain graph", &["ron", "plxterrain"])
            .pick_file()
        else {
            return;
        };
        match fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|text| ron::from_str::<TerrainFieldGraph>(&text).map_err(anyhow::Error::from))
        {
            Ok(graph) => {
                let errors = graph.validate();
                if errors.is_empty() {
                    self.push_undo(self.graph.clone());
                    self.selected_layer = graph.layers.first().map(|layer| layer.id);
                    self.graph = graph;
                    self.path = Some(path);
                    self.dirty_document = false;
                    self.dirty_preview = true;
                    self.status = Some("Opened terrain graph".to_string());
                } else {
                    self.status = Some(format!("Graph has {} validation errors", errors.len()));
                }
            }
            Err(error) => self.status = Some(format!("Open failed: {error}")),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn open(&mut self) {
        self.status = Some("Opening terrain files is not available in the web editor".to_string());
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save(&mut self, save_as: bool) {
        let errors = self.graph.validate();
        if !errors.is_empty() {
            self.status = Some(format!(
                "Resolve {} validation errors before saving",
                errors.len()
            ));
            return;
        }
        let path = if save_as || self.path.is_none() {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("Plaxel terrain graph", &["plxterrain", "ron"])
                .set_file_name("planet.plxterrain")
                .save_file()
            else {
                return;
            };
            path
        } else {
            self.path.clone().unwrap()
        };
        let result = ron::ser::to_string_pretty(&self.graph, ron::ser::PrettyConfig::default())
            .map_err(anyhow::Error::from)
            .and_then(|text| write_graph(&path, text.as_bytes()));
        match result {
            Ok(()) => {
                self.path = Some(path);
                self.dirty_document = false;
                self.status = Some("Saved terrain graph".to_string());
            }
            Err(error) => self.status = Some(format!("Save failed: {error}")),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn save(&mut self, _save_as: bool) {
        self.status = Some("Saving terrain files is not available in the web editor".to_string());
    }

    fn push_undo(&mut self, graph: TerrainFieldGraph) {
        if self.undo.last() == Some(&graph) {
            return;
        }
        self.undo.push(graph);
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn undo(&mut self) {
        let Some(graph) = self.undo.pop() else {
            return;
        };
        self.redo.push(self.graph.clone());
        self.graph = graph;
        self.sanitize_selection();
        self.changed(0.0);
    }

    fn redo(&mut self) {
        let Some(graph) = self.redo.pop() else {
            return;
        };
        self.undo.push(self.graph.clone());
        self.graph = graph;
        self.sanitize_selection();
        self.changed(0.0);
    }

    fn sanitize_selection(&mut self) {
        if self
            .selected_layer
            .is_some_and(|id| self.graph.layers.iter().all(|layer| layer.id != id))
        {
            self.selected_layer = self.graph.layers.first().map(|layer| layer.id);
        }
    }

    fn changed(&mut self, time: f64) {
        self.dirty_document = true;
        self.dirty_preview = true;
        self.last_edit_time = time;
    }

    fn rebuild_preview(&mut self, ctx: &egui::Context) {
        let size = self.preview_resolution.map_size();
        let mut values = Vec::with_capacity(size[0] * size[1]);
        let mut minimum = f64::INFINITY;
        let mut maximum = f64::NEG_INFINITY;
        for y in 0..size[1] {
            let latitude = std::f64::consts::FRAC_PI_2
                - std::f64::consts::PI * (y as f64 + 0.5) / size[1] as f64;
            for x in 0..size[0] {
                let longitude = -std::f64::consts::PI
                    + std::f64::consts::TAU * (x as f64 + 0.5) / size[0] as f64;
                let value = self.sample(longitude, latitude);
                minimum = minimum.min(value);
                maximum = maximum.max(value);
                values.push(value);
            }
        }
        if !minimum.is_finite() || !maximum.is_finite() || maximum <= minimum {
            minimum = 0.0;
            maximum = 1.0;
        }
        self.preview_minimum = minimum;
        self.preview_maximum = maximum;
        let pixels = values
            .into_iter()
            .map(|value| self.preview_color(value, minimum, maximum))
            .collect();
        let image = egui::ColorImage::new(size, pixels);
        self.map_texture =
            Some(ctx.load_texture("terrain_map_preview", image, TextureOptions::LINEAR));
        self.rebuild_globe(ctx);
        self.dirty_preview = false;
    }

    fn rebuild_globe(&mut self, ctx: &egui::Context) {
        let size = self.preview_resolution.globe_size();
        let mut pixels = vec![Color32::from_rgb(12, 15, 20); size * size];
        let forward = direction_from_angles(self.globe_yaw, self.globe_pitch);
        let right = forward.cross(DVec3::Y).normalize_or_zero();
        let right = if right.length_squared() < 1e-8 {
            DVec3::X
        } else {
            right
        };
        let up = right.cross(forward).normalize();
        for y in 0..size {
            for x in 0..size {
                let nx = ((x as f64 + 0.5) / size as f64) * 2.0 - 1.0;
                let ny = 1.0 - ((y as f64 + 0.5) / size as f64) * 2.0;
                let radius_squared = nx * nx + ny * ny;
                if radius_squared > 1.0 {
                    continue;
                }
                let nz = (1.0 - radius_squared).sqrt();
                let direction = (right * nx + up * ny + forward * nz).normalize();
                let (longitude, latitude) = angles_from_direction(direction);
                let value = self.sample(longitude, latitude);
                let mut color =
                    self.preview_color(value, self.preview_minimum, self.preview_maximum);
                let lighting = (0.30
                    + 0.70 * direction.dot(dvec3(-0.4, 0.7, 0.6).normalize()).max(0.0))
                    as f32;
                color = Color32::from_rgb(
                    (f32::from(color.r()) * lighting) as u8,
                    (f32::from(color.g()) * lighting) as u8,
                    (f32::from(color.b()) * lighting) as u8,
                );
                pixels[y * size + x] = color;
            }
        }
        let image = egui::ColorImage::new([size, size], pixels);
        self.globe_texture =
            Some(ctx.load_texture("terrain_globe_preview", image, TextureOptions::LINEAR));
    }

    fn sample(&self, longitude: f64, latitude: f64) -> f64 {
        let direction = direction_from_angles(longitude, latitude);
        self.graph.evaluate_direction(direction).channels[self.preview_channel.index()]
    }

    fn preview_color(&self, value: f64, minimum: f64, maximum: f64) -> Color32 {
        if self.preview_channel == TerrainFieldChannel::Height {
            return height_color(value, self.graph.sea_level, minimum, maximum);
        }
        let t = ((value - minimum) / (maximum - minimum)).clamp(0.0, 1.0) as f32;
        turbo_color(t)
    }
}

pub fn draw_terrain_editor(
    ui: &mut Ui,
    state: &mut TerrainEditorState,
    engine_state: &mut crate::EditorContext<'_>,
    selected_entity: Option<Entity>,
) {
    let camera_location = terrain_camera_location(engine_state, selected_entity, &state.graph);
    let time = ui.input(|input| input.time);
    let command = ui.input(|input| input.modifiers.command);
    if command && ui.input(|input| input.key_pressed(egui::Key::S)) {
        state.save(false);
    }
    if command && ui.input(|input| input.key_pressed(egui::Key::Z)) {
        state.undo();
    }
    if command && ui.input(|input| input.key_pressed(egui::Key::Y)) {
        state.redo();
    }

    crate::theme::toolbar(ui, |ui| {
        if ui.button("New").clicked() {
            state.new_document();
        }
        if ui.button("Open").clicked() {
            state.open();
        }
        if ui.button("Save").clicked() {
            state.save(false);
        }
        if ui.button("Save As").clicked() {
            state.save(true);
        }
        ui.separator();
        if ui
            .add_enabled(!state.undo.is_empty(), egui::Button::new("↩ Undo"))
            .clicked()
        {
            state.undo();
        }
        if ui
            .add_enabled(!state.redo.is_empty(), egui::Button::new("↪ Redo"))
            .clicked()
        {
            state.redo();
        }
        ui.separator();
        if ui.button("⟳ Refresh preview").clicked() {
            state.dirty_preview = true;
            state.last_edit_time = 0.0;
        }
        if ui.button("Apply to planet").clicked() {
            apply_to_planet(state, engine_state, selected_entity);
        }
        ui.checkbox(&mut state.auto_preview, "Auto");
        if state.dirty_document {
            crate::theme::tag(ui, "modified", crate::theme::WARN);
        }
    });

    egui::Panel::left("terrain_graph_layers")
        .default_size(390.0)
        .size_range(300.0..=560.0)
        .resizable(true)
        .show_inside(ui, |ui| {
            let before = state.graph.clone();
            draw_document_settings(ui, state);
            ui.separator();
            draw_layer_list(ui, state);
            ui.separator();
            draw_selected_layer(ui, state);
            if state.graph != before {
                state.push_undo(before);
                state.changed(time);
            }
        });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        draw_preview(ui, state, camera_location);
    });

    if state.dirty_preview
        && (!state.auto_preview || time - state.last_edit_time >= 0.20)
        && (state.auto_preview || state.last_edit_time == 0.0)
    {
        state.rebuild_preview(ui.ctx());
    }
}

fn terrain_camera_location(
    engine_state: &mut crate::EditorContext<'_>,
    selected_entity: Option<Entity>,
    graph: &TerrainFieldGraph,
) -> Option<TerrainCameraLocation> {
    let scene = engine_state.active_scene_mut()?;
    let world = scene.world_mut();
    let mut target = selected_entity.filter(|entity| world.get::<Planet>(*entity).is_some());
    if target.is_none() {
        let mut query = Query::<(&Planet,)>::new(world);
        query.for_each(|entity, _| {
            if target.is_none() {
                target = Some(entity);
            }
        });
    }
    let target = target?;
    let planet_position = world.get::<Planet>(target)?.position.as_dvec3();
    let (radius, runtime_graph) = world
        .get::<std::sync::Arc<game_types::terrain::PlanetTerrainConfig>>(target)
        .map_or((0.0, None), |config| {
            (f64::from(config.radius), config.field_graph.clone())
        });

    let mut camera_position = None;
    let mut query = Query::<(&CameraComponent, &TransformComponent)>::new(world);
    query.for_each(|_, (_, transform)| {
        if camera_position.is_none() {
            camera_position = Some(transform.position.as_dvec3());
        }
    });

    let radial = camera_position? - planet_position;
    let distance = radial.length();
    if !distance.is_finite() || distance <= f64::EPSILON {
        return None;
    }
    let direction = radial / distance;
    let sample = graph.evaluate_direction(direction);
    let runtime_surface_height = runtime_graph.map(|graph| {
        let sample = graph.evaluate_direction(direction);
        sample.channels[TerrainFieldChannel::Height.index()]
            - sample.channels[TerrainFieldChannel::Density.index()]
    });
    Some(TerrainCameraLocation {
        direction,
        altitude: distance - radius,
        preview_surface_height: sample.channels[TerrainFieldChannel::Height.index()]
            - sample.channels[TerrainFieldChannel::Density.index()],
        runtime_surface_height,
    })
}

fn apply_to_planet(
    state: &mut TerrainEditorState,
    engine_state: &mut crate::EditorContext<'_>,
    selected_entity: Option<Entity>,
) {
    let errors = state.graph.validate();
    if let Some(error) = errors.first() {
        state.status = Some(format!("Apply failed: {}", error.message));
        return;
    }
    let Some(scene) = engine_state.active_scene_mut() else {
        state.status = Some("Apply failed: no active scene".to_string());
        return;
    };
    let world = scene.world_mut();
    let mut target = selected_entity.filter(|entity| world.get::<Planet>(*entity).is_some());
    if target.is_none() {
        let mut query = Query::<(&Planet,)>::new(world);
        query.for_each(|entity, _| {
            if target.is_none() {
                target = Some(entity);
            }
        });
    }
    let Some(target) = target else {
        state.status = Some("Apply failed: no planet in the active scene".to_string());
        return;
    };
    if world.get_resource::<TerrainGraphApplyQueue>().is_none() {
        world.insert_resource(TerrainGraphApplyQueue::default());
    }
    let mut queue = world
        .get_resource_mut::<TerrainGraphApplyQueue>()
        .expect("terrain graph apply queue must exist");
    queue.requests.push(TerrainGraphApplyRequest {
        target,
        graph: state.graph.clone(),
    });
    state.status = Some("Graph queued for live planet rebuild".to_string());
}

fn draw_document_settings(ui: &mut Ui, state: &mut TerrainEditorState) {
    ui.label(RichText::new("Terrain Field Graph").strong());
    ui.text_edit_singleline(&mut state.graph.name);
    egui::Grid::new("terrain_graph_settings")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Seed");
            ui.add(egui::DragValue::new(&mut state.graph.seed).speed(1));
            ui.end_row();
            ui.label("Radius");
            ui.add(
                egui::DragValue::new(&mut state.graph.radius)
                    .speed(1_000.0)
                    .range(1.0..=1.0e9)
                    .suffix(" m"),
            );
            ui.end_row();
            ui.label("Sea level");
            ui.add(
                egui::DragValue::new(&mut state.graph.sea_level)
                    .speed(10.0)
                    .suffix(" m"),
            );
            ui.end_row();
        });
}

fn draw_layer_list(ui: &mut Ui, state: &mut TerrainEditorState) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Layers").strong());
        if ui.button("Add").clicked() {
            let id = state.graph.next_layer_id();
            state.graph.layers.push(default_layer(id));
            state.selected_layer = Some(id);
        }
        let selected_index = state
            .selected_layer
            .and_then(|id| state.graph.layers.iter().position(|layer| layer.id == id));
        if ui
            .add_enabled(selected_index.is_some(), egui::Button::new("Duplicate"))
            .clicked()
        {
            let mut layer = state.graph.layers[selected_index.unwrap()].clone();
            layer.id = state.graph.next_layer_id();
            layer.name.push_str(" Copy");
            state.selected_layer = Some(layer.id);
            state
                .graph
                .layers
                .insert(selected_index.unwrap() + 1, layer);
        }
        if ui
            .add_enabled(selected_index.is_some(), egui::Button::new("Remove"))
            .clicked()
        {
            state.graph.layers.remove(selected_index.unwrap());
            state.selected_layer = state
                .graph
                .layers
                .get(selected_index.unwrap().saturating_sub(1))
                .map(|layer| layer.id);
        }
    });
    egui::ScrollArea::vertical()
        .id_salt("terrain_layer_list")
        .max_height(240.0)
        .show(ui, |ui| {
            for index in 0..state.graph.layers.len() {
                let id = state.graph.layers[index].id;
                ui.horizontal(|ui| {
                    ui.checkbox(&mut state.graph.layers[index].enabled, "");
                    let selected = state.selected_layer == Some(id);
                    let label = format!(
                        "{}  ·  {}",
                        state.graph.layers[index].name,
                        state.graph.layers[index].target.name()
                    );
                    if ui.selectable_label(selected, label).clicked() {
                        state.selected_layer = Some(id);
                    }
                    if ui.small_button("↑").clicked() && index > 0 {
                        state.graph.layers.swap(index, index - 1);
                    }
                    if ui.small_button("↓").clicked() && index + 1 < state.graph.layers.len() {
                        state.graph.layers.swap(index, index + 1);
                    }
                });
            }
        });
}

fn draw_selected_layer(ui: &mut Ui, state: &mut TerrainEditorState) {
    let Some(index) = state
        .selected_layer
        .and_then(|id| state.graph.layers.iter().position(|layer| layer.id == id))
    else {
        ui.label("Select a layer");
        return;
    };
    let layer = &mut state.graph.layers[index];
    ui.label(RichText::new("Layer Inspector").strong());
    ui.text_edit_singleline(&mut layer.name);
    ui.horizontal(|ui| {
        combo_channel(ui, "Target", &mut layer.target);
        combo_operation(ui, "Operation", &mut layer.operation);
    });
    ui.separator();
    draw_source(ui, &mut layer.source);
    ui.separator();
    let mut masked = layer.mask.is_some();
    if ui.checkbox(&mut masked, "Use mask").changed() {
        layer.mask = masked.then_some(TerrainFieldMask {
            channel: TerrainFieldChannel::Land,
            minimum: 0.0,
            maximum: 1.0,
            smooth: true,
            invert: false,
        });
    }
    if let Some(mask) = &mut layer.mask {
        combo_channel(ui, "Mask channel", &mut mask.channel);
        ui.horizontal(|ui| {
            ui.label("Range");
            ui.add(egui::DragValue::new(&mut mask.minimum).speed(0.01));
            ui.add(egui::DragValue::new(&mut mask.maximum).speed(0.01));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut mask.smooth, "Smooth");
            ui.checkbox(&mut mask.invert, "Invert");
        });
    }
}

fn draw_source(ui: &mut Ui, source: &mut TerrainFieldSource) {
    let mut kind = match source {
        TerrainFieldSource::Constant { .. } => 0,
        TerrainFieldSource::Latitude { .. } => 1,
        TerrainFieldSource::Channel { .. } => 2,
        TerrainFieldSource::Noise(_) => 3,
    };
    egui::ComboBox::from_label("Source")
        .selected_text(["Constant", "Latitude", "Channel", "Noise"][kind])
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut kind, 0, "Constant");
            ui.selectable_value(&mut kind, 1, "Latitude");
            ui.selectable_value(&mut kind, 2, "Channel");
            ui.selectable_value(&mut kind, 3, "Noise");
        });
    let current_kind = match source {
        TerrainFieldSource::Constant { .. } => 0,
        TerrainFieldSource::Latitude { .. } => 1,
        TerrainFieldSource::Channel { .. } => 2,
        TerrainFieldSource::Noise(_) => 3,
    };
    if kind != current_kind {
        *source = match kind {
            0 => TerrainFieldSource::Constant { value: 1.0 },
            1 => TerrainFieldSource::Latitude {
                amplitude: 1.0,
                bias: 0.0,
                absolute: true,
            },
            2 => TerrainFieldSource::Channel {
                channel: TerrainFieldChannel::Land,
                input_min: 0.0,
                input_max: 1.0,
                output_min: 0.0,
                output_max: 1.0,
                smooth: true,
            },
            _ => TerrainFieldSource::Noise(default_noise()),
        };
    }
    match source {
        TerrainFieldSource::Constant { value } => {
            ui.add(egui::DragValue::new(value).speed(0.01));
        }
        TerrainFieldSource::Latitude {
            amplitude,
            bias,
            absolute,
        } => {
            scalar_row(ui, "Amplitude", amplitude, 0.01);
            scalar_row(ui, "Bias", bias, 0.01);
            ui.checkbox(absolute, "Absolute latitude");
        }
        TerrainFieldSource::Channel {
            channel,
            input_min,
            input_max,
            output_min,
            output_max,
            smooth,
        } => {
            combo_channel(ui, "Input", channel);
            range_row(ui, "Input range", input_min, input_max, 0.01);
            range_row(ui, "Output range", output_min, output_max, 0.01);
            ui.checkbox(smooth, "Smooth remap");
        }
        TerrainFieldSource::Noise(noise) => draw_noise(ui, noise),
    }
}

fn draw_noise(ui: &mut Ui, noise: &mut TerrainNoiseNode) {
    egui::ComboBox::from_label("Noise")
        .selected_text(noise.kind.name())
        .show_ui(ui, |ui| {
            for kind in TerrainNoiseKind::ALL {
                ui.selectable_value(&mut noise.kind, kind, kind.name());
            }
        });
    egui::ComboBox::from_label("Domain")
        .selected_text(noise.domain.name())
        .show_ui(ui, |ui| {
            for domain in TerrainNoiseDomain::ALL {
                ui.selectable_value(&mut noise.domain, domain, domain.name());
            }
        });
    scalar_row(ui, "Scale", &mut noise.scale, 10.0);
    scalar_row(ui, "Amplitude", &mut noise.amplitude, 0.1);
    scalar_row(ui, "Bias", &mut noise.bias, 0.01);
    ui.horizontal(|ui| {
        ui.label("Octaves");
        ui.add(egui::DragValue::new(&mut noise.octaves).range(1..=12));
    });
    scalar_row(ui, "Lacunarity", &mut noise.lacunarity, 0.01);
    scalar_row(ui, "Persistence", &mut noise.persistence, 0.01);
    scalar_row(ui, "Warp scale", &mut noise.warp_scale, 0.01);
    scalar_row(ui, "Warp strength", &mut noise.warp_strength, 0.01);
    ui.horizontal(|ui| {
        ui.label("Seed offset");
        ui.add(egui::DragValue::new(&mut noise.seed_offset).speed(1));
    });
}

fn draw_preview(
    ui: &mut Ui,
    state: &mut TerrainEditorState,
    camera_location: Option<TerrainCameraLocation>,
) {
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("terrain_preview_channel")
            .selected_text(state.preview_channel.name())
            .show_ui(ui, |ui| {
                for channel in TerrainFieldChannel::ALL {
                    if ui
                        .selectable_value(&mut state.preview_channel, channel, channel.name())
                        .changed()
                    {
                        state.dirty_preview = true;
                        state.last_edit_time = 0.0;
                    }
                }
            });
        ui.selectable_value(&mut state.preview_mode, PreviewMode::Map, "Map");
        ui.selectable_value(&mut state.preview_mode, PreviewMode::Globe, "Globe");
        egui::ComboBox::from_id_salt("terrain_preview_resolution")
            .selected_text(state.preview_resolution.name())
            .show_ui(ui, |ui| {
                for resolution in [
                    PreviewResolution::Draft,
                    PreviewResolution::Standard,
                    PreviewResolution::High,
                ] {
                    if ui
                        .selectable_value(
                            &mut state.preview_resolution,
                            resolution,
                            resolution.name(),
                        )
                        .changed()
                    {
                        state.dirty_preview = true;
                        state.last_edit_time = 0.0;
                    }
                }
            });
        ui.label(format!(
            "Range {:.3} … {:.3}",
            state.preview_minimum, state.preview_maximum
        ));
    });
    if let Some(camera) = camera_location {
        let (longitude, latitude) = angles_from_direction(camera.direction);
        let runtime = camera
            .runtime_surface_height
            .map_or("not applied".to_string(), |height| {
                format!("{height:+.1} m")
            });
        let agl = camera
            .runtime_surface_height
            .map_or("unknown".to_string(), |height| {
                format!("{:.1} m", camera.altitude - height)
            });
        ui.colored_label(
            Color32::from_rgb(255, 220, 70),
            format!(
                "Camera {:.2}° {:.2}° | AGL {agl} | preview {:+.1} m | runtime {runtime}",
                longitude.to_degrees(),
                latitude.to_degrees(),
                camera.preview_surface_height,
            ),
        );
    } else {
        ui.weak("Camera position unavailable");
    }
    if state.preview_mode == PreviewMode::Globe {
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Slider::new(
                        &mut state.globe_yaw,
                        -std::f64::consts::PI..=std::f64::consts::PI,
                    )
                    .text("Yaw"),
                )
                .changed()
                || ui
                    .add(egui::Slider::new(&mut state.globe_pitch, -1.5..=1.5).text("Pitch"))
                    .changed()
            {
                state.rebuild_globe(ui.ctx());
            }
        });
    }
    ui.separator();
    let available = ui.available_size();
    let response = match state.preview_mode {
        PreviewMode::Map => state.map_texture.as_ref().map(|texture| {
            let ratio = 2.0;
            let size = fit_size(available, ratio);
            ui.add(egui::Image::new((texture.id(), size)).sense(egui::Sense::click()))
        }),
        PreviewMode::Globe => state.globe_texture.as_ref().map(|texture| {
            let size = fit_size(available, 1.0);
            ui.add(egui::Image::new((texture.id(), size)).sense(egui::Sense::click()))
        }),
    };
    if let Some(response) = response {
        if response.clicked()
            && state.preview_mode == PreviewMode::Map
            && let Some(position) = response.interact_pointer_pos()
        {
            let uv = (position - response.rect.min) / response.rect.size();
            state.probe_longitude = -std::f64::consts::PI + std::f64::consts::TAU * f64::from(uv.x);
            state.probe_latitude =
                std::f64::consts::FRAC_PI_2 - std::f64::consts::PI * f64::from(uv.y);
        }
        if let Some(camera) = camera_location {
            draw_camera_marker(ui, response.rect, state, camera.direction);
        }
    }
    ui.separator();
    draw_probe(ui, state);
    let errors = state.graph.validate();
    if errors.is_empty() {
        ui.colored_label(Color32::from_rgb(96, 210, 130), "Graph valid");
    } else {
        ui.colored_label(
            Color32::from_rgb(235, 100, 90),
            format!("{} validation errors", errors.len()),
        );
        for error in errors.iter().take(8) {
            ui.label(format!(
                "{}: {}",
                error
                    .layer_id
                    .map_or("Graph".to_string(), |id| format!("Layer {id}")),
                error.message
            ));
        }
    }
    if let Some(status) = &state.status {
        ui.label(status);
    }
}

fn draw_camera_marker(ui: &Ui, rect: egui::Rect, state: &TerrainEditorState, direction: DVec3) {
    let position = match state.preview_mode {
        PreviewMode::Map => {
            let (longitude, latitude) = angles_from_direction(direction);
            let u = ((longitude + std::f64::consts::PI) / std::f64::consts::TAU) as f32;
            let v = ((std::f64::consts::FRAC_PI_2 - latitude) / std::f64::consts::PI) as f32;
            rect.min + egui::vec2(rect.width() * u, rect.height() * v)
        }
        PreviewMode::Globe => {
            let forward = direction_from_angles(state.globe_yaw, state.globe_pitch);
            if direction.dot(forward) < 0.0 {
                return;
            }
            let right = forward.cross(DVec3::Y).normalize_or_zero();
            let right = if right.length_squared() < 1e-8 {
                DVec3::X
            } else {
                right
            };
            let up = right.cross(forward).normalize();
            rect.center()
                + egui::vec2(
                    direction.dot(right) as f32 * rect.width() * 0.5,
                    -direction.dot(up) as f32 * rect.height() * 0.5,
                )
        }
    };
    let painter = ui.painter();
    painter.circle_filled(position, 8.0, Color32::from_black_alpha(220));
    painter.circle_filled(position, 5.0, Color32::from_rgb(255, 220, 40));
    painter.circle_stroke(position, 8.0, egui::Stroke::new(1.5, Color32::WHITE));
}

fn draw_probe(ui: &mut Ui, state: &TerrainEditorState) {
    let direction = direction_from_angles(state.probe_longitude, state.probe_latitude);
    let sample = state.graph.evaluate_direction(direction);
    ui.label(RichText::new("Probe").strong());
    ui.label(format!(
        "Longitude {:.2}°  Latitude {:.2}°",
        state.probe_longitude.to_degrees(),
        state.probe_latitude.to_degrees()
    ));
    egui::Grid::new("terrain_probe_values")
        .num_columns(4)
        .striped(true)
        .show(ui, |ui| {
            for (index, channel) in TerrainFieldChannel::ALL.into_iter().enumerate() {
                ui.label(channel.name());
                ui.monospace(format!("{:.3}", sample.channels[channel.index()]));
                if index % 2 == 1 {
                    ui.end_row();
                }
            }
        });
}

fn combo_channel(ui: &mut Ui, label: &str, value: &mut TerrainFieldChannel) {
    egui::ComboBox::from_label(label)
        .selected_text(value.name())
        .show_ui(ui, |ui| {
            for channel in TerrainFieldChannel::ALL {
                ui.selectable_value(value, channel, channel.name());
            }
        });
}

fn combo_operation(ui: &mut Ui, label: &str, value: &mut TerrainFieldOperation) {
    egui::ComboBox::from_label(label)
        .selected_text(value.name())
        .show_ui(ui, |ui| {
            for operation in TerrainFieldOperation::ALL {
                ui.selectable_value(value, operation, operation.name());
            }
        });
}

fn scalar_row(ui: &mut Ui, label: &str, value: &mut f64, speed: f64) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).speed(speed));
    });
}

fn range_row(ui: &mut Ui, label: &str, minimum: &mut f64, maximum: &mut f64, speed: f64) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(minimum).speed(speed));
        ui.add(egui::DragValue::new(maximum).speed(speed));
    });
}

fn default_layer(id: u64) -> TerrainFieldLayer {
    TerrainFieldLayer {
        id,
        name: format!("Layer {id}"),
        enabled: true,
        target: TerrainFieldChannel::Height,
        operation: TerrainFieldOperation::Add,
        source: TerrainFieldSource::Noise(default_noise()),
        mask: None,
    }
}

fn default_noise() -> TerrainNoiseNode {
    TerrainNoiseNode {
        kind: TerrainNoiseKind::Fbm,
        domain: TerrainNoiseDomain::SurfaceMeters,
        scale: 10_000.0,
        amplitude: 100.0,
        bias: 0.0,
        octaves: 4,
        lacunarity: 2.03,
        persistence: 0.5,
        warp_scale: 1.0,
        warp_strength: 0.0,
        seed_offset: 0,
    }
}

fn write_graph(path: &PathBuf, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}

fn direction_from_angles(longitude: f64, latitude: f64) -> DVec3 {
    let latitude_cos = latitude.cos();
    dvec3(
        latitude_cos * longitude.cos(),
        latitude.sin(),
        latitude_cos * longitude.sin(),
    )
}

fn angles_from_direction(direction: DVec3) -> (f64, f64) {
    (
        direction.z.atan2(direction.x),
        direction.y.clamp(-1.0, 1.0).asin(),
    )
}

fn fit_size(available: egui::Vec2, ratio: f32) -> egui::Vec2 {
    let width = available.x.max(1.0);
    let height = (width / ratio).min(available.y.max(1.0));
    egui::vec2(height * ratio, height)
}

fn height_color(value: f64, sea_level: f64, minimum: f64, maximum: f64) -> Color32 {
    if value <= sea_level {
        let t =
            ((value - minimum) / (sea_level - minimum).max(f64::EPSILON)).clamp(0.0, 1.0) as f32;
        return lerp_color(
            Color32::from_rgb(8, 24, 64),
            Color32::from_rgb(45, 125, 190),
            t,
        );
    }
    let t = ((value - sea_level) / (maximum - sea_level).max(f64::EPSILON)).clamp(0.0, 1.0) as f32;
    if t < 0.32 {
        lerp_color(
            Color32::from_rgb(56, 112, 58),
            Color32::from_rgb(126, 142, 72),
            t / 0.32,
        )
    } else if t < 0.72 {
        lerp_color(
            Color32::from_rgb(126, 142, 72),
            Color32::from_rgb(112, 92, 75),
            (t - 0.32) / 0.40,
        )
    } else {
        lerp_color(
            Color32::from_rgb(112, 92, 75),
            Color32::WHITE,
            (t - 0.72) / 0.28,
        )
    }
}

fn turbo_color(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let red = (34.61
        + t * (1172.33 + t * (-10793.56 + t * (33300.12 + t * (-38394.49 + t * 14825.05)))))
        / 255.0;
    let green = (23.31
        + t * (557.33 + t * (1225.33 + t * (-3574.96 + t * (1073.77 + t * 707.56)))))
        / 255.0;
    let blue = (27.2
        + t * (3211.1 + t * (-15327.97 + t * (27814.0 + t * (-22569.18 + t * 6838.66)))))
        / 255.0;
    Color32::from_rgb(
        (red.clamp(0.0, 1.0) * 255.0) as u8,
        (green.clamp(0.0, 1.0) * 255.0) as u8,
        (blue.clamp(0.0, 1.0) * 255.0) as u8,
    )
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    Color32::from_rgb(
        (f32::from(a.r()) + (f32::from(b.r()) - f32::from(a.r())) * t) as u8,
        (f32::from(a.g()) + (f32::from(b.g()) - f32::from(a.g())) * t) as u8,
        (f32::from(a.b()) + (f32::from(b.b()) - f32::from(a.b())) * t) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_document_round_trips_through_ron() {
        let graph = TerrainFieldGraph::default();
        let encoded = ron::ser::to_string(&graph).unwrap();
        let decoded: TerrainFieldGraph = ron::from_str(&encoded).unwrap();
        assert_eq!(graph, decoded);
        assert!(decoded.validate().is_empty());
    }

    #[test]
    fn spherical_coordinate_conversion_round_trips() {
        let longitude = 1.37;
        let latitude = -0.62;
        let direction = direction_from_angles(longitude, latitude);
        let (decoded_longitude, decoded_latitude) = angles_from_direction(direction);
        assert!((longitude - decoded_longitude).abs() < 1e-12);
        assert!((latitude - decoded_latitude).abs() < 1e-12);
    }
}
