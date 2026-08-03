use engine::math::{DVec3, Vec3, vec3};
use game_types::planet::{PlanetTerrainEdits, TerrainBrickKey, TerrainBrickSamples};

const EARTH_MEAN_RADIUS_METERS: f32 = 6_371_000.0;
const EARTH_HIGHEST_ALTITUDE_METERS: f32 = 8_848.86;
const EARTH_HEIGHT_EXAGGERATION: f32 = 16.0; //32.0;
const EARTH_BROAD_DETAIL_SCALE: f32 = 0.18;
const EARTH_RIDGE_DETAIL_SCALE: f32 = 0.12;
const EARTH_FINE_DETAIL_SCALE: f32 = 0.035;
pub const TERRAIN_EDIT_BRICK_SIZE: f32 = 32.0;
pub const TERRAIN_EDIT_CELL_COUNT: usize = 16;
pub const TERRAIN_EDIT_SAMPLE_COUNT: usize = TERRAIN_EDIT_CELL_COUNT + 1;
const TERRAIN_EDIT_LEVEL: u32 = 0;

pub struct EarthHeightmap {
    pub width: u32,
    pub height: u32,
    pub samples: Vec<f32>,
    pub min_height: f32,
    pub max_height: f32,
}

impl EarthHeightmap {
    pub fn sample_unit_height(&self, dir: Vec3) -> Option<f32> {
        if self.width == 0 || self.height == 0 {
            return None;
        }

        let expected_len = self.width as usize * self.height as usize;
        if self.samples.len() < expected_len {
            return None;
        }

        let dir = if dir.length_squared() > 1e-12 {
            dir.normalize()
        } else {
            vec3(0.0, 1.0, 0.0)
        };

        let lon = dir.z.atan2(dir.x);
        let lat = dir.y.clamp(-1.0, 1.0).asin();
        let u = (0.5 - lon / std::f32::consts::TAU).rem_euclid(1.0);
        let v = (0.5 - lat / std::f32::consts::PI).clamp(0.0, 1.0);

        let x = u * self.width as f32;
        let y = v * (self.height - 1) as f32;
        let x0 = x.floor() as u32 % self.width;
        let x1 = (x0 + 1) % self.width;
        let y0 = y.floor() as u32;
        let y1 = (y0 + 1).min(self.height - 1);
        let tx = x - x.floor();
        let ty = y - y.floor();

        let sample =
            |x: u32, y: u32| -> f32 { self.samples[y as usize * self.width as usize + x as usize] };

        let top = lerp(sample(x0, y0), sample(x1, y0), tx);
        let bottom = lerp(sample(x0, y1), sample(x1, y1), tx);
        Some(lerp(top, bottom, ty))
    }

    pub fn sample_height(&self, dir: Vec3, planet_r: f32) -> Option<f32> {
        let dir = if dir.length_squared() > 1e-12 {
            dir.normalize()
        } else {
            vec3(0.0, 1.0, 0.0)
        };
        let sampled_height = self.sample_unit_height(dir)?;
        let height_scale = planet_r
            * (EARTH_HIGHEST_ALTITUDE_METERS / EARTH_MEAN_RADIUS_METERS)
            * EARTH_HEIGHT_EXAGGERATION;
        let land_mask = smoothstep(((sampled_height - 0.015) / 0.08).clamp(0.0, 1.0));
        let mountain_mask = smoothstep(((sampled_height - 0.24) / 0.42).clamp(0.0, 1.0));

        let broad = (fbm(dir * 42.0 + vec3(9.7, 31.2, 4.4), 4) - 0.5)
            * height_scale
            * EARTH_BROAD_DETAIL_SCALE
            * land_mask;
        let ridge_raw = fbm(dir * 155.0 + vec3(43.1, 7.8, 91.4), 5);
        let ridges = (1.0 - (ridge_raw * 2.0 - 1.0).abs()).powf(1.7)
            * height_scale
            * EARTH_RIDGE_DETAIL_SCALE
            * mountain_mask;
        let fine = (fbm(dir * 520.0 + vec3(3.2, 77.5, 18.6), 3) - 0.5)
            * height_scale
            * EARTH_FINE_DETAIL_SCALE
            * land_mask;
        let ocean_depression = if sampled_height == 0.0 { -2.0 } else { 0.0 };

        Some(sampled_height * height_scale + broad + ridges + fine + ocean_depression)
    }
}

