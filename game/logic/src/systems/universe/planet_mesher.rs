use std::sync::Arc;

use engine::{ecs::entity::Entity, game_info};
use game_types::{
    octree::{GeneratedMesh, NodeKey, PlanetMeshRequest},
    planet::{Planet, PlanetTerrainEdits, PlanetVertex},
    terrain::PlanetTerrainConfig,
};

use crate::systems::{
    DensityGrid, PlanetExt, terrain::terrain_sampler::PlanetTerrainSamplerContext,
};

// Generate mesh
pub fn generate_planet_node_mesh(
    request: &PlanetMeshRequest,
    terrain_config: Arc<PlanetTerrainConfig>,
    edits: Arc<PlanetTerrainEdits>,
) -> GeneratedMesh {
    game_info!("Testeeee");
    let grid = DensityGrid::new();
    let (vertices, indices) = Planet::dual_contour_grid(
        &grid,
        request.node_min_corner,
        request.node_size,
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
        node_origin_planet: [
            request.planet_position.x as i32,
            request.planet_position.y as i32,
            request.planet_position.z as i32,
        ],
        version: 0,
        urgent: false,
        vertices: vertices,
        indices: indices,
    };
    generated_mesh
}

// Cook it into a physical collider

// Upload mesh to gpu
pub fn upload_mesh(mesh: &GeneratedMesh) {}

// Unload mesh from gpu
pub fn remove_mesh(planet_entity: Entity, key: NodeKey) {}
