struct AtmosphereUniform {
    camera_position: vec4<f32>,
    sun_direction: vec4<f32>,
    planet_center: vec4<f32>,
    params: vec4<f32>, // x: planet radius, y: atmosphere radius, z: Mie g, w: sun intensity
    screen_size: vec2<f32>,
    num_in_scattering_points: i32,
    num_optical_depth_points: i32,
    rayleigh_scattering: vec3<f32>,
    rayleigh_scale_height: f32,
    mie_scattering: vec3<f32>,
    mie_scale_height: f32,
    inverse_projection: mat4x4<f32>,
    inverse_view: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> atmosphere: AtmosphereUniform;
@group(0) @binding(1) var scene_depth: texture_depth_2d;
@group(0) @binding(2) var scene_color: texture_2d<f32>;
@group(0) @binding(3) var scene_sampler: sampler;
@group(0) @binding(4) var skybox_texture: texture_2d<f32>;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(3.0, 1.0),
        vec2<f32>(-1.0, 1.0),
    );
    let pos = positions[vertex_index];
    var out: VertexOutput;
    out.clip_position = vec4<f32>(pos, 0.0, 1.0);
    out.ndc = pos;
    return out;
}

fn get_world_pos_from_depth(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let clip = vec4<f32>(ndc, depth, 1.0);
    let view = atmosphere.inverse_projection * clip;
    let view_pos = view.xyz / view.w;
    let world = atmosphere.inverse_view * vec4<f32>(view_pos, 1.0);
    return world.xyz / world.w;
}

