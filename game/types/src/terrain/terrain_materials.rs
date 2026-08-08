use super::MaterialId;

// Material IDs are stored directly in PlanetVertex and index the GPU terrain
// palette. Keep these values in lockstep with create_terrain_palette.
pub const GRASS: MaterialId = MaterialId(0);
pub const ROCK: MaterialId = MaterialId(1);
pub const WATER: MaterialId = MaterialId(2);
pub const SNOW: MaterialId = MaterialId(3);
pub const MATERIAL_COUNT: usize = 4;
