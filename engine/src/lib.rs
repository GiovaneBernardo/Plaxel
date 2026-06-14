use std::{path::Path, sync::Arc};

pub mod assets;
pub mod core;
pub mod frame_capturer;
pub mod global_resources;
pub mod logging;
pub mod renderer;

use cgmath::Point3;
use cgmath::Quaternion;
use cgmath::Vector3;
use cgmath::vec3;
pub use core::camera;
pub use core::ecs;
pub use renderer::model;
pub use renderer::texture;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

pub use winit::{
    application::ApplicationHandler,
    event::*,
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::PhysicalKey,
    window::Window,
};

use crate::assets::importer::AssetImporter;
use crate::assets::importer::AssetPayload;
use crate::assets::importer::ImportSettings;
use crate::assets::importer::TargetPlatform;
use crate::assets::importers::obj_importer::ObjImporter;
use crate::assets::loader;
use crate::assets::material::Material;
use crate::assets::material::MaterialResource;
use crate::assets::serializer;
use crate::core::components::{
    core::TransformComponent,
    physics::{BodyKind, ColliderComponent, ColliderShape, RigidBodyComponent},
    renderer::MeshRendererComponent,
};
use crate::core::ecs::world::World;
use crate::core::input::InputState;
use crate::core::input::KeyCode;
use crate::core::physics::physics::Physics;
use crate::core::time::Time;
use crate::ecs::scene::Scene;
use crate::global_resources::GlobalResources;
use crate::model::TransformInstance;
use crate::model::Vertex;
use crate::model::{AttributeFormat, MeshAsset, StepMode, VertexAttribute, VertexLayout};
use crate::renderer::TextureDimension;
use crate::renderer::TextureFormat;
use crate::renderer::TextureSize;
use crate::renderer::TextureUsages;
use crate::renderer::{FrameBindings, GeometryPassNode, GeometryRenderQueue};
use cgmath::InnerSpace;

fn cook_trimesh_indices(vertices: &[cgmath::Point3<f32>], indices: &[u32]) -> Vec<[u32; 3]> {
    let vertex_count = vertices.len() as u32;
    let mut triangles = Vec::with_capacity(indices.len() / 3);

    for triangle in indices.chunks_exact(3) {
        let [a, b, c] = [triangle[0], triangle[1], triangle[2]];
        if a >= vertex_count || b >= vertex_count || c >= vertex_count {
            continue;
        }

        let pa = vertices[a as usize];
        let pb = vertices[b as usize];
        let pc = vertices[c as usize];
        if !pa.x.is_finite()
            || !pa.y.is_finite()
            || !pa.z.is_finite()
            || !pb.x.is_finite()
            || !pb.y.is_finite()
            || !pb.z.is_finite()
            || !pc.x.is_finite()
            || !pc.y.is_finite()
            || !pc.z.is_finite()
        {
            continue;
        }

        let ab = pb - pa;
        let ac = pc - pa;
        if ab.cross(ac).magnitude2() <= f32::EPSILON {
            continue;
        }

        triangles.push([a, b, c]);
    }

    triangles
}

fn find_sibling_asset_by_uuid(
    asset_path: &Path,
    uuid: assets::manager::Uuid,
    extension: &str,
) -> Option<std::path::PathBuf> {
    let dir = asset_path.parent()?;
    let entries = std::fs::read_dir(dir).ok()?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(extension))
        {
            continue;
        }

        let Ok(header) = loader::load_header(&path) else {
            continue;
        };
        if header.uuid == uuid {
            return Some(path);
        }
    }

    None
}

// This will store the state of our game
pub struct State {
    pub window: Arc<Window>,
    pub active_scene_index: Option<u32>,
    pub scenes: Vec<ecs::scene::Scene>,
    pub events: Vec<WindowEvent>,
    pub frame_index: u32,
    pub registered_systems: bool,
    pub global_resources: GlobalResources,
}

impl State {
    pub async fn new(window: Arc<Window>) -> anyhow::Result<Self> {
        let _size = window.inner_size();

        let mut scenes = Vec::new();
        scenes.insert(0, Self::create_main_game_scene());

        let mut global_resources = GlobalResources::new(window.clone()).await;
        global_resources.renderer.init();

        Ok(Self {
            window,
            events: Vec::new(),
            active_scene_index: Some(0),
            scenes,
            frame_index: 0,
            registered_systems: false,
            global_resources,
        })
    }

