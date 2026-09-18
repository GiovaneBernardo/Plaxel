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
    compiled: Option<CompiledTerrainField>,
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
    ) -> Arc<Vec<f32>> {
        // Approximate uniform samples are never used with edits: adding a
        // density delta to a substitute value could move the edited surface.
        let exact = !edits.modified_chunks.is_empty();
        let key = (request.node_key, request.node_size.to_bits(), exact);
        if let Some(grid) = self.entries.lock().unwrap().grids.get(&key).cloned() {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return grid;
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
        grid
    }
}
