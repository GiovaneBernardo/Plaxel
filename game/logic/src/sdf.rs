use cgmath::{InnerSpace, Vector3, vec3};
use game_types::planet::{PlanetTerrainEdits, TerrainBrickKey};

const EARTH_MEAN_RADIUS_METERS: f32 = 6_371_000.0;
const EARTH_HIGHEST_ALTITUDE_METERS: f32 = 8_848.86;
const EARTH_HEIGHT_EXAGGERATION: f32 = 16.0; //32.0;
const EARTH_BROAD_DETAIL_SCALE: f32 = 0.18;
const EARTH_RIDGE_DETAIL_SCALE: f32 = 0.12;
const EARTH_FINE_DETAIL_SCALE: f32 = 0.035;
const TERRAIN_EDIT_BRICK_SIZE: f32 = 32.0;
const TERRAIN_EDIT_LEVEL: u32 = 0;

pub struct EarthHeightmap {
    pub width: u32,
    pub height: u32,
    pub samples: Vec<f32>,
    pub min_height: f32,
    pub max_height: f32,
}

impl EarthHeightmap {
    pub fn sample_unit_height(&self, dir: Vector3<f32>) -> Option<f32> {
        if self.width == 0 || self.height == 0 {
            return None;
        }

        let expected_len = self.width as usize * self.height as usize;
        if self.samples.len() < expected_len {
            return None;
        }

        let dir = if dir.magnitude2() > 1e-12 {
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

    pub fn sample_height(&self, dir: Vector3<f32>, planet_r: f32) -> Option<f32> {
        let dir = if dir.magnitude2() > 1e-12 {
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
pub fn hash3(p: Vector3<f32>) -> f32 {
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
pub fn smooth_noise(p: Vector3<f32>) -> f32 {
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

pub fn fbm(p: Vector3<f32>, octaves: u32) -> f32 {
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

pub fn sample_terrain_edit(local_p: Vector3<f32>, terrain_edits: &PlanetTerrainEdits) -> f32 {
    let key = TerrainBrickKey {
        x: (local_p.x / TERRAIN_EDIT_BRICK_SIZE).floor() as i32,
        y: (local_p.y / TERRAIN_EDIT_BRICK_SIZE).floor() as i32,
        z: (local_p.z / TERRAIN_EDIT_BRICK_SIZE).floor() as i32,
        level: TERRAIN_EDIT_LEVEL,
    };

    let Some(brick) = terrain_edits.modified_chunks.get(&key) else {
        return 0.0;
    };

    let resolution = brick.len();
    if resolution == 0 {
        return 0.0;
    }

    let brick_min = vec3(
        key.x as f32 * TERRAIN_EDIT_BRICK_SIZE,
        key.y as f32 * TERRAIN_EDIT_BRICK_SIZE,
        key.z as f32 * TERRAIN_EDIT_BRICK_SIZE,
    );
    let uvw = (local_p - brick_min) / TERRAIN_EDIT_BRICK_SIZE;
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

pub fn sdf_at_center(
    p: cgmath::Vector3<f32>,
    planet_center: cgmath::Vector3<f32>,
    planet_size: u32,
    heightmap: Option<&EarthHeightmap>,
    terrain_edits: &PlanetTerrainEdits,
) -> f32 {
    let planet_r = planet_radius(planet_size);
    let local_p = p - planet_center;
    let dist_from_center = local_p.magnitude();
    let dir = if dist_from_center > 1e-6 {
        local_p / dist_from_center
    } else {
        vec3(0.0, 1.0, 0.0)
    };

    let height = heightmap
        .and_then(|heightmap| heightmap.sample_height(dir, planet_r))
        .unwrap_or_else(|| spherical_terrain_height(dir, planet_r));
    let terrain = dist_from_center - (planet_r + height);
    return terrain + sample_terrain_edit(local_p, terrain_edits);

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

pub fn planet_radius(planet_size: u32) -> f32 {
    planet_size as f32 / 8.0
}

pub fn min_terrain_height(planet_size: u32) -> f32 {
    -planet_radius(planet_size) * 0.011
}

pub fn max_terrain_height(planet_size: u32) -> f32 {
    planet_radius(planet_size) * 0.045
}

pub fn spherical_terrain_height(dir: Vector3<f32>, planet_r: f32) -> f32 {
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