    pub fn active_scene(&self) -> Option<&ecs::scene::Scene> {
        self.active_scene_index
            .and_then(|i| self.scenes.get(i as usize))
    }

    pub fn active_scene_mut(&mut self) -> Option<&mut ecs::scene::Scene> {
        self.active_scene_index
            .and_then(|i| self.scenes.get_mut(i as usize))
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.global_resources.renderer.resize(width, height);

            // Resize render graph
            self.global_resources.renderer.render_graph.resize(
                self.global_resources.renderer.renderer_api.as_mut(),
                &mut self.global_resources.renderer.render_resources,
                width,
                height,
            );
        }
    }

    fn handle_key(&mut self, event_loop: &ActiveEventLoop, code: KeyCode, is_pressed: bool) {
        if code == KeyCode::Escape && is_pressed {
            event_loop.exit();
        } else {
            //self.camera_controller.handle_key(code, is_pressed);
        }

        if code == KeyCode::KeyH && is_pressed {
            self.global_resources.frame_capturer.request_capture();
        }

        if code == KeyCode::KeyR && is_pressed {
            self.global_resources.renderer.renderer_api.reload_shaders();
        }

        let world = self.active_scene_mut().unwrap().world_mut();
        let mut input = world.get_resource_mut::<InputState>().unwrap();
        if is_pressed {
            input.pressed.insert(code);
            input.just_pressed.insert(code);
        } else {
            input.pressed.remove(&code);
            input.just_released.insert(code);
        }
    }

    fn handle_mouse_click(&mut self, button: MouseButton, is_pressed: bool) {
        if button == MouseButton::Right {
            //self.camera_controller.handle_mouse_click(is_pressed);
        }

        let world = self.active_scene_mut().unwrap().world_mut();
        let mut input = world.get_resource_mut::<InputState>().unwrap();
        if is_pressed {
            input.mouse_pressed.insert(button);
            input.mouse_just_pressed.insert(button);
        } else {
            input.mouse_pressed.remove(&button);
            input.mouse_just_released.insert(button);
        }
    }

    fn handle_mouse_scroll(&mut self, delta: MouseScrollDelta) {
        let scroll = match delta {
            MouseScrollDelta::LineDelta(_, y) => y,
            MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / 100.0,
        };

        let world = self.active_scene_mut().unwrap().world_mut();
        let mut input = world.get_resource_mut::<InputState>().unwrap();
        input.scroll += scroll.clamp(-1.0, 1.0);
    }

    fn handle_mouse_motion(&mut self, dx: f64, dy: f64) {
        let world = self.active_scene_mut().unwrap().world_mut();
        let mut input = world.get_resource_mut::<InputState>().unwrap();
        input.mouse_delta.0 += dx as f32;
        input.mouse_delta.1 += dy as f32;
    }

    fn extract_positions(mesh: &MeshAsset) -> Result<Vec<Point3<f32>>, String> {
        let stride = mesh.vertex_layout.stride as usize;

        //if stride != std::mem::size_of::<[f32; 3]>() {
        //    return Err(format!("expected stride 12, got {}", stride));
        //}

        let attr = mesh
            .vertex_layout
            .attributes
            .iter()
            .find(|attr| attr.shader_location == 0)
            .ok_or("missing position attribute at shader_location 0")?;

        if attr.offset != 0 {
            return Err(format!("expected position offset 0, got {}", attr.offset));
        }

        if mesh.vertices.len() % stride != 0 {
            return Err(format!(
                "vertex byte buffer length {} is not divisible by stride {}",
                mesh.vertices.len(),
                stride
            ));
        }

        let mut positions = Vec::with_capacity(mesh.vertices.len() / stride);

        for vertex in mesh.vertices.chunks_exact(stride) {
            let x = f32::from_le_bytes(vertex[0..4].try_into().unwrap());
            let y = f32::from_le_bytes(vertex[4..8].try_into().unwrap());
            let z = f32::from_le_bytes(vertex[8..12].try_into().unwrap());

            positions.push(Point3::new(x, y, z));
        }

        Ok(positions)
    }

    fn handle_dropped_file(&mut self, path: &Path) {
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("jpg"))
        {
            let file_name = path.file_name().unwrap().to_string_lossy().to_string();
            self.global_resources.renderer.renderer_api.load_texture(
                &path.to_str().unwrap().to_string(),
                &crate::renderer::TextureDescriptor {
                    label: file_name,
                    format: TextureFormat::Rgba8Srgb,
                    size: TextureSize::Custom {
                        width: 256,
                        height: 256,
                    },
                    dimension: TextureDimension::D2,
                    usage: TextureUsages::COPY_SRC
                        | TextureUsages::COPY_DST
                        | TextureUsages::TEXTURE_BINDING,
                    mip_levels: 1,
                    sample_count: 1,
                },
            );
            return;
        }

        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("plxtex"))
        {
            let payload = match loader::load_payload(path) {
                Ok(payload) => payload,
                Err(error) => {
                    log::error!("Unable to load texture asset {:?}: {error}", path);
                    return;
                }
            };

            let AssetPayload::Texture(texture) = payload else {
                log::error!("Imported asset {:?} is not a texture", path);
                return;
            };

            let handle = self
                .global_resources
                .renderer
                .renderer_api
                .upload_texture_asset(&texture, None);
            log::info!("Uploaded texture asset {:?} as {:?}", path, handle);
            return;
        }

        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("plxmat"))
        {
            let payload = match loader::load_payload(path) {
                Ok(payload) => payload,
                Err(error) => {
                    log::error!("Unable to load material asset {:?}: {error}", path);
                    return;
                }
            };

            let AssetPayload::Material(mut material) = payload else {
                log::error!("Imported asset {:?} is not a material", path);
                return;
            };

            self.upload_material_textures(path, &mut material);
            let uuid = material.uuid;
            self.global_resources
                .asset_manager
                .paths
                .insert(path.to_path_buf(), uuid);
            self.global_resources
                .asset_manager
                .add_asset::<Material>(material);
            log::info!("Loaded material asset {:?}", path);
            return;
        }

        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("plxmesh"))
        {
            let header = loader::load_header(path).unwrap();
            println!("Header: {header:?}");

            let payload = loader::load_payload(path).unwrap();

            if let AssetPayload::Mesh(mesh) = payload {
                let handle = self
                    .global_resources
                    .renderer
                    .renderer_api
                    .upload_mesh(&mesh);
                println!("Uploaded mesh: {:?}", handle);

                let material_uuid = self
                    .load_sibling_material_uuid(path, &mesh)
                    .unwrap_or_else(|| self.add_fallback_material(mesh.vertex_layout.clone()));

                let Some(camera_layout) = self
                    .global_resources
                    .renderer
                    .render_graph
                    .get_node_mut::<GeometryPassNode>(0)
                    .and_then(|node| node.camera_bind_group_layout)
                else {
                    return;
                };
                let Some(textures_layout) = self
                    .global_resources
                    .renderer
                    .render_resources
                    .get_labeled::<FrameBindings>("frame_bindings")
                    .map(|bindings| bindings.textures_layout)
                else {
                    return;
                    //anyhow::bail!("Frame material bind group layout is not available");
                };

                if let Some(material) = self
                    .global_resources
                    .asset_manager
                    .get_by_uuid::<Material>(material_uuid)
                {
                    self.global_resources
                        .renderer
                        .renderer_api
                        .create_pipeline(material, &[camera_layout, textures_layout]);
                }

                let world = self.active_scene_mut().unwrap().world_mut();
                let entity = world.spawn();
                world.insert(
                    entity,
                    TransformComponent {
                        position: Vector3::new(0.0, 0.0, 0.0),
                        rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                        scale: Vector3::new(0.01, 0.01, 0.01),
                        velocity: Vector3 {
                            x: 0.0,
                            y: 0.0,
                            z: 0.0,
                        },
                    },
                );

                world.insert(
                    entity,
                    MeshRendererComponent {
                        mesh: handle,
                        material: material_uuid,
                    },
                );

                let vertices = State::extract_positions(&mesh).unwrap();

                let indices: Vec<[u32; 3]> = mesh
                    .indices
                    .chunks_exact(3)
                    .map(|tri| [tri[0], tri[1], tri[2]])
                    .collect();

                world.insert(
                    entity,
                    ColliderComponent {
                        shape: ColliderShape::Trimesh { vertices, indices },
                        friction: 0.5,
                        restitution: 0.5,
                    },
                );
            }
        }

        if !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("obj"))
        {
            return;
        }

        let project_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let asset_root = project_root.join("res/imported");
        let import_settings = ImportSettings {
            force_reimport: false,
            generate_mipmaps: false,
            ignored_platform: TargetPlatform::None,
        };
        let import_context = assets::importer::ImportContext {
            project_root: &project_root,
            source_root: path.parent().unwrap_or_else(|| std::path::Path::new("")),
            asset_root: &asset_root,
            source_path: path,
            manager: &self.global_resources.asset_manager,
            settings: &import_settings,
        };

        let obj_importer = ObjImporter;
        let imported_assets = match obj_importer.import(path, &import_context) {
            Ok(imported_assets) => imported_assets,
            Err(error) => {
                log::error!("Unable to import dropped OBJ {:?}: {error}", path);
                return;
            }
        };

        for imported_asset in &imported_assets {
            let output_path = serializer::output_path_for(imported_asset, &asset_root);
            if let Err(error) = serializer::write_imported_asset(imported_asset, &output_path) {
                log::error!(
                    "Unable to serialize imported asset {:?}: {error}",
                    output_path
                );
                continue;
            }

            log::info!("Serialized imported asset to {:?}", output_path);
        }

        //if let Err(error) = self.spawn_dropped_obj(path, &point3(0.0, 0.0, 0.0)) {
        //    log::error!("Unable to load dropped OBJ {:?}: {error}", path);
        //}
    }

    fn load_sibling_material_uuid(
        &mut self,
        mesh_path: &Path,
        mesh: &MeshAsset,
    ) -> Option<assets::manager::Uuid> {
        if let Some(material_uuid) = mesh.material_uuid {
            if self
                .prepare_loaded_material(material_uuid, mesh.vertex_layout.clone())
                .is_some()
            {
                return Some(material_uuid);
            }

            let Some(material_path) =
                find_sibling_asset_by_uuid(mesh_path, material_uuid, "plxmat")
            else {
                log::warn!(
                    "Unable to find material {:?} for mesh {:?}",
                    material_uuid,
                    mesh_path
                );
                return None;
            };

            return self.load_material_from_path(&material_path, mesh.vertex_layout.clone());
        }

        let dir = mesh_path.parent()?;
        let mut material_paths = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("plxmat"))
            })
            .collect::<Vec<_>>();
        material_paths.sort();

        for material_path in material_paths {
            if let Some(material_uuid) =
                self.load_material_from_path(&material_path, mesh.vertex_layout.clone())
            {
                return Some(material_uuid);
            }
        }

        None
    }

    fn prepare_loaded_material(
        &mut self,
        material_uuid: assets::manager::Uuid,
        vertex_layout: VertexLayout,
    ) -> Option<assets::manager::Uuid> {
        let material =
            self.global_resources
                .asset_manager
                .get_mut::<Material>(assets::manager::Handle {
                    uuid: material_uuid,
                    asset_type: assets::manager::AssetType::Material,
                    _marker: std::marker::PhantomData,
                })?;
        material.pipeline_descriptor.vertex_layouts =
            vec![vertex_layout, TransformInstance::layout()];
        Some(material_uuid)
    }

    fn load_material_from_path(
        &mut self,
        material_path: &Path,
        vertex_layout: VertexLayout,
    ) -> Option<assets::manager::Uuid> {
        let found_uuid = self
            .global_resources
            .asset_manager
            .uuid_for_path(&material_path.to_path_buf())
            .copied();
        if let Some(found_uuid) = found_uuid {
            if self
                .prepare_loaded_material(found_uuid, vertex_layout.clone())
                .is_some()
            {
                return Some(found_uuid);
            }
        }

        let Ok(payload) = loader::load_payload(material_path) else {
            return None;
        };
        let AssetPayload::Material(mut material) = payload else {
            return None;
        };

        material = material.with_vertex_layouts(vec![vertex_layout, TransformInstance::layout()]);
        let uuid = material.uuid;
        self.upload_material_textures(material_path, &mut material);
        self.global_resources
            .asset_manager
            .add_asset::<Material>(material);

        Some(uuid)
    }

    fn add_fallback_material(&mut self, vertex_layout: VertexLayout) -> assets::manager::Uuid {
        let material = Material::new("shaders/cube.wgsl".to_string())
            .with_vertex_layouts(vec![vertex_layout, TransformInstance::layout()]);
        let uuid = material.uuid;
        self.global_resources
            .asset_manager
            .add_asset::<Material>(material);
        uuid
    }

    fn upload_material_textures(&mut self, material_path: &Path, material: &mut Material) {
        for binding in &material.bindings {
            let MaterialResource::Texture(texture_uuid) = &binding.resource else {
                continue;
            };
            if self
                .global_resources
                .renderer
                .renderer_api
                .is_texture_asset_uploaded(*texture_uuid)
            {
                continue;
            }

            let Some(texture_path) =
                find_sibling_asset_by_uuid(material_path, *texture_uuid, "plxtex")
            else {
                log::warn!(
                    "Unable to find texture {:?} for material {:?}",
                    texture_uuid,
                    material_path
                );
                continue;
            };
            let Ok(payload) = loader::load_payload(&texture_path) else {
                log::warn!("Unable to load texture asset {:?}", texture_path);
                continue;
            };
            let AssetPayload::Texture(texture) = payload else {
                log::warn!("Asset {:?} is not a texture", texture_path);
                continue;
            };

            self.global_resources
                .renderer
                .renderer_api
                .upload_texture_asset(&texture, None);
        }

        material.material_index = self
            .global_resources
            .renderer
            .renderer_api
            .upload_material_asset(material, None);
    }

    pub fn spawn_dropped_obj(
        &mut self,
        path: &Path,
        spawn_position: &Point3<f32>,
    ) -> anyhow::Result<()> {
        let (models, _) = tobj::load_obj(
            path,
            &tobj::LoadOptions {
                triangulate: true,
                single_index: true,
                ..Default::default()
            },
        )?;

        let y_offset = -10.0;
        let mut positions = Vec::<[f32; 3]>::new();
        let mut indices = Vec::<u32>::new();
        let mut min = cgmath::vec3(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let mut max = cgmath::vec3(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);

        for model in &models {
            let base_vertex = positions.len() as u32;
            for position in model.mesh.positions.chunks_exact(3) {
                let baked = [position[0], position[1] + y_offset, position[2]];
                min.x = min.x.min(baked[0]);
                min.y = min.y.min(baked[1]);
                min.z = min.z.min(baked[2]);
                max.x = max.x.max(baked[0]);
                max.y = max.y.max(baked[1]);
                max.z = max.z.max(baked[2]);
                positions.push(baked);
            }

            indices.extend(model.mesh.indices.iter().map(|index| base_vertex + *index));
        }

        if positions.is_empty() || indices.is_empty() {
            anyhow::bail!("OBJ has no triangle mesh data");
        }

        let mut center = (min + max) * 0.5;
        center.y -= 10000.0;

        let collider_vertices = positions
            .iter()
            .map(|position| {
                cgmath::point3(
                    position[0] - center.x + spawn_position.x,
                    position[1] - center.y + spawn_position.y,
                    position[2] - center.z + spawn_position.z,
                )
            })
            .collect::<Vec<_>>();
        let collider_indices = cook_trimesh_indices(&collider_vertices, &indices);
        if collider_indices.is_empty() {
            anyhow::bail!("OBJ has no valid collision triangles");
        }

        let vertex_layout = VertexLayout {
            stride: std::mem::size_of::<[f32; 3]>() as u64,
            step_mode: StepMode::Vertex,
            attributes: vec![VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: AttributeFormat::Float32x3,
            }],
        };

        let mesh = MeshAsset {
            name: path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("DroppedObj")
                .to_string(),
            uuid: assets::manager::Uuid::new_v4(),
            vertices: bytemuck::cast_slice(&positions).to_vec(),
            indices,
            material_uuid: None,
            vertex_layout: vertex_layout.clone(),
        };

        let material = Material::new("shaders/cube.wgsl".to_string())
            .with_vertex_layouts(vec![vertex_layout, TransformInstance::layout()]);
        let material_uuid = material.uuid;

        let Some(camera_layout) = self
            .global_resources
            .renderer
            .render_graph
            .get_node_mut::<GeometryPassNode>(0)
            .and_then(|node| node.camera_bind_group_layout)
        else {
            anyhow::bail!("GeometryPassNode camera bind group layout is not available");
        };
        let Some(textures_layout) = self
            .global_resources
            .renderer
            .render_resources
            .get_labeled::<FrameBindings>("frame_bindings")
            .map(|bindings| bindings.textures_layout)
        else {
            anyhow::bail!("Frame material bind group layout is not available");
        };

        self.global_resources
            .renderer
            .renderer_api
            .create_pipeline(&material, &[camera_layout, textures_layout]);
        self.global_resources
            .asset_manager
            .add_asset::<Material>(material);
        let mesh_handle = self
            .global_resources
            .renderer
            .renderer_api
            .upload_mesh(&mesh);

        let Some(scene_index) = self.active_scene_index.map(|i| i as usize) else {
            return Ok(());
        };
        let Some(scene) = self.scenes.get_mut(scene_index) else {
            return Ok(());
        };

        let world = scene.world_mut();
        let entity = world.spawn();
        world.insert(
            entity,
            TransformComponent {
                position: vec3(spawn_position.x, spawn_position.y, spawn_position.z),
                rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
                scale: cgmath::vec3(1.0, 1.0, 1.0),
                velocity: cgmath::vec3(0.0, 0.0, 0.0),
            },
        );
        world.insert(
            entity,
            MeshRendererComponent {
                mesh: mesh_handle,
                material: material_uuid,
            },
        );
        world.insert(
            entity,
            ColliderComponent {
                shape: ColliderShape::Trimesh {
                    vertices: collider_vertices,
                    indices: collider_indices,
                },
                restitution: 0.2,
                friction: 0.9,
            },
        );
        world.insert(
            entity,
            RigidBodyComponent {
                kind: BodyKind::Fixed,
                mass: 0.0,
                velocity: cgmath::vec3(0.0, 0.0, 0.0),
            },
        );

        log::info!("Spawned dropped OBJ {:?} as entity {:?}", path, entity);
        Ok(())
    }

    fn update(&mut self) {
        if let Some(scene_index) = self.active_scene_index.map(|i| i as usize) {
            if let Some(scene) = self.scenes.get_mut(scene_index) {
                scene.update(&mut self.global_resources);
            }
        }

        let world = self.active_scene_mut().unwrap().world_mut();
        let Some(mut physics) = world.get_resource_mut::<Physics>() else {
            return;
        };
        physics.step();
        //self.camera_controller.update_camera(&mut self.camera);
        //self.camera_uniform.update_view_proj(&self.camera);
        //self.renderer.render_resources.insert(renderer::CameraData {
        //    uniform: self.camera_uniform,
        //});
    }

    fn update_after_render(&mut self) {
        let world = self.active_scene_mut().unwrap().world_mut();
        Self::clear_input_system(world);
    }

    fn sync_render_queues(&mut self) {
        let Some(scene_index) = self.active_scene_index.map(|i| i as usize) else {
            return;
        };
        let Some(scene) = self.scenes.get(scene_index) else {
            return;
        };

        self.global_resources
            .renderer
            .sync_geometry_render_queue(scene.world());
    }

    fn clear_input_system(world: &mut World) {
        let Some(mut input) = world.get_resource_mut::<InputState>() else {
            return;
        };

        input.just_pressed.clear();
        input.just_released.clear();
        input.mouse_just_pressed.clear();
        input.mouse_just_released.clear();
        input.mouse_delta = (0.0, 0.0);
        input.scroll = 0.0;
    }

    fn insert_engine_resources(world: &mut World) {
        world.insert_resource(Time::new());
        world.insert_resource(InputState::new());
        world.insert_resource(Physics::new());
        world.insert_resource(GeometryRenderQueue::new());
    }

    pub fn create_main_game_scene() -> Scene {
        let mut scene = Scene::new();

        Self::insert_engine_resources(scene.world_mut());

        scene
    }
}

