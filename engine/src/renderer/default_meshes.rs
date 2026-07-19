use uuid::Uuid;

use crate::{
    assets::manager::Handle,
    model::{MeshAsset, ModelVertex, Vertex},
    renderer::RendererAPI,
};

/// GPU handles for the engine's reusable unit primitives.
///
/// The cube occupies `[-0.5, 0.5]` on each axis and the sphere has radius `0.5`,
/// so a scale of one gives both primitives a diameter of one world unit.
#[derive(Debug, Clone, Copy)]
pub struct DefaultMeshes {
    pub cube: Handle<MeshAsset>,
    pub sphere: Handle<MeshAsset>,
    pub wire_cube: Handle<MeshAsset>,
}

impl DefaultMeshes {
    pub(crate) fn upload(api: &mut dyn RendererAPI) -> Self {
        let cube = api.upload_mesh(&cube_mesh());
        let sphere = api.upload_mesh(&sphere_mesh(12, 24));
        let wire_cube = api.upload_mesh(&wire_cube_mesh());

        Self {
            cube,
            sphere,
            wire_cube,
        }
    }
}

fn mesh(name: &str, vertices: Vec<ModelVertex>, indices: Vec<u32>) -> MeshAsset {
    MeshAsset {
        name: name.to_string(),
        uuid: Uuid::new_v4(),
        vertices: bytemuck::cast_slice(&vertices).to_vec(),
        indices,
        material_uuid: None,
        vertex_layout: ModelVertex::layout(),
    }
}

fn cube_mesh() -> MeshAsset {
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    let faces = [
        (
            [0.0, 0.0, 1.0],
            [
                [-0.5, -0.5, 0.5],
                [0.5, -0.5, 0.5],
                [0.5, 0.5, 0.5],
                [-0.5, 0.5, 0.5],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [0.5, -0.5, -0.5],
                [-0.5, -0.5, -0.5],
                [-0.5, 0.5, -0.5],
                [0.5, 0.5, -0.5],
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            [
                [0.5, -0.5, 0.5],
                [0.5, -0.5, -0.5],
                [0.5, 0.5, -0.5],
                [0.5, 0.5, 0.5],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [-0.5, -0.5, -0.5],
                [-0.5, -0.5, 0.5],
                [-0.5, 0.5, 0.5],
                [-0.5, 0.5, -0.5],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [-0.5, 0.5, 0.5],
                [0.5, 0.5, 0.5],
                [0.5, 0.5, -0.5],
                [-0.5, 0.5, -0.5],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [-0.5, -0.5, -0.5],
                [0.5, -0.5, -0.5],
                [0.5, -0.5, 0.5],
                [-0.5, -0.5, 0.5],
            ],
        ),
    ];
    let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];

    for (normal, positions) in faces {
        let base = vertices.len() as u32;
        for (position, tex_coords) in positions.into_iter().zip(uvs) {
            vertices.push(ModelVertex {
                position,
                tex_coords,
                normal,
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    mesh("DefaultCube", vertices, indices)
}

fn sphere_mesh(latitudes: u32, longitudes: u32) -> MeshAsset {
    assert!(latitudes >= 2);
    assert!(longitudes >= 3);

    let mut vertices = Vec::with_capacity(((latitudes + 1) * (longitudes + 1)) as usize);
    let mut indices = Vec::with_capacity((latitudes * longitudes * 6) as usize);

    for latitude in 0..=latitudes {
        let v = latitude as f32 / latitudes as f32;
        let theta = std::f32::consts::PI * v;
        let y = theta.cos();
        let ring_radius = theta.sin();

        for longitude in 0..=longitudes {
            let u = longitude as f32 / longitudes as f32;
            let phi = std::f32::consts::TAU * u;
            let normal = [ring_radius * phi.cos(), y, ring_radius * phi.sin()];
            vertices.push(ModelVertex {
                position: [normal[0] * 0.5, normal[1] * 0.5, normal[2] * 0.5],
                tex_coords: [u, v],
                normal,
            });
        }
    }

    let row = longitudes + 1;
    for latitude in 0..latitudes {
        for longitude in 0..longitudes {
            let i0 = latitude * row + longitude;
            let i1 = i0 + 1;
            let i2 = i0 + row;
            let i3 = i2 + 1;
            indices.extend_from_slice(&[i0, i2, i1, i1, i2, i3]);
        }
    }

    mesh("DefaultSphere", vertices, indices)
}

fn wire_cube_mesh() -> MeshAsset {
    let positions = [
        [-0.5, -0.5, -0.5],
        [0.5, -0.5, -0.5],
        [0.5, 0.5, -0.5],
        [-0.5, 0.5, -0.5],
        [-0.5, -0.5, 0.5],
        [0.5, -0.5, 0.5],
        [0.5, 0.5, 0.5],
        [-0.5, 0.5, 0.5],
    ];
    let vertices = positions
        .into_iter()
        .map(|position| ModelVertex {
            position,
            tex_coords: [0.0; 2],
            normal: [0.0; 3],
        })
        .collect();
    let indices = vec![
        0, 1, 1, 2, 2, 3, 3, 0, 4, 5, 5, 6, 6, 7, 7, 4, 0, 4, 1, 5, 2, 6, 3, 7,
    ];

    mesh("DefaultWireCube", vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_has_per_face_vertices() {
        let cube = cube_mesh();
        assert_eq!(cube.vertices.len(), 24 * size_of::<ModelVertex>());
        assert_eq!(cube.indices.len(), 36);
        assert_eq!(cube.vertex_layout, ModelVertex::layout());
    }

    #[test]
    fn sphere_uses_requested_tessellation() {
        let sphere = sphere_mesh(12, 24);
        assert_eq!(sphere.vertices.len(), 13 * 25 * size_of::<ModelVertex>());
        assert_eq!(sphere.indices.len(), 12 * 24 * 6);
    }
}
