use engine::prelude::*;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use game_types::{
    octree::{NodeKey, PlanetMeshRequest},
    planet::PlanetTerrainEdits,
    terrain::{PlanetTerrainConfig, terrain_field::compiled::CompiledTerrainField},
};

use crate::{
    CHUNK_CELL_COUNT,
    systems::{planet_mesher::sampling, terrain::terrain_sampler::PlanetTerrainSamplerContext},
};

type GridKey = (NodeKey, u32, bool);
#[derive(Default)]
struct GridEntries {
    grids: HashMap<GridKey, Arc<Vec<f32>>>,
    order: VecDeque<GridKey>,
}

pub(crate) struct DensityCache {
    pub config: Arc<PlanetTerrainConfig>,
    pub(super) compiled: Option<CompiledTerrainField>,
    entries: Mutex<GridEntries>,
    pub hits: AtomicU64,
    pub samples: AtomicU64,
}

impl DensityCache {
    pub fn new(config: Arc<PlanetTerrainConfig>) -> Self {
        let compiled = config
            .field_graph
            .as_ref()
            .map(|graph| graph.compile_density());
        Self {
            config,
            compiled,
            entries: Mutex::new(GridEntries::default()),
            hits: AtomicU64::new(0),
            samples: AtomicU64::new(0),
        }
    }

    pub(super) fn grid(
        &self,
        request: &PlanetMeshRequest,
        edits: &PlanetTerrainEdits,
    ) -> (Arc<Vec<f32>>, bool) {
        // Approximate uniform samples are never used with edits: adding a
        // density delta to a substitute value could move the edited surface.
        let exact = grid_has_edits(request, edits);
        let key = (request.node_key, request.node_size.to_bits(), exact);
        if let Some(grid) = self.entries.lock().unwrap().grids.get(&key).cloned() {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return (grid, exact);
        }
        let terrain = PlanetTerrainSamplerContext {
            config: &self.config,
            edits,
            planet_position: request.planet_position,
        };
        let (grid, samples) = sampling::generate(
            request.node_min_corner.as_dvec3() - request.planet_position.as_dvec3(),
            f64::from(request.node_size / CHUNK_CELL_COUNT as f32),
            &terrain,
            exact,
            self.compiled.as_ref(),
        );
        self.samples.fetch_add(samples as u64, Ordering::Relaxed);
        let grid = Arc::new(grid);
        let mut entries = self.entries.lock().unwrap();
        if !entries.grids.contains_key(&key) {
            // 256 grids are about 39 MiB. No lock is held while sampling.
            if entries.order.len() >= 256 {
                if let Some(old) = entries.order.pop_front() {
                    entries.grids.remove(&old);
                }
            }
            entries.order.push_back(key);
            entries.grids.insert(key, Arc::clone(&grid));
        }
        (grid, exact)
    }
}

// Include the positive ghost samples used to mesh chunk boundaries. Use the
// same f32 brick lookup as sample_terrain_edit, including negative coordinates.
fn grid_has_edits(request: &PlanetMeshRequest, edits: &PlanetTerrainEdits) -> bool {
    let min = request.node_min_corner.as_dvec3() - request.planet_position.as_dvec3();
    let spacing = f64::from(request.node_size / CHUNK_CELL_COUNT as f32);
    let max = min + DVec3::splat(spacing * (CHUNK_CELL_COUNT + 1) as f64);
    let first = (min.as_vec3() / crate::sdf::TERRAIN_EDIT_BRICK_SIZE)
        .floor()
        .as_ivec3();
    let last = (max.as_vec3() / crate::sdf::TERRAIN_EDIT_BRICK_SIZE)
        .floor()
        .as_ivec3();
    edits.modified_chunks.keys().any(|key| {
        key.level == 0
            && key.x >= first.x
            && key.x <= last.x
            && key.y >= first.y
            && key.y <= last.y
            && key.z >= first.z
            && key.z <= last.z
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_types::{octree::FaceNeighbor, planet::TerrainBrickKey};

    #[test]
    fn edit_bounds_include_ghost_samples_and_negative_bricks() {
        let mut request = PlanetMeshRequest {
            planet_entity: engine::ecs::entity::Entity::PLACEHOLDER,
            node_key: NodeKey {
                level: 0,
                x: 0,
                y: 0,
                z: 0,
            },
            planet_position: Vec3::splat(1024.0),
            node_min_corner: Vec3::splat(1024.0),
            node_size: 31.5,
            face_neighbors: [FaceNeighbor::SAME_OR_ABSENT; 6],
        };
        let mut edits = PlanetTerrainEdits {
            modified_chunks: HashMap::new(),
            modified_ranges: HashMap::new(),
        };
        for (x, expected) in [(0, true), (1, true), (2, false), (-1, false)] {
            edits.modified_chunks.clear();
            edits.modified_chunks.insert(
                TerrainBrickKey {
                    level: 0,
                    x,
                    y: 0,
                    z: 0,
                },
                Arc::new(vec![]),
            );
            assert_eq!(grid_has_edits(&request, &edits), expected, "brick {x}");
        }
        request.node_min_corner.x -= 1.0;
        assert!(grid_has_edits(&request, &edits));
    }
}