#[inline(always)]
pub fn hash3(p: Vec3) -> f32 {
    let ix = (p.x.floor() as i32).wrapping_mul(1619);
    let iy = (p.y.floor() as i32).wrapping_mul(31337);
    let iz = (p.z.floor() as i32).wrapping_mul(6271);
    let n = ix.wrapping_add(iy).wrapping_add(iz);
    let n = n.wrapping_mul(n.wrapping_mul(n).wrapping_mul(60493).wrapping_add(19990303));
    (n as u32 as f32) / (u32::MAX as f32)
}

#[inline(always)]
pub fn hash3i(x: i32, y: i32, z: i32) -> f32 {
    let n = x
        .wrapping_mul(1619)
        .wrapping_add(y.wrapping_mul(31337))
        .wrapping_add(z.wrapping_mul(6271));

    let n = n.wrapping_mul(n.wrapping_mul(n).wrapping_mul(60493).wrapping_add(19990303));

    (n as u32 as f32) * (1.0 / u32::MAX as f32)
}

#[inline(always)]
pub fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

#[inline(always)]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + t * (b - a)
}

#[inline(always)]
pub fn smooth_noise(p: Vec3) -> f32 {
    let ix = p.x.floor() as i32;
    let iy = p.y.floor() as i32;
    let iz = p.z.floor() as i32;

    let fx = p.x - ix as f32;
    let fy = p.y - iy as f32;
    let fz = p.z - iz as f32;

    let ux = smoothstep(fx);
    let uy = smoothstep(fy);
    let uz = smoothstep(fz);

    let v000 = hash3i(ix, iy, iz);
    let v100 = hash3i(ix + 1, iy, iz);
    let v010 = hash3i(ix, iy + 1, iz);
    let v110 = hash3i(ix + 1, iy + 1, iz);
    let v001 = hash3i(ix, iy, iz + 1);
    let v101 = hash3i(ix + 1, iy, iz + 1);
    let v011 = hash3i(ix, iy + 1, iz + 1);
    let v111 = hash3i(ix + 1, iy + 1, iz + 1);

    let x00 = lerp(v000, v100, ux);
    let x10 = lerp(v010, v110, ux);
    let x01 = lerp(v001, v101, ux);
    let x11 = lerp(v011, v111, ux);

    let y0 = lerp(x00, x10, uy);
    let y1 = lerp(x01, x11, uy);

    lerp(y0, y1, uz)
}

pub fn fbm(p: Vec3, octaves: u32) -> f32 {
    let mut value = 0.0f32;
    let mut amplitude = 0.5f32;
    let mut frequency = 1.0f32;
    for _ in 0..octaves {
        value += amplitude * smooth_noise(p * frequency);
        frequency *= 2.0;
        amplitude *= 0.5;
    }
    value
}

fn sample_terrain_brick(brick: &TerrainBrickSamples, uvw: Vec3) -> f32 {
    let resolution = brick.len();
    if resolution == 0 {
        return 0.0;
    }

    let max_index = resolution.saturating_sub(1);
    if max_index == 0 {
        return brick[0]
            .get(0)
            .and_then(|plane| plane.get(0))
            .copied()
            .unwrap_or(0.0);
    }

    let sample_axis = |v: f32| {
        let coord = v.clamp(0.0, 1.0) * max_index as f32;
        let i0 = coord.floor() as usize;
        let i1 = (i0 + 1).min(max_index);
        let t = coord - i0 as f32;
        (i0, i1, t)
    };

    let (x0, x1, tx) = sample_axis(uvw.x);
    let (y0, y1, ty) = sample_axis(uvw.y);
    let (z0, z1, tz) = sample_axis(uvw.z);

    let sample = |x: usize, y: usize, z: usize| {
        brick
            .get(x)
            .and_then(|plane| plane.get(y))
            .and_then(|row| row.get(z))
            .copied()
            .unwrap_or(0.0)
    };

    let c000 = sample(x0, y0, z0);
    let c100 = sample(x1, y0, z0);
    let c010 = sample(x0, y1, z0);
    let c110 = sample(x1, y1, z0);
    let c001 = sample(x0, y0, z1);
    let c101 = sample(x1, y0, z1);
    let c011 = sample(x0, y1, z1);
    let c111 = sample(x1, y1, z1);

    let c00 = lerp(c000, c100, tx);
    let c10 = lerp(c010, c110, tx);
    let c01 = lerp(c001, c101, tx);
    let c11 = lerp(c011, c111, tx);
    let c0 = lerp(c00, c10, ty);
    let c1 = lerp(c01, c11, ty);

    lerp(c0, c1, tz)
}

