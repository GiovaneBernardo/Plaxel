//! Skip only proven uniform cells; keep every possible surface cell exact.
use crate::{
    CHUNK_CELL_COUNT,
    systems::terrain::terrain_sampler::{self, PlanetTerrainSamplerContext},
};
use engine::math::DVec3;
use game_types::terrain::terrain_field::{TerrainFieldGraph, compiled::CompiledTerrainField};

pub(super) fn generate(
    local_min: DVec3,
    resolution: f64,
    terrain: &PlanetTerrainSamplerContext<'_>,
    exact_grid: bool,
    compiled: Option<&CompiledTerrainField>,
) -> (Vec<f32>, usize) {
    generate_with_leaf_size(local_min, resolution, terrain, exact_grid, compiled, 4)
}

fn generate_with_leaf_size(
    local_min: DVec3,
    resolution: f64,
    terrain: &PlanetTerrainSamplerContext<'_>,
    exact_grid: bool,
    compiled: Option<&CompiledTerrainField>,
    leaf_size: usize,
) -> (Vec<f32>, usize) {
    engine::profile_scope!("terrain.grid.generate");
    let size = CHUNK_CELL_COUNT + 2;
    let length = size * size * size;
    let mut grid = vec![0.0; length];
    let mut exact = vec![false; length];
    if let Some(graph) = terrain.config.field_graph.as_ref().filter(|_| !exact_grid) {
        engine::profile_scope!("terrain.grid.classify");
        let mut classifier = Classifier {
            graph,
            radius: f64::from(terrain.config.radius),
            local_min,
            resolution,
            dimensions: [size; 3],
            leaf_size,
            grid: &mut grid,
            exact: &mut exact,
        };
        classifier.visit([0; 3], [size - 1; 3]);
    } else {
        exact.fill(true);
    }
    let required: Vec<_> = exact
        .into_iter()
        .enumerate()
        .filter_map(|(i, exact)| exact.then_some(i))
        .collect();
    let mut positions = Vec::with_capacity(256);
    let mut values = vec![0.0; 256];
    {
        engine::profile_scope!("terrain.grid.sample_density");
        for indices in required.chunks(256) {
            positions.clear();
            for &i in indices {
                positions.push(
                    local_min
                        + DVec3::new(
                            (i / (size * size)) as f64,
                            (i / size % size) as f64,
                            (i % size) as f64,
                        ) * resolution,
                );
            }
            if let Some(compiled) = compiled {
                compiled.densities(
                    &positions,
                    f64::from(terrain.config.radius),
                    &mut values[..indices.len()],
                );
            } else {
                for (&p, value) in positions.iter().zip(&mut values) {
                    *value = terrain_sampler::sample_original_density_planet_local(terrain, p);
                }
            }
            for (&i, &value) in indices.iter().zip(&values) {
                grid[i] = value;
            }
        }
    }
    (grid, required.len())
}

struct Classifier<'a> {
    graph: &'a TerrainFieldGraph,
    radius: f64,
    local_min: DVec3,
    resolution: f64,
    dimensions: [usize; 3],
    leaf_size: usize,
    grid: &'a mut [f32],
    exact: &'a mut [bool],
}
impl Classifier<'_> {
    fn visit(&mut self, lo: [usize; 3], hi: [usize; 3]) {
        let position = |v: [usize; 3]| {
            self.local_min + DVec3::from_array(v.map(|v| v as f64 * self.resolution))
        };
        let bound = self
            .graph
            .density_range_in_box(position(lo), position(hi), self.radius);
        let fill = bound
            .and_then(|b| {
                // Round toward zero so the substitute never exaggerates the
                // distance implied by the proven density bound.
                if b.minimum > 0.0 {
                    Some((b.minimum as f32).next_down().max(0.0))
                } else if b.maximum < 0.0 {
                    Some((b.maximum as f32).next_up().min(0.0))
                } else {
                    None
                }
            })
            .filter(|v| v.is_finite() && *v != 0.0);
        if fill.is_some() || (0..3).all(|a| hi[a] - lo[a] <= self.leaf_size) {
            for x in lo[0]..=hi[0] {
                for y in lo[1]..=hi[1] {
                    for z in lo[2]..=hi[2] {
                        let index = (x * self.dimensions[1] + y) * self.dimensions[2] + z;
                        if let Some(value) = fill {
                            self.grid[index] = value;
                        } else {
                            self.exact[index] = true;
                        }
                    }
                }
            }
            return;
        }
        let mid = std::array::from_fn::<_, 3, _>(|a| (lo[a] + hi[a]) / 2);
        for child in 0..8 {
            let a = std::array::from_fn(|axis| {
                if child & (1 << axis) == 0 {
                    lo[axis]
                } else {
                    mid[axis]
                }
            });
            let b = std::array::from_fn(|axis| {
                if child & (1 << axis) == 0 {
                    mid[axis]
                } else {
                    hi[axis]
                }
            });
            if (0..3).all(|axis| a[axis] < b[axis]) {
                self.visit(a, b);
            }
        }
    }
}

