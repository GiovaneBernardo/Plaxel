struct CloudsUniform {
    camera_position: vec4<f32>,
    sun_direction: vec4<f32>,
    planet_center: vec4<f32>,
    params: vec4<f32>, // planet radius, base altitude, top altitude, noise tile size
    screen_size: vec2<f32>,
    steps: i32,
    light_steps: i32,
    inverse_projection: mat4x4<f32>,
    inverse_view: mat4x4<f32>,
};
@group(0) @binding(0) var<uniform> clouds: CloudsUniform;
@group(0) @binding(1) var scene_depth: texture_depth_2d;
@group(0) @binding(2) var scene_color: texture_2d<f32>;
@group(0) @binding(3) var scene_sampler: sampler;
@group(0) @binding(4) var clouds_texture1: texture_3d<f32>;

@vertex
fn vs_main(@builtin(vertex_index)index: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0), vec2<f32>(3.0, 1.0), vec2<f32>(-1.0, 1.0)
    );
    return vec4<f32>(positions[index], 0.0, 1.0);
}

// Ray distances in planet-relative space. A reversed interval denotes a miss.
fn sphere_hit(origin: vec3<f32>, direction: vec3<f32>, radius: f32) -> vec2<f32> {
    let b = dot(origin, direction);
    let c = dot(origin, origin) - radius * radius;
    let discriminant = b * b - c;
    if discriminant < 0.0 { return vec2<f32>(1e30, -1e30); }
    let root = sqrt(discriminant);
    return vec2<f32>(-b - root, -b + root);
}

fn density(position: vec3<f32>) -> f32 {
    let altitude = length(position) - clouds.params.x;
    let height = (altitude - clouds.params.y) / (clouds.params.z - clouds.params.y);
    if height <= 0.0 || height >= 1.0 { return 0.0; }
    let uvw = position / clouds.params.w;
    let shape = textureSampleLevel(clouds_texture1, scene_sampler, uvw, 0.0).r;
    let detail = textureSampleLevel(clouds_texture1, scene_sampler, uvw * 3.1 + vec3<f32>(0.37), 0.0).r;
    let profile = smoothstep(0.0, 0.15, height) * (1.0 - smoothstep(0.65, 1.0, height));
    // Preview controls: lower the threshold for more coverage.
    return smoothstep(0.46, 0.68, shape * 0.8 + detail * 0.2) * profile;
}

@fragment
fn fs_main(@builtin(position)pixel: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = pixel.xy / clouds.screen_size;
    let background = textureSampleLevel(scene_color, scene_sampler, uv, 0.0).rgb;
    let ndc = uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0);
    let view = clouds.inverse_projection * vec4<f32>(ndc, 1.0, 1.0);
    let direction = normalize((clouds.inverse_view * vec4<f32>(view.xyz / view.w, 0.0)).xyz);
    let origin = clouds.camera_position.xyz - clouds.planet_center.xyz;
    let inner_radius = clouds.params.x + clouds.params.y;
    let outer_radius = clouds.params.x + clouds.params.z;
    let outer = sphere_hit(origin, direction, outer_radius);
    var start = max(outer.x, 0.0);
    var end = outer.y;
    let inner = sphere_hit(origin, direction, inner_radius);
    if length(origin) < inner_radius {
        start = max(start, inner.y);
    } else if inner.x > 0.0 && inner.x < end {
        end = inner.x;
    }
    // Clip to the planet even where terrain geometry has not loaded yet.
    let ground = sphere_hit(origin, direction, clouds.params.x);
    if ground.x > 0.0 { end = min(end, ground.x); }
    let depth = textureLoad(scene_depth, vec2<i32>(pixel.xy), 0);
    // Reverse Z: zero is the clear/sky depth.
    if depth > 0.0 {
        let view_hit = clouds.inverse_projection * vec4<f32>(ndc, depth, 1.0);
        let world_hit = clouds.inverse_view * vec4<f32>(view_hit.xyz / view_hit.w, 1.0);
        end = min(end, length(world_hit.xyz / world_hit.w - clouds.camera_position.xyz));
    }
    if end <= start { return vec4<f32>(background, 1.0); }

    let thickness = clouds.params.z - clouds.params.y;
    let extinction = 5.0 / thickness;
    let step_size = (end - start) / f32(clouds.steps);
    let sun = normalize(clouds.sun_direction.xyz);
    let light_step = thickness / f32(clouds.light_steps);
    var transmittance = 1.0;
    var scattered = vec3<f32>(0.0);
    for (var i = 0; i < clouds.steps; i += 1) {
        let position = origin + direction * (start + (f32(i) + 0.5) * step_size);
        let d = density(position);
        if d > 0.001 {
            var light_depth = 0.0;
            for (var j = 0; j < clouds.light_steps; j += 1) {
                light_depth += density(position + sun * (f32(j) + 0.5) * light_step) * light_step;
            }
            let daylight = smoothstep(-0.08, 0.15, dot(normalize(position), sun));
            let silver = 0.35 * pow(max(dot(direction, sun), 0.0), 8.0);
            let ambient = vec3<f32>(0.18, 0.23, 0.32) * (0.08 + 0.92 * daylight);
            let lighting = ambient + vec3<f32>(1.0, 0.95, 0.86) * daylight * exp(-light_depth * extinction) * (0.85 + silver);
            let alpha = 1.0 - exp(-d * extinction * step_size);
            scattered += transmittance * alpha * lighting;
            transmittance *= 1.0 - alpha;
            if transmittance < 0.01 { break; }
        }
    }
    return vec4<f32>(scattered + background * transmittance, 1.0);
}