pub fn resample_terrain_edit_brick(
    brick: &TerrainBrickSamples,
    sample_count: usize,
) -> TerrainBrickSamples {
    if sample_count == 0 {
        return Vec::new();
    }
    if brick.len() == sample_count
        && brick.iter().all(|plane| {
            plane.len() == sample_count && plane.iter().all(|row| row.len() == sample_count)
        })
    {
        return brick.clone();
    }

    let denominator = sample_count.saturating_sub(1).max(1) as f32;
    (0..sample_count)
        .map(|x| {
            (0..sample_count)
                .map(|y| {
                    (0..sample_count)
                        .map(|z| {
                            sample_terrain_brick(
                                brick,
                                vec3(
                                    x as f32 / denominator,
                                    y as f32 / denominator,
                                    z as f32 / denominator,
                                ),
                            )
                        })
                        .collect()
                })
                .collect()
        })
        .collect()
}

pub fn sample_terrain_edit(local_p: Vec3, terrain_edits: &PlanetTerrainEdits) -> f32 {
    let key = TerrainBrickKey {
        x: (local_p.x / TERRAIN_EDIT_BRICK_SIZE).floor() as i32,
        y: (local_p.y / TERRAIN_EDIT_BRICK_SIZE).floor() as i32,
        z: (local_p.z / TERRAIN_EDIT_BRICK_SIZE).floor() as i32,
        level: TERRAIN_EDIT_LEVEL,
    };

    let Some(brick) = terrain_edits.modified_chunks.get(&key) else {
        return 0.0;
    };

    let brick_min = vec3(
        key.x as f32 * TERRAIN_EDIT_BRICK_SIZE,
        key.y as f32 * TERRAIN_EDIT_BRICK_SIZE,
        key.z as f32 * TERRAIN_EDIT_BRICK_SIZE,
    );
    sample_terrain_brick(brick, (local_p - brick_min) / TERRAIN_EDIT_BRICK_SIZE)
}

// This already accounts for terrain edits (better for gameplay code most of the time)
pub fn sdf_at_center(
    p: engine::math::Vec3,
    planet_center: engine::math::Vec3,
    planet_radius: f32,
    heightmap: Option<&EarthHeightmap>,
    terrain_edits: &PlanetTerrainEdits,
) -> f32 {
    let local_p = p - planet_center;
    base_sdf_at_center(p, planet_center, planet_radius, heightmap)
        + sample_terrain_edit(local_p, terrain_edits)
}

// This doesnt takes into account terrain edits
pub fn base_sdf_at_center(
    p: engine::math::Vec3,
    planet_center: engine::math::Vec3,
    planet_radius: f32,
    heightmap: Option<&EarthHeightmap>,
) -> f32 {
    base_sdf_planet_local(
        p.as_dvec3() - planet_center.as_dvec3(),
        f64::from(planet_radius),
        heightmap,
    )
}

pub fn base_sdf_planet_local(
    local_p: DVec3,
    planet_radius: f64,
    heightmap: Option<&EarthHeightmap>,
) -> f32 {
    let dist_from_center = local_p.length();
    let dir = if dist_from_center > 1e-6 {
        local_p / dist_from_center
    } else {
        engine::math::dvec3(0.0, 1.0, 0.0)
    };

    let height = heightmap
        .and_then(|heightmap| {
            heightmap
                .sample_height(dir.as_vec3(), planet_radius as f32)
                .map(f64::from)
        })
        .unwrap_or_else(|| spherical_terrain_height_f64(dir, planet_radius));
    (dist_from_center - (planet_radius + height)) as f32

    // let depth_below_surface = -terrain;
    // let fade_zone = planet_r * 0.1;
    // let cave_blend = (depth_below_surface / fade_zone).clamp(0.0, 1.0);
    // if cave_blend > 0.0 {
    //     let cave = cave_sdf(p);
    //     let carved = terrain.max(-cave);
    //     terrain + (carved - terrain) * cave_blend
    // } else {
    //     terrain
    // }
}

