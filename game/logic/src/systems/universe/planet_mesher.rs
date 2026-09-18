use std::sync::Arc;
pub(crate) mod cache;
mod sampling;

use engine::{ecs::entity::Entity, game_info, math::dvec3};
use game_types::{
    octree::{GeneratedMesh, NodeKey, PlanetMeshRequest},
    planet::{Planet, PlanetTerrainEdits, PlanetVertex},
    terrain::PlanetTerrainConfig,
};

use crate::{
    CHUNK_CELL_COUNT,
    systems::{
        DensityGrid, PlanetExt,
        planet_mesher::cache::DensityCache,
        terrain::terrain_sampler::{self, PlanetTerrainSamplerContext},
    },
};

// Generate mesh
pub fn generate_planet_node_mesh(
    request: &PlanetMeshRequest,
    terrain_config: Arc<PlanetTerrainConfig>,
    edits: Arc<PlanetTerrainEdits>,
    cache: &DensityCache,
) -> GeneratedMesh {
    let terrain = PlanetTerrainSamplerContext {
        config: &cache.config,
        edits: &edits,
        planet_position: request.planet_position,
    };
    let resolution = request.node_size / CHUNK_CELL_COUNT as f32;
    // Include the positive ghost cells used to join neighboring chunks.
    let size = CHUNK_CELL_COUNT + 2;
    let local_min = request.node_min_corner.as_dvec3() - request.planet_position.as_dvec3();
    let base = cache.grid(request, &edits);
    let mut grid = Arc::clone(&base);
    if !edits.modified_chunks.is_empty() {
        for (i, density) in Arc::make_mut(&mut grid).iter_mut().enumerate() {
            let position = local_min
                + dvec3(
                    (i / (size * size)) as f64,
                    (i / size % size) as f64,
                    (i % size) as f64,
                ) * f64::from(resolution);
            *density +=
                terrain_sampler::sample_terrain_edits_density_planet_local(&terrain, position);
        }
    }

    let (vertices, indices) = Planet::dual_contour_grid(
        &grid,
        request.node_min_corner,
        resolution,
        &PlanetTerrainSamplerContext {
            config: &terrain_config,
            edits: &edits,
            planet_position: request.planet_position,
        },
        &request.face_neighbors,
    );
    let generated_mesh = GeneratedMesh {
        generation: 0,
        planet_entity: request.planet_entity,
        key: request.node_key,
        node_origin_planet: [local_min.x as i32, local_min.y as i32, local_min.z as i32],
        version: 0,
        urgent: false,
        vertices,
        indices,
    };
    generated_mesh
}

// Cook it into a physical collider

// Upload mesh to gpu
pub fn upload_mesh(mesh: &GeneratedMesh) {}

// Unload mesh from gpu
pub fn remove_mesh(planet_entity: Entity, key: NodeKey) {}
