use std::sync::Arc;

use engine::math::{DVec3, Vec3};
use game_types::{
    planet::PlanetTerrainEdits,
    terrain::{
        BiomeId, BiomeWeights, MaterialBlend, MaterialId, PlanetTerrainConfig, SurfaceSample,
        WeightedBiome,
    },
};

use crate::sdf::{base_sdf_planet_local, sample_terrain_edit};

pub struct PlanetTerrainSnapshot {
    pub config: Arc<PlanetTerrainConfig>,
    pub edits: PlanetTerrainEdits,
    pub planet_position: Vec3,
}

#[derive(Clone, Copy)]
pub struct PlanetTerrainSamplerContext<'a> {
    pub config: &'a PlanetTerrainConfig,
    pub edits: &'a PlanetTerrainEdits,
    pub planet_position: Vec3,
}

impl PlanetTerrainSnapshot {
    pub fn sampler_context(&self) -> PlanetTerrainSamplerContext<'_> {
        PlanetTerrainSamplerContext {
            config: self.config.as_ref(),
            edits: &self.edits,
            planet_position: self.planet_position,
        }
    }
}

pub fn sample_original_density(
    terrain: &PlanetTerrainSamplerContext<'_>,
    world_position: Vec3,
) -> f32 {
    sample_original_density_planet_local(
        terrain,
        world_position.as_dvec3() - terrain.planet_position.as_dvec3(),
    )
}

pub fn sample_original_density_planet_local(
    terrain: &PlanetTerrainSamplerContext<'_>,
    planet_local_position: DVec3,
) -> f32 {
    base_sdf_planet_local(planet_local_position, terrain.config, None)
}

pub fn sample_final_density(
    terrain: &PlanetTerrainSamplerContext<'_>,
    world_position: Vec3,
) -> f32 {
    sample_final_density_planet_local(
        terrain,
        world_position.as_dvec3() - terrain.planet_position.as_dvec3(),
    )
}

pub fn sample_final_density_planet_local(
    terrain: &PlanetTerrainSamplerContext<'_>,
    planet_local_position: DVec3,
) -> f32 {
    sample_original_density_planet_local(terrain, planet_local_position)
        + sample_terrain_edits_density_planet_local(terrain, planet_local_position)
}

pub fn sample_terrain_edits_density(
    terrain: &PlanetTerrainSamplerContext<'_>,
    world_position: Vec3,
) -> f32 {
    sample_terrain_edits_density_planet_local(
        terrain,
        world_position.as_dvec3() - terrain.planet_position.as_dvec3(),
    )
}

pub fn sample_terrain_edits_density_planet_local(
    terrain: &PlanetTerrainSamplerContext<'_>,
    planet_local_position: DVec3,
) -> f32 {
    sample_terrain_edit(planet_local_position.as_vec3(), terrain.edits)
}

pub fn sample_surface(
    _terrain: &PlanetTerrainSamplerContext<'_>,
    _local_position: Vec3,
    _normal: Vec3,
) -> SurfaceSample {
    let entries = [
        WeightedBiome {
            biome: BiomeId(0),
            weight: 1.0,
        },
        WeightedBiome {
            biome: BiomeId(0),
            weight: 1.0,
        },
        WeightedBiome {
            biome: BiomeId(0),
            weight: 1.0,
        },
        WeightedBiome {
            biome: BiomeId(0),
            weight: 1.0,
        },
    ];

    let materials = MaterialBlend {
        material_a: MaterialId(0),
        material_b: MaterialId(1),
        blend: 0.5,
    };

    SurfaceSample {
        biome_weights: BiomeWeights { entries },
        soil_depth: 0.5,
        rock_exposure: 0.5,
        materials,
    }
}

pub fn is_terrain_edits_empty(terrain: &PlanetTerrainSamplerContext<'_>) -> bool {
    terrain.edits.modified_chunks.is_empty()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use engine::math::vec3;

    use super::*;
    use crate::systems::universe::planet_system::default_planet_terrain_config;

    fn empty_edits() -> PlanetTerrainEdits {
        PlanetTerrainEdits {
            modified_chunks: HashMap::new(),
            modified_ranges: HashMap::new(),
        }
    }

    #[test]
    fn configured_radius_is_used_directly() {
        let mut config = default_planet_terrain_config();
        config.radius = 100.0;
        config.landforms.continent_height = 0.0;
        config.landforms.mountain_height = 0.0;
        config.features.overhang_strength = 0.0;
        config.features.cave_frequency = 0.0;
        config.features.cave_size = 0.0;
        let edits = empty_edits();
        let terrain = PlanetTerrainSamplerContext {
            config: &config,
            edits: &edits,
            planet_position: Vec3::ZERO,
        };

        assert!(sample_original_density(&terrain, Vec3::X * 50.0) < 0.0);
        assert!(sample_original_density(&terrain, Vec3::X * 200.0) > 0.0);
    }

    #[test]
    fn sampling_is_translation_invariant_between_planets() {
        let mut config = default_planet_terrain_config();
        config.radius = 100.0;
        let edits = empty_edits();
        let offset = vec3(1250.0, -340.0, 980.0);
        let local_sample = vec3(80.0, 20.0, -5.0);
        let origin_terrain = PlanetTerrainSamplerContext {
            config: &config,
            edits: &edits,
            planet_position: Vec3::ZERO,
        };
        let translated_terrain = PlanetTerrainSamplerContext {
            config: &config,
            edits: &edits,
            planet_position: offset,
        };

        let at_origin = sample_original_density(&origin_terrain, local_sample);
        let translated = sample_original_density(&translated_terrain, offset + local_sample);
        assert!((at_origin - translated).abs() < 1e-4);
    }
}
