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
    terrain::{
        PlanetTerrainConfig,
        terrain_field::{TerrainFieldGraph, compiled::CompiledTerrainField},
    },
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
    uniform: HashMap<(NodeKey, u32), Option<UniformRegion>>,
}

pub(crate) struct DensityCache {
    pub config: Arc<PlanetTerrainConfig>,
    pub(super) compiled: Option<CompiledTerrainField>,
    entries: Mutex<GridEntries>,
    pub hits: AtomicU64,
    pub samples: AtomicU64,
    pub misses: AtomicU64,
    pub evictions: AtomicU64,
    pub uniform_hits: AtomicU64,
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
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
            uniform_hits: AtomicU64::new(0),
        }
    }

    // A proof covers the entire padded grid, not just its sampled points.
    // Coarser neighbors need additional seam samples outside this box.
    pub(super) fn uniform_region(
        &self,
        request: &PlanetMeshRequest,
        edits: &PlanetTerrainEdits,
    ) -> Option<UniformRegion> {
        use game_types::octree::FaceNeighborKind;
        if !edits.modified_chunks.is_empty()
            || request
                .face_neighbors
                .iter()
                .any(|n| n.kind == FaceNeighborKind::Coarser)
        {
            return None;
        }
        let key = (request.node_key, request.node_size.to_bits());
        if let Some(kind) = self.entries.lock().unwrap().uniform.get(&key).copied() {
            if kind.is_some() {
                self.uniform_hits.fetch_add(1, Ordering::Relaxed);
            }
            return kind;
        }
        engine::profile_scope!("terrain.mesh.prove_uniform");
        let graph = self.config.field_graph.as_ref()?;
        let min = request.node_min_corner.as_dvec3() - request.planet_position.as_dvec3();
        let spacing = f64::from(request.node_size / CHUNK_CELL_COUNT as f32);
        let max = min + DVec3::splat(spacing * (CHUNK_CELL_COUNT + 1) as f64);
        let kind = prove_uniform(graph, f64::from(self.config.radius), min, max, 0);
        self.entries.lock().unwrap().uniform.insert(key, kind);
        kind
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
        self.misses.fetch_add(1, Ordering::Relaxed);
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
                    self.evictions.fetch_add(1, Ordering::Relaxed);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UniformRegion {
    Air,
    Solid,
}

fn uniform_box(
    graph: &TerrainFieldGraph,
    radius: f64,
    min: DVec3,
    max: DVec3,
) -> Option<UniformRegion> {
    let range = graph.density_range_in_box(min, max, radius)?;
    if range.minimum > 0.0 {
        Some(UniformRegion::Air)
    } else if range.maximum < 0.0 {
        Some(UniformRegion::Solid)
    } else {
        None
    }
}

fn prove_uniform(
    graph: &TerrainFieldGraph,
    radius: f64,
    min: DVec3,
    max: DVec3,
    depth: u8,
) -> Option<UniformRegion> {
    if let Some(kind) = uniform_box(graph, radius, min, max) {
        return Some(kind);
    }

    if depth == 3 {
        return None;
    }

    let mid = (min + max) * 0.5;
    let mut result = None;

    for child in 0..8 {
        let child_min = DVec3::from_array(std::array::from_fn(|axis| {
            if child & (1 << axis) == 0 {
                min[axis]
            } else {
                mid[axis]
            }
        }));

        let child_max = DVec3::from_array(std::array::from_fn(|axis| {
            if child & (1 << axis) == 0 {
                mid[axis]
            } else {
                max[axis]
            }
        }));

        let kind = prove_uniform(graph, radius, child_min, child_max, depth + 1)?;

        if result.is_some_and(|previous| previous != kind) {
            return None;
        }

        result = Some(kind);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_types::{octree::FaceNeighbor, planet::TerrainBrickKey};

    #[test]
    fn uniform_proofs_are_continuous_cached_and_edit_safe() {
        let mut config =
            crate::systems::universe::planet_system::earth_like_planet_terrain_config();
        config.radius = 100.0;
        config.field_graph.as_mut().unwrap().layers.clear();
        config.field_graph.as_mut().unwrap().radius = 100.0;
        let cache = DensityCache::new(Arc::new(config));
        let mut edits = PlanetTerrainEdits {
            modified_chunks: HashMap::new(),
            modified_ranges: HashMap::new(),
        };
        let mut request = PlanetMeshRequest {
            planet_entity: engine::ecs::entity::Entity::PLACEHOLDER,
            node_key: NodeKey {
                level: 0,
                x: 200,
                y: 0,
                z: 0,
            },
            planet_position: Vec3::ZERO,
            node_min_corner: Vec3::new(200.0, 0.0, 0.0),
            node_size: 32.0,
            face_neighbors: [FaceNeighbor::SAME_OR_ABSENT; 6],
        };
        assert_eq!(
            cache.uniform_region(&request, &edits),
            Some(UniformRegion::Air)
        );
        assert_eq!(
            cache.uniform_region(&request, &edits),
            Some(UniformRegion::Air)
        );
        assert_eq!(cache.uniform_hits.load(Ordering::Relaxed), 1);
        assert_eq!(cache.samples.load(Ordering::Relaxed), 0);
        edits.modified_chunks.insert(
            TerrainBrickKey {
                level: 0,
                x: 6,
                y: 0,
                z: 0,
            },
            Arc::new(vec![]),
        );
        assert_eq!(cache.uniform_region(&request, &edits), None);
        edits.modified_chunks.clear();
        request.face_neighbors[0].kind = game_types::octree::FaceNeighborKind::Coarser;
        assert_eq!(cache.uniform_region(&request, &edits), None);
        let graph = cache.config.field_graph.as_ref().unwrap();
        assert_eq!(
            prove_uniform(graph, 100.0, DVec3::ZERO, DVec3::splat(1.0), 0),
            Some(UniformRegion::Solid)
        );
        // All eight corners are outside this sphere, but its interior crosses zero.
        assert_eq!(
            prove_uniform(graph, 100.0, DVec3::splat(-110.0), DVec3::splat(110.0), 0),
            None
        );
        let mut changed = (*cache.config).clone();
        changed.radius = 1000.0;
        assert_eq!(
            DensityCache::new(Arc::new(changed))
                .uniform_region(&request_with_no_seam(request), &edits),
            Some(UniformRegion::Solid)
        );
    }

    fn request_with_no_seam(mut request: PlanetMeshRequest) -> PlanetMeshRequest {
        request.face_neighbors = [FaceNeighbor::SAME_OR_ABSENT; 6];
        request
    }

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