pub fn min_terrain_height(planet_radius: f32) -> f32 {
    -planet_radius * 0.011
}

pub fn max_terrain_height(planet_radius: f32) -> f32 {
    planet_radius * 0.045
}

/// Conservative lower and upper bounds for terrain height above the base
/// planet radius. Keeping this separate from SDF sampling lets the octree
/// classify whole regions without evaluating procedural noise.
pub fn terrain_height_bounds(planet_radius: f32, heightmap: Option<&EarthHeightmap>) -> (f32, f32) {
    let Some(heightmap) = heightmap.filter(|heightmap| {
        heightmap.width > 0
            && heightmap.height > 0
            && heightmap.samples.len() >= heightmap.width as usize * heightmap.height as usize
            && heightmap.min_height.is_finite()
            && heightmap.max_height.is_finite()
    }) else {
        return (
            min_terrain_height(planet_radius),
            max_terrain_height(planet_radius),
        );
    };

    let height_scale = planet_radius
        * (EARTH_HIGHEST_ALTITUDE_METERS / EARTH_MEAN_RADIUS_METERS)
        * EARTH_HEIGHT_EXAGGERATION;

    // Four FBM octaves lie in [0, 0.9375], and three lie in [0, 0.875].
    // Include the complete broad, ridge, fine, and ocean-depression ranges.
    let min_detail =
        (-0.5 * EARTH_BROAD_DETAIL_SCALE - 0.5 * EARTH_FINE_DETAIL_SCALE) * height_scale;
    let max_detail = (0.4375 * EARTH_BROAD_DETAIL_SCALE
        + EARTH_RIDGE_DETAIL_SCALE
        + 0.375 * EARTH_FINE_DETAIL_SCALE)
        * height_scale;

    (
        heightmap.min_height * height_scale + min_detail - 2.0,
        heightmap.max_height * height_scale + max_detail,
    )
}

pub fn spherical_terrain_height(dir: Vec3, planet_r: f32) -> f32 {
    let warp = vec3(
        fbm(dir * 3.0 + vec3(17.1, 3.7, 11.5), 4),
        fbm(dir * 3.0 + vec3(5.3, 19.1, 2.8), 4),
        fbm(dir * 3.0 + vec3(13.8, 7.4, 23.6), 4),
    ) * 2.0
        - vec3(1.0, 1.0, 1.0);
    let warped_dir = (dir + warp * 0.18).normalize();

    let continent = fbm(warped_dir * 2.2, 5);
    let continent_height = (continent - 0.45) * planet_r * 0.018;
    let mountain_mask = ((continent - 0.38) / 0.34).clamp(0.0, 1.0);
    let mountain_mask = smoothstep(mountain_mask);

    let mountain_raw = fbm(warped_dir * 18.0, 6);
    let mountains = (1.0 - (mountain_raw * 2.0 - 1.0).abs()).powf(1.6);
    let mountain_height = mountains * mountain_mask * planet_r * 0.032;

    let detail = (fbm(warped_dir * 72.0, 3) - 0.5) * planet_r * 0.004;

    continent_height + mountain_height + detail * mountain_mask
}