fn tone_map_aces(color: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((color * (a * color + b)) / (color * (c * color + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Rayleigh Phase Function: Light scatters equally forward and backward
fn phase_rayleigh(cos_theta: f32) -> f32 {
    let pi = 3.14159265359;
    return (3.0 / (16.0 * pi)) * (1.0 + cos_theta * cos_theta);
}

// Henyey-Greenstein Mie Phase Function: Creates direct forward glare around the sun
fn phase_mie(cos_theta: f32, g: f32) -> f32 {
    let pi = 3.14159265359;
    let g2 = g * g;
    let denom = 1.0 + g2 - 2.0 * g * cos_theta;
    return (1.0 / (4.0 * pi)) * ((1.0 - g2) / (denom * sqrt(denom)));
}

fn ray_sphere(sphere_center: vec3<f32>, sphere_radius: f32, ray_origin: vec3<f32>, ray_direction: vec3<f32>) -> vec2<f32> {
    let offset = ray_origin - sphere_center;
    let a = 1.0;
    let b = 2.0 * dot(offset, ray_direction);
    let c = dot(offset, offset) - sphere_radius * sphere_radius;
    let d = b * b - 4.0 * a * c;

    if d > 0.0 {
        let s = sqrt(d);
        let distance_sphere_near = max(0.0, (-b - s) / (2.0 * a));
        let distance_sphere_far = (-b + s) / (2.0 * a);

        if distance_sphere_far >= 0.0 {
            return vec2<f32>(distance_sphere_near, distance_sphere_far - distance_sphere_near);
        }
    }
    return vec2<f32>(3200000.0, 0.0);
}

// Returns vec2(Rayleigh Density, Mie Density) exponentially scaled by height
fn densities_at_point(sample_point: vec3<f32>, planet_center: vec3<f32>, planet_radius: f32) -> vec2<f32> {
    let height = length(sample_point - planet_center) - planet_radius;
    let density_r = exp(-max(0.0, height) / atmosphere.rayleigh_scale_height);
    let density_m = exp(-max(0.0, height) / atmosphere.mie_scale_height);
    return vec2<f32>(density_r, density_m);
}

// Computes optical depth along a ray for Rayleigh and Mie
fn optical_depths(ray_origin: vec3<f32>, ray_direction: vec3<f32>, ray_length: f32, planet_center: vec3<f32>, planet_radius: f32) -> vec2<f32> {
    var sample_pt = ray_origin;
    let step_size = ray_length / f32(max(1, atmosphere.num_optical_depth_points));
    var accum = vec2<f32>(0.0);

    for (var i = 0; i < atmosphere.num_optical_depth_points; i++) {
        accum += densities_at_point(sample_pt, planet_center, planet_radius) * step_size;
        sample_pt += ray_direction * step_size;
    }
    return accum;
}

fn calculate_light(
    planet_center: vec3<f32>,
    planet_radius: f32,
    atmosphere_radius: f32,
    ray_origin: vec3<f32>,
    ray_direction: vec3<f32>,
    ray_length: f32,
    original_color: vec3<f32>
) -> vec3<f32> {
    let sun_dir = normalize(atmosphere.sun_direction.xyz);
    let step_size = ray_length / f32(max(1, atmosphere.num_in_scattering_points));
    var sample_point = ray_origin + ray_direction * (step_size * 0.5);

    let beta_rayleigh = atmosphere.rayleigh_scattering;
    let beta_mie = atmosphere.mie_scattering;

    var accum_rayleigh = vec3<f32>(0.0);
    var accum_mie = vec3<f32>(0.0);
    var view_optical_depth = vec2<f32>(0.0);

    for (var i = 0; i < atmosphere.num_in_scattering_points; i++) {
        let densities = densities_at_point(sample_point, planet_center, planet_radius);
        view_optical_depth += densities * step_size;

        let sun_hit = ray_sphere(planet_center, atmosphere_radius, sample_point, sun_dir);
        let sun_optical_depth = optical_depths(sample_point, sun_dir, sun_hit.y, planet_center, planet_radius);

        let total_optical_depth = view_optical_depth + sun_optical_depth;
        let extinction = exp(-(beta_rayleigh * total_optical_depth.x + beta_mie * total_optical_depth.y));

        accum_rayleigh += densities.x * extinction * step_size;
        accum_mie += densities.y * extinction * step_size;

        sample_point += ray_direction * step_size;
    }

    let cos_theta = dot(ray_direction, sun_dir);
    let p_r = phase_rayleigh(cos_theta);
    let p_m = phase_mie(cos_theta, atmosphere.params.z);

    let sun_intensity = atmosphere.params.w;
    let in_scattered = sun_intensity * (accum_rayleigh * beta_rayleigh * p_r + accum_mie * beta_mie * p_m);

    // Attenuate background scene color by atmospheric extinction
    let scene_transmittance = exp(-(beta_rayleigh * view_optical_depth.x + beta_mie * view_optical_depth.y));
    let final_color = original_color * scene_transmittance + in_scattered;

    return tone_map_aces(final_color);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let planet_radius = atmosphere.params.x;
    let atmosphere_radius = atmosphere.params.y;

    let uv = in.ndc * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5, 0.5);
    let depth_texel = vec2<i32>(uv * atmosphere.screen_size);
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);

    let clip = vec4<f32>(ndc, 1.0, 1.0);
    let view_pos = atmosphere.inverse_projection * clip;
    let view_dir = normalize(view_pos.xyz / view_pos.w);

    let ray_origin = atmosphere.camera_position.xyz;
    let ray_direction = normalize((atmosphere.inverse_view * vec4<f32>(view_dir, 0.0)).xyz);

    let raw_depth = textureLoad(scene_depth, depth_texel, 0);
    var scene_color_sample = textureSample(scene_color, scene_sampler, uv);

    let scene_world_pos = get_world_pos_from_depth(uv, raw_depth);
    var scene_depth_value = length(scene_world_pos - ray_origin);

    if raw_depth <= 0.0 {
        scene_depth_value = 1e9; // Outer space depth limit
    }

    let hit_info = ray_sphere(atmosphere.planet_center.xyz, atmosphere_radius, ray_origin, ray_direction);
    let dst_to_atmosphere = hit_info.x;
    let dst_through_atmosphere = max(0.0, min(hit_info.y, scene_depth_value - dst_to_atmosphere));

    if dst_through_atmosphere > 0.0 {
        let point_in_atmosphere = ray_origin + ray_direction * dst_to_atmosphere;
        let light = calculate_light(
            atmosphere.planet_center.xyz,
            planet_radius,
            atmosphere_radius,
            point_in_atmosphere,
            ray_direction,
            dst_through_atmosphere,
            scene_color_sample.rgb
        );
        return vec4<f32>(light, 1.0);
    }

    return scene_color_sample;
}
