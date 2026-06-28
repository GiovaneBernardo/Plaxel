use cgmath::{InnerSpace, Vector3, vec3};

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

pub fn sdf(p: cgmath::Vector3<f32>, planet_size: u32) -> f32 {
    sdf_at_center(p, vec3(0.0, 0.0, 0.0), planet_size)
}

pub fn sdf_at_center(
    p: cgmath::Vector3<f32>,
    planet_center: cgmath::Vector3<f32>,
    planet_size: u32,
) -> f32 {
    let planet_r = planet_radius(planet_size);
    let local_p = p - planet_center;
    let dist_from_center = local_p.magnitude();
    let dir = if dist_from_center > 1e-6 {
        local_p / dist_from_center
    } else {
        vec3(0.0, 1.0, 0.0)
    };

    let height = spherical_terrain_height(dir, planet_r);
    let terrain = dist_from_center - (planet_r + height);
    return terrain;

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