fn smooth_noise_f64(p: DVec3) -> f64 {
    let ix = p.x.floor() as i32;
    let iy = p.y.floor() as i32;
    let iz = p.z.floor() as i32;

    let fx = p.x - f64::from(ix);
    let fy = p.y - f64::from(iy);
    let fz = p.z - f64::from(iz);

    let smooth = |value: f64| value * value * (3.0 - 2.0 * value);
    let ux = smooth(fx);
    let uy = smooth(fy);
    let uz = smooth(fz);
    let sample = |x, y, z| f64::from(hash3i(x, y, z));
    let interpolate = |a: f64, b: f64, t: f64| a + t * (b - a);

    let x00 = interpolate(sample(ix, iy, iz), sample(ix + 1, iy, iz), ux);
    let x10 = interpolate(sample(ix, iy + 1, iz), sample(ix + 1, iy + 1, iz), ux);
    let x01 = interpolate(sample(ix, iy, iz + 1), sample(ix + 1, iy, iz + 1), ux);
    let x11 = interpolate(
        sample(ix, iy + 1, iz + 1),
        sample(ix + 1, iy + 1, iz + 1),
        ux,
    );

    interpolate(interpolate(x00, x10, uy), interpolate(x01, x11, uy), uz)
}

fn fbm_f64(p: DVec3, octaves: u32) -> f64 {
    let mut value = 0.0;
    let mut amplitude = 0.5;
    let mut frequency = 1.0;
    for _ in 0..octaves {
        value += amplitude * smooth_noise_f64(p * frequency);
        frequency *= 2.0;
        amplitude *= 0.5;
    }
    value
}

fn spherical_terrain_height_f64(dir: DVec3, planet_radius: f64) -> f64 {
    let warp = engine::math::dvec3(
        fbm_f64(dir * 3.0 + engine::math::dvec3(17.1, 3.7, 11.5), 4),
        fbm_f64(dir * 3.0 + engine::math::dvec3(5.3, 19.1, 2.8), 4),
        fbm_f64(dir * 3.0 + engine::math::dvec3(13.8, 7.4, 23.6), 4),
    ) * 2.0
        - DVec3::ONE;
    let warped_dir = (dir + warp * 0.18).normalize();

    let continent = fbm_f64(warped_dir * 2.2, 5);
    let continent_height = (continent - 0.45) * planet_radius * 0.018;
    let mountain_mask = ((continent - 0.38) / 0.34).clamp(0.0, 1.0);
    let mountain_mask = mountain_mask * mountain_mask * (3.0 - 2.0 * mountain_mask);
    let mountain_raw = fbm_f64(warped_dir * 18.0, 6);
    let mountains = (1.0 - (mountain_raw * 2.0 - 1.0).abs()).powf(1.6);
    let mountain_height = mountains * mountain_mask * planet_radius * 0.032;
    let detail = (fbm_f64(warped_dir * 72.0, 3) - 0.5) * planet_radius * 0.004;

    continent_height + mountain_height + detail * mountain_mask
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use game_types::{
        octree::DensityRange,
        planet::{PlanetTerrainEdits, TerrainBrickKey},
    };

    use super::*;

    fn linear_x_brick(brick_x: i32) -> Vec<Vec<Vec<f32>>> {
        let spacing = TERRAIN_EDIT_BRICK_SIZE / TERRAIN_EDIT_CELL_COUNT as f32;
        (0..TERRAIN_EDIT_SAMPLE_COUNT)
            .map(|x| {
                let value = brick_x as f32 * TERRAIN_EDIT_BRICK_SIZE + x as f32 * spacing;
                vec![vec![value; TERRAIN_EDIT_SAMPLE_COUNT]; TERRAIN_EDIT_SAMPLE_COUNT]
            })
            .collect()
    }

    #[test]
    fn adjacent_edit_bricks_share_endpoint_samples() {
        let key0 = TerrainBrickKey {
            x: 0,
            y: 0,
            z: 0,
            level: 0,
        };
        let key1 = TerrainBrickKey {
            x: 1,
            y: 0,
            z: 0,
            level: 0,
        };
        let edits = PlanetTerrainEdits {
            modified_chunks: HashMap::from([
                (key0, Arc::new(linear_x_brick(0))),
                (key1, Arc::new(linear_x_brick(1))),
            ]),
            modified_ranges: HashMap::from([
                (key0, DensityRange::new(0.0, 32.0)),
                (key1, DensityRange::new(32.0, 64.0)),
            ]),
        };

        for x in [31.5, 32.0, 32.5] {
            let sampled = sample_terrain_edit(vec3(x, 1.0, 1.0), &edits);
            assert!((sampled - x).abs() < 1e-5);
        }
    }
}
