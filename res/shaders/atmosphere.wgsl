struct AtmosphereUniform {
    camera_position: vec4<f32>,
    sun_direction: vec4<f32>,
    planet_center: vec4<f32>,
    params: vec4<f32>,
    screen_size: vec2<f32>,
    inverse_projection: mat4x4<f32>,
    inverse_view: mat4x4<f32>,
};

@group(0) @binding(0)
var<uniform> atmosphere: AtmosphereUniform;
@group(0) @binding(1)
var scene_depth: texture_depth_2d;
@group(0) @binding(2)
var scene_color: texture_2d<f32>;
@group(0) @binding(3)
var scene_sampler: sampler;

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
    let ndc = vec2<f32>(
        uv.x * 2.0 - 1.0,
        1.0 - uv.y * 2.0
    );

    let clip = vec4<f32>(ndc, depth, 1.0);

    let view = atmosphere.inverse_projection * clip;
    let view_pos = view.xyz / view.w;

    let world = atmosphere.inverse_view * vec4<f32>(view_pos, 1.0);
    return world.xyz / world.w;
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
    let ray_direction = normalize(
        (atmosphere.inverse_view * vec4<f32>(view_dir, 0.0)).xyz
    );

    let raw_depth = textureLoad(scene_depth, depth_texel, 0);
    var scene_color_sample = textureSample(scene_color, scene_sampler, uv);

    let scene_world_pos = get_world_pos_from_depth(uv, raw_depth);
    let scene_depth_value = length(scene_world_pos - ray_origin);

    let hit_info = ray_sphere(
        atmosphere.planet_center.xyz,
        atmosphere_radius,
        ray_origin,
        ray_direction
    );

    let dst_to_atmosphere = hit_info.x;

    let dst_through_atmosphere = max(
        0.0,
        min(hit_info.y, scene_depth_value - dst_to_atmosphere)
    );

    if dst_through_atmosphere > 0.0 {
        let epsilon = 0.0001;
        let point_in_atmosphere = ray_origin + ray_direction * (dst_to_atmosphere + epsilon);
        let light = calculate_light(atmosphere.planet_center.xyz, planet_radius, atmosphere_radius, point_in_atmosphere, ray_direction, dst_through_atmosphere - epsilon * 2, scene_color_sample.rgb);
        return vec4<f32>(light, 1.0);
    }

    return scene_color_sample;
}

fn ray_sphere(sphere_center: vec3<f32>, sphere_radius: f32, ray_origin: vec3<f32>, ray_direction: vec3<f32>) -> vec2<f32> {
    let offset = ray_origin - sphere_center;
    let a = 1.0;
    let b = 2 * dot(offset, ray_direction);
    let c = dot(offset, offset) - sphere_radius * sphere_radius;
    let d = b * b - 4.0 * a * c;

    if d > 0 {
        let s = sqrt(d);
        let distance_sphere_near = max(0, (-b - s) / (2.0 * a));
        let distance_sphere_far = (-b + s) / (2.0 * a);

        if distance_sphere_far >= 0 {
            return vec2<f32>(distance_sphere_near, distance_sphere_far - distance_sphere_near);
        }
    }

    return vec2<f32>(3200000.0, 0.0);
}

fn calculate_light(planet_center: vec3<f32>, planet_radius: f32, atmosphere_radius: f32, ray_origin: vec3<f32>, ray_direction: vec3<f32>, ray_length: f32, original_color: vec3<f32>) -> vec3<f32> {
    let numInScatteringPoints = 10;
    var in_scatter_point: vec3<f32> = ray_origin;
    let step_size: f32 = ray_length / (f32(numInScatteringPoints) - 1.0);
    let atmosphere_height = atmosphere_radius - planet_radius;
    let normalized_step_size = step_size / atmosphere_height;
    var in_scattered_light = vec3<f32>(0.0, 0.0, 0.0);
    var view_ray_optical_depth = 0.0;

    let wavelengths = vec3<f32>(700.0, 530.0, 460.0);
    let scatteringStrength = 7.0;
    let scatterR = pow(400.0 / wavelengths.x, 4.0) * scatteringStrength;
    let scatterG = pow(400.0 / wavelengths.y, 4.0) * scatteringStrength;
    let scatterB = pow(400.0 / wavelengths.z, 4.0) * scatteringStrength;
    let scatteringCoefficients = vec3<f32>(scatterR, scatterG, scatterB);

    for (var i = 0; i < numInScatteringPoints; i++) {
        let sun_ray_length = ray_sphere(planet_center, atmosphere_radius, in_scatter_point, normalize(atmosphere.sun_direction.xyz)).y;
        let sun_ray_optical_depth = optical_depth(in_scatter_point, normalize(atmosphere.sun_direction.xyz), sun_ray_length, planet_center, planet_radius, atmosphere_radius);
        view_ray_optical_depth = optical_depth(in_scatter_point, -ray_direction, step_size * f32(i), planet_center, planet_radius, atmosphere_radius);
        let transmittance: vec3<f32> = exp(-(sun_ray_optical_depth + view_ray_optical_depth) * scatteringCoefficients);
        let local_density = density_at_point(in_scatter_point, planet_center, planet_radius, atmosphere_radius);

        in_scattered_light += local_density * transmittance * scatteringCoefficients * normalized_step_size;
        in_scatter_point += ray_direction * step_size;
    }

    let original_color_transmittance = exp(-view_ray_optical_depth * scatteringCoefficients);
    return original_color * original_color_transmittance + in_scattered_light;
}

fn density_at_point(
    density_sample_point: vec3<f32>,
    planet_center: vec3<f32>,
    planet_radius: f32,
    atmosphere_radius: f32
) -> f32 {
    let height_above_surface = length(density_sample_point - planet_center) - planet_radius;
    let atmosphere_height = atmosphere_radius - planet_radius;

    let height01 = clamp(height_above_surface / atmosphere_height, 0.0, 1.0);
    let density_falloff = 4.0;

    let local_density = exp(-height01 * density_falloff) * (1.0 - height01);
    return local_density;
}

fn optical_depth(ray_origin: vec3<f32>, ray_direction: vec3<f32>, ray_length: f32, planet_center: vec3<f32>, planet_radius: f32, atmosphere_radius: f32) -> f32 {
    let num_optical_depth_points = 10;

    var density_sample_point = ray_origin;
    let step_size = ray_length / (f32(num_optical_depth_points) - 1);
    let atmosphere_height = atmosphere_radius - planet_radius;
    let normalized_step_size = step_size / atmosphere_height;
    var optical_depth = 0.0;

    for (var i = 0; i < num_optical_depth_points; i++) {
        let local_density = density_at_point(density_sample_point, planet_center, planet_radius, atmosphere_radius);
        optical_depth += local_density * normalized_step_size;
        density_sample_point += ray_direction * step_size;
    }

    return optical_depth;
}
