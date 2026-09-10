use std::sync::Arc;

use engine::ecs::entity::Entity;
use game_types::{
    octree::{GeneratedMesh, NodeKey, PlanetMeshRequest},
    terrain::PlanetTerrainConfig,
};

// Generate mesh
pub fn generate_planet_node_mesh(
    request: &PlanetMeshRequest,
    terrain_config: Arc<PlanetTerrainConfig>,
) -> GeneratedMesh {
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
        vertices: Vec::new(),
        indices: Vec::new(),
    };
    generated_mesh
}

// Cook it into a physical collider

// Upload mesh to gpu
pub fn upload_mesh(mesh: &GeneratedMesh) {}

// Unload mesh from gpu
pub fn remove_mesh(planet_entity: Entity, key: NodeKey) {}
