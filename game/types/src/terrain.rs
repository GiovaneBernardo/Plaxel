use engine::math::Vec3;

use self::terrain_field::TerrainFieldGraph;

pub mod terrain_biomes;
pub mod terrain_climate;
pub mod terrain_density;
pub mod terrain_edits;
pub mod terrain_field;
pub mod terrain_geology;
pub mod terrain_landforms;
pub mod terrain_materials;

pub struct PlanetTerrain {
    pub config: PlanetTerrainConfig,
}

pub struct TerrainSample {
    pub density: f32,
    pub geology: GeologyId,
}

pub struct SurfaceSample {
    pub biome_weights: BiomeWeights,
    pub soil_depth: f32,
    pub rock_exposure: f32,
    pub materials: MaterialBlend,
}

#[derive(Clone, Debug, plaxel_reflect::Reflect)]
pub struct PlanetTerrainConfig {
    pub seed: u64,
    pub radius: f32,
    pub sea_level: f32,
    pub rotation_axis: Vec3,
    pub field_graph: Option<TerrainFieldGraph>,

    pub geology: GeologyConfig,
    pub landforms: LandformConfig,
    pub climate: ClimateConfig,
    pub biomes: BiomeConfig,
    pub features: FeatureConfig,
}

#[derive(Clone, Copy, Debug, plaxel_reflect::Reflect)]
pub struct ClimateConfig {
    pub equator_temperature: f32,
    pub pole_temperature: f32,
    pub altitude_cooling: f32,
    pub humidity_scale: f32,
}

#[derive(Clone, Copy, Debug, plaxel_reflect::Reflect)]
pub struct LandformConfig {
    pub continent_scale: f32,
    pub continent_height: f32,
    pub mountain_height: f32,
    pub mountain_width: f32,
}

#[derive(Clone, Copy, Debug, plaxel_reflect::Reflect)]
pub struct FeatureConfig {
    pub cave_frequency: f32,
    pub cave_size: f32,
    pub overhang_strength: f32,
}

#[derive(Clone, Debug, plaxel_reflect::Reflect)]
pub struct GeologyConfig {
    pub province_scale: f32,
    pub strata_scale: f32,
    pub definitions: Vec<GeologyDefinition>,
}

#[derive(Clone, Debug, plaxel_reflect::Reflect)]
pub struct BiomeConfig {
    pub definitions: Vec<BiomeDefinition>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, plaxel_reflect::Reflect)]
pub struct MaterialId(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GeologyId(pub u16);

#[derive(Clone, Debug, plaxel_reflect::Reflect)]
pub struct GeologyDefinition {
    pub name: String,
    pub hardness: f32,
    pub porosity: f32,
    pub cave_probability: f32,
    pub erosion_resistance: f32,
    pub base_material: MaterialId,
}

pub struct GeologySample {
    pub primary: GeologyId,
    pub secondary: GeologyId,
    pub blend: f32,

    pub hardness: f32,
    pub erosion_resistance: f32,
    pub cave_affinity: f32,
    pub strata_direction: Vec3,
}

pub struct GeologyGenerator {
    pub seed: u64,
    pub province_scale: f32,
    pub strata_scale: f32,
}

pub struct BiomeGenerator {
    pub definitions: Vec<BiomeDefinition>,
}

#[derive(Clone, Copy, Debug, plaxel_reflect::Reflect)]
pub struct BiomeDefinition {
    pub id: BiomeId,
    pub preferred_temperature: f32,
    pub temperature_range: f32,
    pub preferred_humidity: f32,
    pub humidity_range: f32,
    pub minimum_elevation: f32,
    pub maximum_elevation: f32,
}

#[derive(Clone, Copy, Debug, plaxel_reflect::Reflect)]
pub struct BiomeId(pub u16);

pub struct WeightedBiome {
    pub biome: BiomeId,
    pub weight: f32,
}

pub struct BiomeWeights {
    pub entries: [WeightedBiome; 4],
}

pub struct MaterialBlend {
    pub material_a: MaterialId,
    pub material_b: MaterialId,
    pub blend: f32,
}
