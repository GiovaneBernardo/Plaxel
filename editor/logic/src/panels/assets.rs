//! Project browser and material editor. The directory listing and asset headers are
//! cached: reading them touches the disk, which must never happen per frame.

use crate::EditorContext;
use crate::panels::fields::{bool_row, field_label, float_array_row};
use crate::panels::fields::{inspector_grid, int_row, readonly_row, scalar_row, text_row, u32_row};
use crate::panels::icons::{self, Icon};
use crate::theme;
use egui::{FontId, Rect, RichText, Ui, Vec2};
use engine::assets::{
    loader,
    manager::{AssetCatalog, AssetHeader, AssetType, Assets, Uuid},
    material::{Material, MaterialResource, MaterialValue},
    serializer,
    server::AssetServer,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

const MIN_TILE: f32 = 56.0;
const MAX_TILE: f32 = 140.0;
const DEFAULT_TILE: f32 = 84.0;
/// Directory contents are re-read at most this often, plus on demand.
const LISTING_REFRESH_INTERVAL: f64 = 2.0;

pub struct AssetEditorState {
    current_dir: PathBuf,
    selected_path: Option<PathBuf>,
    material: Option<MaterialEditor>,
    status: Option<String>,
    search: String,
    tile_size: f32,
    listing: DirectoryListing,
}

#[derive(Default)]
struct DirectoryListing {
    dir: PathBuf,
    refreshed_at: f64,
    error: Option<String>,
    entries: Vec<AssetEntry>,
}

struct AssetEntry {
    path: PathBuf,
    name: String,
    is_dir: bool,
    icon: Icon,
    header: Option<AssetHeader>,
}

impl AssetEditorState {
    pub fn new() -> Self {
        Self {
            current_dir: project_root(),
            selected_path: None,
            material: None,
            status: None,
            search: String::new(),
            tile_size: DEFAULT_TILE,
            listing: DirectoryListing::default(),
        }
    }

    fn refresh_listing(&mut self, now: f64, force: bool) {
        let stale = now - self.listing.refreshed_at > LISTING_REFRESH_INTERVAL;
        if !force && !stale && self.listing.dir == self.current_dir {
            return;
        }

        self.listing.dir = self.current_dir.clone();
        self.listing.refreshed_at = now;
        self.listing.entries.clear();
        self.listing.error = None;

        let read = match fs::read_dir(&self.current_dir) {
            Ok(read) => read,
            Err(error) => {
                self.listing.error = Some(format!("Unable to read folder: {error}"));
                return;
            }
        };

        for entry in read.filter_map(Result::ok) {
            let path = entry.path();
            let is_dir = path.is_dir();
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            let header = (!is_dir && is_compiled_asset(&path))
                .then(|| loader::load_header(&path).ok())
                .flatten();
            let icon = entry_icon(is_dir, &path, header.as_ref());
            self.listing.entries.push(AssetEntry {
                path,
                name,
                is_dir,
                icon,
                header,
            });
        }

        self.listing.entries.sort_by(|a, b| {
            b.is_dir.cmp(&a.is_dir).then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
        });
    }

    fn select_path(&mut self, path: PathBuf, state: &EditorContext<'_>) {
        self.selected_path = Some(path.clone());
        self.status = None;
        self.material = None;

        if path.is_dir() || !is_asset_extension(&path, "plxmat") {
            return;
        }

        match MaterialEditor::load(path, state) {
            Ok(editor) => self.material = Some(editor),
            Err(error) => self.status = Some(format!("Unable to load material: {error}")),
        }
    }

    pub fn assign_texture_to_material(&mut self, texture_uuid: Uuid) {
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
            None => editor.status = Some("Material has no texture binding.".to_string()),
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
        let mut material = loader::load_material_payload(&path)?;

        if let Some(loaded) = state
            .world
            .get_resource::<Assets<Material>>()
            .and_then(|assets| assets.get_by_id(material.uuid).cloned())
        {
            material.material_index = loaded.material_index;
        }

        if let Some(server) = state.world.get_resource::<AssetServer>() {
            server.add(material.clone());
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
        self.header.version = 2;
        self.header.type_name = Some(std::any::type_name::<Material>().to_owned());

        match serializer::write_asset(
            self.header.clone(),
            std::any::type_name::<Material>(),
            "plxmat",
            &self.material,
            &self.path,
        ) {
            Ok(()) => {
                if let Some(server) = state.world.get_resource::<AssetServer>() {
                    server.register_cooked_path(&self.header, self.path.clone());
                    server.add(self.material.clone());
                }
                if let Some(mut catalog) = state.world.get_resource_mut::<AssetCatalog>() {
                    catalog.paths.insert(self.path.clone(), self.material.uuid);
                    catalog
                        .headers
                        .insert(self.material.uuid, self.header.clone());
                }
                self.status = Some("Saved material.".to_string());
            }
            Err(error) => self.status = Some(format!("Save failed: {error}")),
        }
    }
}

pub fn draw_asset_browser(
    ui: &mut Ui,
    state: &mut EditorContext<'_>,
    assets: &mut AssetEditorState,
) {
    let now = ui.input(|input| input.time);
    let mut force_refresh = false;

    theme::toolbar(ui, |ui| {
        if ui.button("Root").clicked() {
            assets.current_dir = project_root();
        }
        if ui
            .add_enabled(
                assets.current_dir.parent().is_some(),
                egui::Button::new("⬆ Up"),
            )
            .clicked()
            && let Some(parent) = assets.current_dir.parent()
        {
            assets.current_dir = parent.to_path_buf();
        }
        if ui.button("⟳").on_hover_text("Rescan folder").clicked() {
            force_refresh = true;
        }
        ui.separator();
        theme::truncated(ui, relative_display(&assets.current_dir), theme::TEXT_DIM);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(
                egui::Slider::new(&mut assets.tile_size, MIN_TILE..=MAX_TILE)
                    .show_value(false)
                    .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 0.5 }),
            )
            .on_hover_text("Tile size");
            theme::search_field(ui, "asset_search", "search", &mut assets.search);
        });
    });

    assets.refresh_listing(now, force_refresh);

    let available = ui.available_width();
    let inspector_width = (available * 0.26).clamp(190.0, 300.0);
    let tiles_width = (available - inspector_width - 10.0).max(120.0);
    ui.horizontal_top(|ui| {
        ui.allocate_ui(Vec2::new(tiles_width, ui.available_height()), |ui| {
            draw_asset_tiles(ui, state, assets)
        });
        ui.separator();
        ui.allocate_ui(
            Vec2::new(ui.available_width(), ui.available_height()),
            |ui| {
                draw_selected_asset_inspector(ui, state, assets);
            },
        );
    });
}

