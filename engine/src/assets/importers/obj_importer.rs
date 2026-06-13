use std::path::Path;

use anyhow::{Ok, Result};
use uuid::Uuid;

use crate::{
    assets::{
        importer::{AssetImporter, AssetPayload, ImportContext, ImportedAsset},
        manager::{AssetHeader, AssetType},
        material::{Material, MaterialBinding, MaterialResource, TextureAsset, TextureMip},
    },
    engine_info,
    model::{MeshAsset, ModelVertex, Vertex},
    renderer::{TextureDescriptor, TextureDimension, TextureFormat, TextureSize, TextureUsages},
};

pub struct ObjImporter;

impl AssetImporter for ObjImporter {
    fn extensions(&self) -> &[&'static str] {
        &["obj"]
    }

    fn id(&self) -> &'static str {
        "obj"
    }

    fn import(&self, source: &Path, _ctx: &ImportContext) -> Result<Vec<ImportedAsset>> {
        let (models, materials) = tobj::load_obj(
            source,
            &tobj::LoadOptions {
                triangulate: true,
                single_index: true,
                ..Default::default()
            },
        )?;

        let materials = materials.unwrap_or_default();

        println!("Number of models          = {}", models.len());
        println!("Number of materials       = {}", materials.len());

        let mut imported_assets = Vec::new();
        let material_uuids = materials.iter().map(|_| Uuid::new_v4()).collect::<Vec<_>>();
        for model in models {
            let uuid = Uuid::new_v4();
            let material_uuid = model
                .mesh
                .material_id
                .and_then(|material_id| material_uuids.get(material_id).copied());
            let vertex_count = model.mesh.positions.len() / 3;
            let mut vertices = Vec::with_capacity(vertex_count);

            for i in 0..vertex_count {
                let position = [
                    model.mesh.positions[i * 3],
                    model.mesh.positions[i * 3 + 1],
                    model.mesh.positions[i * 3 + 2],
                ];
                let tex_coords = if model.mesh.texcoords.len() >= (i + 1) * 2 {
                    [
                        model.mesh.texcoords[i * 2],
                        1.0 - model.mesh.texcoords[i * 2 + 1],
                    ]
                } else {
                    [0.0, 0.0]
                };
                let normal = if model.mesh.normals.len() >= (i + 1) * 3 {
                    [
                        model.mesh.normals[i * 3],
                        model.mesh.normals[i * 3 + 1],
                        model.mesh.normals[i * 3 + 2],
                    ]
                } else {
                    [0.0, 1.0, 0.0]
                };

                vertices.push(ModelVertex {
                    position,
                    tex_coords,
                    normal,
                });
            }

            let mesh = MeshAsset {
                name: model.name.clone(),
                uuid,
                vertices: bytemuck::cast_slice(&vertices).to_vec(),
                indices: model.mesh.indices,
                material_uuid,
                vertex_layout: ModelVertex::layout(),
            };

            imported_assets.push(ImportedAsset {
                header: AssetHeader {
                    version: 0,
                    uuid,
                    name: model.name.clone(),
                    asset_type: AssetType::Mesh,
                    file_path: source.to_path_buf(),
                    content_offset: 0,
                    content_size: 0,
                },
                payload: AssetPayload::Mesh(mesh),
            });
        }

        // Import materials
        for (material_index, material) in materials.into_iter().enumerate() {
            let uuid = material_uuids[material_index];

            let mut my_material = Material::new("shaders/opaque.wgsl".into());
            my_material.uuid = uuid;
            //my_material.with_vertex_layouts(layouts);

            if let Some(diffuse_texture) = material.diffuse_texture.as_ref() {
                let texture_path = source.parent().unwrap().join(diffuse_texture);
                let imported_texture = self.import_texture(&texture_path);

                my_material.bindings.push(MaterialBinding {
                    binding: 0,
                    name: "diffuse".into(),
                    group: 0,
                    resource: MaterialResource::Texture(imported_texture.header.uuid),
                });

                imported_assets.push(imported_texture);
            }

            imported_assets.push(ImportedAsset {
                header: AssetHeader {
                    version: 0,
                    uuid,
                    name: material.name.clone(),
                    asset_type: AssetType::Material,
                    file_path: source.to_path_buf(),
                    content_offset: 0,
                    content_size: 0,
                },
                payload: AssetPayload::Material(my_material),
            });
        }

        Ok(imported_assets)
    }

    fn version(&self) -> u32 {
        0
    }
}

impl ObjImporter {
    pub fn import_texture(&self, path: &Path) -> ImportedAsset {
        engine_info!("{:}", path.to_string_lossy().to_string());
        let img = image::open(path)
            .expect("Failed to load texture")
            .to_rgba8();

        let (width, height) = img.dimensions();

        let descriptor = TextureDescriptor {
            label: path.file_name().unwrap().to_string_lossy().into(),
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Srgb,
            mip_levels: 0,
            sample_count: 1,
            size: TextureSize::Custom { width, height },
            usage: TextureUsages::COPY_SRC
                | TextureUsages::COPY_DST
                | TextureUsages::TEXTURE_BINDING,
        };

        let data = img.as_raw();

        let uuid = Uuid::new_v4();
        ImportedAsset {
            header: AssetHeader {
                version: 0,
                uuid: uuid,
                name: path.file_name().unwrap().to_string_lossy().into(),
                asset_type: AssetType::Texture,
                file_path: path.to_path_buf(),
                content_offset: 0,
                content_size: 0,
            },
            payload: AssetPayload::Texture(TextureAsset {
                format: descriptor.format,
                width: width,
                height: height,
                mip_levels: [TextureMip {
                    width,
                    height,
                    bytes: data.to_vec(),
                }]
                .to_vec(),
                name: descriptor.label,
                uuid,
            }),
        }
    }
}