pub struct App {
    #[cfg(target_arch = "wasm32")]
    proxy: Option<winit::event_loop::EventLoopProxy<State>>,
    state: Option<State>,
    on_register_system: Option<Box<dyn FnMut(&mut State)>>,
    on_update: Option<Box<dyn FnMut(&mut State)>>,
    on_key: Option<Box<dyn FnMut(&mut State, KeyCode, bool)>>,
    on_resize: Option<Box<dyn FnMut(&mut State, u32, u32)>>,
    on_mouse_button: Option<Box<dyn FnMut(&mut State, MouseButton, bool)>>,
    on_mouse_motion: Option<Box<dyn FnMut(&mut State, f64, f64)>>,
    on_mouse_scroll: Option<Box<dyn FnMut(&mut State, MouseScrollDelta)>>,
    on_render: Option<
        Box<dyn FnMut(&wgpu::Device, &wgpu::Queue, &wgpu::TextureView, &mut wgpu::CommandEncoder)>,
    >,
}

impl App {
    pub fn new(#[cfg(target_arch = "wasm32")] event_loop: &EventLoop<State>) -> Self {
        #[cfg(target_arch = "wasm32")]
        let proxy = Some(event_loop.create_proxy());
        Self {
            state: None,
            on_register_system: None,
            on_update: None,
            on_key: None,
            on_resize: None,
            on_mouse_button: None,
            on_mouse_motion: None,
            on_mouse_scroll: None,
            on_render: None,
            #[cfg(target_arch = "wasm32")]
            proxy,
        }
    }