fn draw_asset_tiles(ui: &mut Ui, state: &mut EditorContext<'_>, assets: &mut AssetEditorState) {
    if let Some(error) = &assets.listing.error {
        ui.colored_label(theme::ERROR, error);
        return;
    }

    let query = assets.search.trim().to_ascii_lowercase();
    let tile_size = assets.tile_size;
    let mut activated: Option<PathBuf> = None;
    let mut selected: Option<PathBuf> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("asset_tiles")
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(4.0);
            ui.horizontal_wrapped(|ui| {
                for entry in &assets.listing.entries {
                    if !query.is_empty() && !entry.name.to_ascii_lowercase().contains(&query) {
                        continue;
                    }
                    let is_selected = assets.selected_path.as_ref() == Some(&entry.path);
                    let uploaded = entry
                        .header
                        .as_ref()
                        .filter(|header| header.asset_type == AssetType::Texture)
                        .is_some_and(|header| {
                            state
                                .global_resources
                                .renderer
                                .renderer_api
                                .is_texture_asset_uploaded(header.uuid)
                        });

                    let response = asset_tile(ui, entry, tile_size, is_selected, uploaded);
                    if response.double_clicked() && entry.is_dir {
                        activated = Some(entry.path.clone());
                    } else if response.clicked() {
                        selected = Some(entry.path.clone());
                    }
                }
            });
        });

    if let Some(path) = activated {
        assets.current_dir = path;
        assets.listing.refreshed_at = f64::NEG_INFINITY;
    } else if let Some(path) = selected {
        assets.select_path(path, state);
    }
}