#[cfg(test)]
mod timing_tests {
    use super::*;

    #[test]
    fn classified_grid_preserves_every_surface_cell() {
        let config = crate::systems::universe::planet_system::earth_like_planet_terrain_config();
        let edits = game_types::planet::PlanetTerrainEdits {
            modified_chunks: Default::default(),
            modified_ranges: Default::default(),
        };
        let terrain = PlanetTerrainSamplerContext {
            config: &config,
            edits: &edits,
            planet_position: engine::math::Vec3::ZERO,
        };
        let compiled = config.field_graph.as_ref().unwrap().compile_density();
        let mut lo = f64::from(config.radius) - 100000.0;
        let mut hi = f64::from(config.radius) + 100000.0;
        for _ in 0..48 {
            let mid = (lo + hi) * 0.5;
            if terrain_sampler::sample_original_density_planet_local(&terrain, DVec3::X * mid) < 0.0
            {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let n = CHUNK_CELL_COUNT + 2;
        for size in [32.0, 1024.0, 32768.0] {
            let min = DVec3::new((lo + hi) * 0.5, 0.0, 0.0) - DVec3::splat(size * 0.5);
            let (grid, _) = generate(
                min,
                size / CHUNK_CELL_COUNT as f64,
                &terrain,
                false,
                Some(&compiled),
            );
            let (exact, _) = generate(
                min,
                size / CHUNK_CELL_COUNT as f64,
                &terrain,
                true,
                Some(&compiled),
            );
            let mut surface_cells = 0;
            for x in 0..n - 1 {
                for y in 0..n - 1 {
                    for z in 0..n - 1 {
                        let indices: [usize; 8] = std::array::from_fn(|i| {
                            ((x + (i & 1)) * n + y + ((i >> 1) & 1)) * n + z + ((i >> 2) & 1)
                        });
                        let negative = exact[indices[0]] < 0.0;
                        if indices.iter().any(|&i| (exact[i] < 0.0) != negative) {
                            surface_cells += 1;
                            for i in indices {
                                assert_eq!(grid[i], exact[i], "size={size} surface sample={i}");
                            }
                        }
                    }
                }
            }
            assert!(surface_cells > 0);
        }
        let min = DVec3::new(f64::from(config.radius) - 16.0, -16.0, -16.0);
        let (_, samples) = generate(min, 1.0, &terrain, false, Some(&compiled));
        assert_eq!(
            samples, 0,
            "channel bounds should prove this default-graph box uniform"
        );
    }

    #[test]
    #[ignore = "manual classifier cost comparison"]
    fn compare_classification_leaf_sizes() {
        let config = crate::systems::universe::planet_system::earth_like_planet_terrain_config();
        let edits = game_types::planet::PlanetTerrainEdits {
            modified_chunks: Default::default(),
            modified_ranges: Default::default(),
        };
        let terrain = PlanetTerrainSamplerContext {
            config: &config,
            edits: &edits,
            planet_position: engine::math::Vec3::ZERO,
        };
        let compiled = config.field_graph.as_ref().unwrap().compile_density();
        for size in [32.0, 1024.0, 32768.0, 1048576.0] {
            let origin = DVec3::new(
                f64::from(config.radius) - size * 0.5,
                -size * 0.5,
                -size * 0.5,
            );
            for leaf in [4, 8, 16, 64] {
                let start = std::time::Instant::now();
                let mut samples = 0;
                for _ in 0..5 {
                    let result = generate_with_leaf_size(
                        origin,
                        size / CHUNK_CELL_COUNT as f64,
                        &terrain,
                        false,
                        Some(&compiled),
                        leaf,
                    );
                    samples = result.1;
                    std::hint::black_box(result);
                }
                println!(
                    "size={size} leaf={leaf} samples={samples} mean={:?}",
                    start.elapsed() / 5
                );
            }
        }
    }

    #[test]
    #[ignore = "manual terrain sampling benchmark"]
    fn measure_coarse_grid_sampling() {
        let config = crate::systems::universe::planet_system::earth_like_planet_terrain_config();
        let edits = game_types::planet::PlanetTerrainEdits {
            modified_chunks: Default::default(),
            modified_ranges: Default::default(),
        };
        let terrain = PlanetTerrainSamplerContext {
            config: &config,
            edits: &edits,
            planet_position: engine::math::Vec3::ZERO,
        };
        let compiled = config.field_graph.as_ref().unwrap().compile_density();
        for size in [32.0, 1024.0, 32768.0, 1048576.0] {
            let origin = DVec3::new(
                f64::from(config.radius) - size * 0.5,
                -size * 0.5,
                -size * 0.5,
            );
            for exact in [false, true] {
                let start = std::time::Instant::now();
                let (grid, samples) = generate(
                    origin,
                    size / CHUNK_CELL_COUNT as f64,
                    &terrain,
                    exact,
                    Some(&compiled),
                );
                std::hint::black_box(grid);
                println!(
                    "size={size} exact={exact} samples={samples} elapsed={:?}",
                    start.elapsed()
                );
            }
        }
    }
}