    pub fn with_register_system(mut self, f: impl FnMut(&mut State) + 'static) -> Self {
        self.on_register_system = Some(Box::new(f));
        self
    }

    pub fn with_update(mut self, f: impl FnMut(&mut State) + 'static) -> Self {
        self.on_update = Some(Box::new(f));
        self
    }

    pub fn with_on_key(mut self, f: impl FnMut(&mut State, KeyCode, bool) + 'static) -> Self {
        self.on_key = Some(Box::new(f));
        self
    }

    pub fn with_on_resize(mut self, f: impl FnMut(&mut State, u32, u32) + 'static) -> Self {
        self.on_resize = Some(Box::new(f));
        self
    }

    pub fn with_on_mouse_button(
        mut self,
        f: impl FnMut(&mut State, MouseButton, bool) + 'static,
    ) -> Self {
        self.on_mouse_button = Some(Box::new(f));
        self
    }

    pub fn with_on_mouse_motion(mut self, f: impl FnMut(&mut State, f64, f64) + 'static) -> Self {
        self.on_mouse_motion = Some(Box::new(f));
        self
    }

    pub fn with_on_mouse_scroll(
        mut self,
        f: impl FnMut(&mut State, MouseScrollDelta) + 'static,
    ) -> Self {
        self.on_mouse_scroll = Some(Box::new(f));
        self
    }