/// Thumbnail first tile: the icon owns the tile and the name sits in a footer strip,
/// so a folder full of assets reads as a grid of icons rather than a wall of text.
fn asset_tile(
    ui: &mut Ui,
    entry: &AssetEntry,
    tile_size: f32,
    selected: bool,
    uploaded: bool,
) -> egui::Response {
    let footer = (tile_size * 0.26).clamp(20.0, 30.0);
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(tile_size, tile_size + footer),
        egui::Sense::click(),
    );
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let (fill, stroke_color) = if selected {
        (theme::ACCENT_DIM, theme::ACCENT)
    } else if response.hovered() {
        (theme::BG_HOVER, theme::BORDER_STRONG)
    } else {
        (theme::BG_SURFACE, theme::BORDER)
    };

    let painter = ui.painter();
    let radius = egui::CornerRadius::same(4);
    painter.rect(
        rect,
        radius,
        fill,
        egui::Stroke::new(1.0, stroke_color),
        egui::StrokeKind::Inside,
    );

    let thumbnail = Rect::from_min_size(rect.min, Vec2::new(rect.width(), tile_size));
    painter.rect_filled(thumbnail.shrink(1.0), radius, theme::BG_DEEP);
    icons::paint(
        painter,
        thumbnail.shrink(tile_size * 0.18),
        entry.icon,
        entry.icon.color(),
    );

    let mut job = egui::text::LayoutJob::simple(
        entry.name.clone(),
        FontId::proportional(10.5),
        if selected {
            theme::TEXT_STRONG
        } else {
            theme::TEXT
        },
        rect.width() - 8.0,
    );
    job.wrap.max_rows = 2;
    job.wrap.break_anywhere = true;
    job.halign = egui::Align::Center;
    let galley = painter.layout_job(job);
    painter.galley(
        egui::pos2(rect.center().x, thumbnail.bottom() + 3.0),
        galley,
        theme::TEXT,
    );

    if uploaded {
        painter.circle_filled(
            egui::pos2(rect.right() - 8.0, rect.top() + 8.0),
            3.0,
            theme::SUCCESS,
        );
    }

    response.on_hover_text(entry.path.to_string_lossy().to_string())
}

fn draw_selected_asset_inspector(
    ui: &mut Ui,
    state: &mut EditorContext<'_>,
    assets: &mut AssetEditorState,
) {
    theme::section(ui, "Asset inspector");

    let Some(path) = assets.selected_path.clone() else {
        theme::empty_state(ui, "◌", "Select an asset or folder.");
        return;
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("asset_inspector")
        .show(ui, |ui| {
            theme::truncated(ui, relative_display(&path), theme::TEXT_STRONG);
            if path.is_dir() {
                ui.label(RichText::new("Folder").color(theme::TEXT_DIM));
                return;
            }

            if let Some(message) = &assets.status {
                ui.colored_label(theme::WARN, message);
            }

            match assets.material.as_mut() {
                Some(editor) if editor.path == path => draw_material_editor(ui, state, editor),
                _ => draw_generic_asset_info(ui, state, &path),
            }
        });
}

fn draw_generic_asset_info(ui: &mut Ui, state: &EditorContext<'_>, path: &Path) {
    match loader::load_header(path) {
        Ok(header) => theme::card(ui, |ui| {
            inspector_grid(ui, "generic_asset_info", |ui| {
                readonly_row(ui, "Name", &header.name);
                readonly_row(ui, "Type", &format!("{:?}", header.asset_type));
                readonly_row(ui, "Uuid", &header.uuid.to_string());
                readonly_row(ui, "Loaded", &asset_loaded_text(state, &header));
            });
        }),
        Err(_) => {
            ui.label(RichText::new("Raw file").color(theme::TEXT_DIM));
        }
    }
}

fn draw_material_editor(ui: &mut Ui, state: &mut EditorContext<'_>, editor: &mut MaterialEditor) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Material").strong().color(theme::TEXT_STRONG));
        if ui.button("Save").clicked() {
            editor.save(state);
        }
    });

    if let Some(message) = &editor.status {
        ui.colored_label(theme::SUCCESS, message);
    }

    theme::card(ui, |ui| {
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
    });

    theme::section(ui, "Bindings");
    for (index, binding) in editor.material.bindings.iter_mut().enumerate() {
        theme::card(ui, |ui| {
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
                    && ui.button("Clear").clicked()
                {
                    binding.resource = MaterialResource::Texture(Uuid::nil());
                }
            });
        });
    }

    theme::section(ui, "Parameters");
    for (index, parameter) in editor.material.parameters.iter_mut().enumerate() {
        theme::card(ui, |ui| {
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
}

fn material_resource_row(ui: &mut Ui, label: &str, resource: &mut MaterialResource) {
    field_label(ui, label);
    match resource {
        MaterialResource::Texture(uuid) => {
            let mut text = uuid.to_string();
            if ui
                .add_sized([220.0, 18.0], egui::TextEdit::singleline(&mut text))
                .lost_focus()
                && let Ok(parsed) = Uuid::parse_str(text.trim())
            {
                *uuid = parsed;
            }
        }
        MaterialResource::TextureArray(uuids) => {
            ui.label(format!("TextureArray [{}]", uuids.len()));
        }
        MaterialResource::Sampler(_) => {
            ui.label("Sampler");
        }
        MaterialResource::Buffer(uuid) => {
            theme::truncated(ui, format!("Buffer {uuid}"), theme::TEXT);
        }
    }
    ui.end_row();
}

pub fn draw_texture_explorer(
    ui: &mut Ui,
    state: &mut EditorContext<'_>,
    assets: &mut AssetEditorState,
) {
    let textures = loaded_texture_headers(state);

    theme::toolbar(ui, |ui| {
        ui.label(
            RichText::new("Loaded textures")
                .strong()
                .color(theme::TEXT_STRONG),
        );
        theme::tag(ui, &textures.len().to_string(), theme::ACCENT);
        ui.separator();
        ui.label(
            RichText::new("Assign one to the selected material binding").color(theme::TEXT_DIM),
        );
    });

    if textures.is_empty() {
        theme::empty_state(ui, "◌", "No loaded texture assets found.");
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for header in textures {
                    theme::card(ui, |ui| {
                        ui.set_width(168.0);
                        theme::truncated(ui, &header.name, theme::TEXT_STRONG);
                        ui.label(
                            RichText::new(header.uuid.to_string())
                                .monospace()
                                .size(9.5)
                                .color(theme::TEXT_DIM),
                        );
                        if ui.button("Use").clicked() {
                            assets.assign_texture_to_material(header.uuid);
                        }
                    });
                }
            });
        });
}

fn entry_icon(is_dir: bool, path: &Path, header: Option<&AssetHeader>) -> Icon {
    if is_dir {
        return Icon::Folder;
    }
    if let Some(header) = header {
        return match header.asset_type {
            AssetType::Material => Icon::Material,
            AssetType::Texture => Icon::Image,
            AssetType::Mesh => Icon::Mesh,
            AssetType::Prefab => Icon::Prefab,
            AssetType::Audio => Icon::Audio,
            AssetType::Custom => Icon::File,
        };
    }
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" | "jpg" | "jpeg" | "tga" | "hdr" => Icon::Image,
        "wgsl" => Icon::Shader,
        "obj" | "gltf" | "glb" => Icon::Mesh,
        "wav" | "ogg" | "mp3" => Icon::Audio,
        "ron" | "toml" | "json" | "md" | "txt" | "log" => Icon::Text,
        _ => Icon::File,
    }
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
            .world
            .get_resource::<Assets<Material>>()
            .is_some_and(|assets| assets.get_by_id(header.uuid).is_some())
            .to_string(),
        _ => "unknown".to_string(),
    }
}

fn loaded_texture_headers(state: &EditorContext<'_>) -> Vec<AssetHeader> {
    let mut textures = state
        .world
        .get_resource::<AssetCatalog>()
        .map(|catalog| {
            catalog
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
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
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

pub fn project_root() -> PathBuf {
    static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .clone()
}

pub fn relative_display(path: &Path) -> String {
    path.strip_prefix(project_root())
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}