    pub fn with_render(
        mut self,
        f: impl FnMut(&wgpu::Device, &wgpu::Queue, &wgpu::TextureView, &mut wgpu::CommandEncoder)
        + 'static,
    ) -> Self {
        self.on_render = Some(Box::new(f));
        self
    }
}

impl ApplicationHandler<State> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Run the loop continuously instead of sleeping between OS events —
        // we want every frame to tick update() so background workers can
        // make progress even when the player isn't moving.
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

        #[allow(unused_mut)]
        let mut window_attributes = Window::default_attributes();

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            use winit::platform::web::WindowAttributesExtWebSys;

            const CANVAS_ID: &str = "canvas";

            let window = wgpu::web_sys::window().unwrap_throw();
            let document = window.document().unwrap_throw();
            let canvas = document.get_element_by_id(CANVAS_ID).unwrap_throw();
            let html_canvas_element = canvas.unchecked_into();
            window_attributes = window_attributes.with_canvas(Some(html_canvas_element));
        }

        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());

        #[cfg(not(target_arch = "wasm32"))]
        {
            // If we are not on web we can use pollster to
            // await the
            self.state = Some(pollster::block_on(State::new(window)).unwrap());
        }

        #[cfg(target_arch = "wasm32")]
        {
            // Run the future asynchronously and use the
            // proxy to send the results to the event loop
            if let Some(proxy) = self.proxy.take() {
                wasm_bindgen_futures::spawn_local(async move {
                    assert!(
                        proxy
                            .send_event(
                                State::new(window)
                                    .await
                                    .expect("Unable to create canvas!!!")
                            )
                            .is_ok()
                    )
                });
            }
        }
    }

    #[allow(unused_mut)]
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, mut event: State) {
        #[cfg(target_arch = "wasm32")]
        {
            event.window.request_redraw();
            event.resize(
                event.window.inner_size().width,
                event.window.inner_size().height,
            );
        }
        self.state = Some(event);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let state = match &mut self.state {
            Some(canvas) => canvas,
            None => return,
        };

        if !matches!(event, WindowEvent::RedrawRequested) {
            state.events.push(event.clone());
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                state.resize(size.width, size.height);
                if let Some(f) = &mut self.on_resize {
                    f(state, size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                if !state.registered_systems {
                    if let Some(f) = &mut self.on_register_system {
                        f(state);
                    }
                    state.registered_systems = true;
                }
                state.window.request_redraw();
                state.update();
                state.global_resources.renderer.clear_geometry_render_data();
                if let Some(f) = &mut self.on_update {
                    f(state);
                }
                state.sync_render_queues();
                state.events.clear();
                let state = self.state.as_mut().unwrap();
                match state.global_resources.renderer.render() {
                    Ok(_) => {
                        state
                            .global_resources
                            .frame_capturer
                            .finish_capture_after_frame();
                    }
                    Err(e) => {
                        log::error!("Unable to render {}", e);
                    }
                }

                state.update_after_render();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state: key_state,
                        ..
                    },
                ..
            } => {
                state.handle_key(event_loop, code, key_state.is_pressed());
                if let Some(f) = &mut self.on_key {
                    f(state, code, key_state.is_pressed());
                }
            }
            WindowEvent::CursorMoved {
                position: winit::dpi::PhysicalPosition { x: _, y: _ },
                ..
            } => {
                // state.camera_controller.handle_mouse(x, y);
            }
            WindowEvent::MouseInput {
                device_id: _,
                state: key_state,
                button,
            } => {
                state.handle_mouse_click(button, key_state.is_pressed());
                if let Some(f) = &mut self.on_mouse_button {
                    f(state, button, key_state.is_pressed());
                }
            }

            WindowEvent::MouseWheel {
                device_id: _,
                delta,
                phase: _,
            } => {
                state.handle_mouse_scroll(delta);
                if let Some(f) = &mut self.on_mouse_scroll {
                    f(state, delta);
                }
            }
            WindowEvent::DroppedFile(path) => {
                state.handle_dropped_file(&path);
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        if let Some(state) = &mut self.state {
            if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
                state.handle_mouse_motion(dx, dy);
                if let Some(f) = &mut self.on_mouse_motion {
                    f(state, dx, dy);
                }
                //if state.camera_controller.is_right_click_pressed {
                //    state.camera_controller.handle_mouse(dx as f32, dy as f32);
                //    state
                //        .window
                //        .set_cursor_grab(winit::window::CursorGrabMode::Locked)
                //        .ok();
                //    state.window.set_cursor_visible(false);
                //} else {
                //    state.window.set_cursor_visible(true);
                //    state
                //        .window
                //        .set_cursor_grab(winit::window::CursorGrabMode::None)
                //        .ok();
                //}
            }
        }
    }
}

pub fn run() -> anyhow::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        logging::init();
    }
    #[cfg(target_arch = "wasm32")]
    {
        console_log::init_with_level(log::Level::Info).unwrap_throw();
    }

    let event_loop = EventLoop::with_user_event().build()?;
    let mut app = App::new(
        #[cfg(target_arch = "wasm32")]
        &event_loop,
    );
    event_loop.run_app(&mut app)?;

    Ok(())
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn run_web() -> Result<(), wasm_bindgen::JsValue> {
    console_error_panic_hook::set_once();
    run().unwrap_throw();

    Ok(())
}
