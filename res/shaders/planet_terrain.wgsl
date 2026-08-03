struct CameraUniform {
    view_proj: mat4x4<f32>,
    position: vec3<f32>,
};
@group(0) @binding(0)
var<uniform> camera: CameraUniform;

struct GpuTerrainFrame {
    view_projection_rotation: mat4x4<f32>,
    camera_anchor_planet: vec3<i32>,
    position_unit: f32,
    camera_remainder_planet: vec3<f32>,
    _padding: f32,
    planet_world_position: vec3<f32>,
    _planet_padding: f32,
};

struct ShadowUniform {
    view_proj: mat4x4<f32>,
    light_direction: vec3<f32>,
    depth_bias: f32,
};
@group(3) @binding(0)
var<uniform> shadow: ShadowUniform;
@group(3) @binding(1)
var shadow_depth_map: texture_depth_2d;

struct InstanceInput {
    @location(5) model_matrix_0: vec4<f32>,
    @location(6) model_matrix_1: vec4<f32>,
    @location(7) model_matrix_2: vec4<f32>,
    @location(8) model_matrix_3: vec4<f32>,
};

// Packed to match PlanetVertex in game/types/src/planet.rs:
//   mats  = mat_a | (mat_b << 16)
//   blend = low byte holds the 0..255 blend factor
struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) mats: u32,
    @location(3) blend_packed: u32,
    @location(4) chunk_index: u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) world_position: vec3<f32>,
    @location(2) @interpolate(flat) mat_a: u32,
    @location(3) @interpolate(flat) mat_b: u32,
    @location(4) blend: f32,
    @location(5) camera_position: vec3<f32>,
    @location(6) shadow_position: vec4<f32>,
    @location(7) texture_position: vec3<f32>,
};

struct GpuPlanetTerrainMaterial {
    diffuse_texture_index: u32,
    normal_texture_index: u32,
    displacement_texture_index: u32,
    roughness_texture_index: u32,
    texture_scale: f32,
    displacement_scale: f32,
    roughness_factor: f32,
    flags: u32,
}

struct GpuPlanetChunk {
    node_origin_planet: vec3<i32>,
    level: i32,
};

@vertex
fn vs_main(
    model: VertexInput,
) -> VertexOutput {
    var out: VertexOutput;
    let chunk = terrain_chunks[model.chunk_index];
    let relative_anchor = chunk.node_origin_planet - terrain_frame.camera_anchor_planet;
    let camera_relative_position = vec3<f32>(relative_anchor) * terrain_frame.position_unit
        + model.position
        - terrain_frame.camera_remainder_planet;
    let texture_anchor = chunk.node_origin_planet % vec3<i32>(4096);
    let camera_relative = vec4<f32>(camera_relative_position, 1.0);

    out.world_position = camera_relative_position;
    out.texture_position = vec3<f32>(texture_anchor) * terrain_frame.position_unit + model.position;
    out.clip_position = terrain_frame.view_projection_rotation * camera_relative;

    out.normal = safe_normal(model.normal, model.position);
    out.mat_a = u32(model.mats & 0xFFFFu);
    out.mat_b = u32(model.mats >> 16u);
    out.blend = f32(model.blend_packed & 0xFFu) / 255.0;
    out.camera_position = vec3<f32>(0.0);
    out.shadow_position = shadow.view_proj * camera_relative;

    return out;
}

@vertex
fn vs_shadow(
    model: VertexInput,
) -> @builtin(position) vec4<f32> {
    let chunk = terrain_chunks[model.chunk_index];
    let relative_anchor = chunk.node_origin_planet - terrain_frame.camera_anchor_planet;
    let camera_relative_position = vec3<f32>(relative_anchor) * terrain_frame.position_unit
        + model.position
        - terrain_frame.camera_remainder_planet;
    return terrain_frame.view_projection_rotation * vec4<f32>(camera_relative_position, 1.0);
}

fn safe_normal(normal: vec3<f32>, fallback_position: vec3<f32>) -> vec3<f32> {
    let normal_len2 = dot(normal, normal);
    if normal_len2 > 0.000001 {
        return normal * inverseSqrt(normal_len2);
    }

    let fallback_len2 = dot(fallback_position, fallback_position);
    if fallback_len2 > 0.000001 {
        return fallback_position * inverseSqrt(fallback_len2);
    }

    return vec3<f32>(0.0, 1.0, 0.0);
}

fn get_material_color(index: u32) -> vec4<f32> {
    if index == 0 {
        return vec4(0.9, 0.2, 0.2, 1.0);
    } else if index == 1 {
        return vec4(0.2, 0.9, 0.2, 1.0);
    }
    return vec4(0.2, 0.2, 0.9, 1.0);
}

@group(1) @binding(0)
var my_textures: binding_array<texture_2d<f32>, 512>;
@group(1) @binding(1)
var default_sampler: sampler;
@group(2) @binding(0)
var<storage, read> terrain_materials: array<GpuPlanetTerrainMaterial>;

@group(2) @binding(1)
var<uniform> terrain_frame: GpuTerrainFrame;

@group(2) @binding(2)
var<storage, read> terrain_chunks: array<GpuPlanetChunk>;

fn triplanar_sample(tex_index: u32, pos: vec3<f32>, normal: vec3<f32>, texture_scale: f32) -> vec4<f32> {
    let n = safe_normal(normal, pos);
    let an = abs(n);

    let weights = an / max(an.x + an.y + an.z, 0.0001);

    let p = pos * texture_scale;

    var x_uv = p.zy;
    var y_uv = p.xz;
    var z_uv = p.xy;

    if n.x < 0.0 {
        x_uv.x = -x_uv.x;
    }
    if n.y < 0.0 {
        y_uv.x = -y_uv.x;
    }
    if n.z < 0.0 {
        z_uv.x = -z_uv.x;
    }

    let x_sample = textureSample(my_textures[tex_index], default_sampler, x_uv);
    let y_sample = textureSample(my_textures[tex_index], default_sampler, y_uv);
    let z_sample = textureSample(my_textures[tex_index], default_sampler, z_uv);

    return x_sample * weights.x +
           y_sample * weights.y +
           z_sample * weights.z;
}

fn sample_terrain_albedo(material_index: u32, pos: vec3<f32>, normal: vec3<f32>) -> vec4<f32> {
    let material = terrain_materials[material_index];
    return triplanar_sample(
        material.diffuse_texture_index,
        pos,
        normal,
        0.1,
    );
}

fn sample_terrain_normal(
    material_index: u32,
    pos: vec3<f32>,
    normal: vec3<f32>,
) -> vec3<f32> {
    let material = terrain_materials[material_index];

    return triplanar_sample_normal(
        material.normal_texture_index,
        pos,
        normal,
        0.1,
    );
}

fn shadow_visibility(shadow_position: vec4<f32>) -> f32 {
    let ndc = shadow_position.xyz / shadow_position.w;
    if abs(ndc.x) > 1.0 || abs(ndc.y) > 1.0 || ndc.z < 0.0 || ndc.z > 1.0 {
        return 1.0;
    }

    // WGPU's framebuffer Y direction is opposite NDC Y when addressed as a texture.
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let dimensions = textureDimensions(shadow_depth_map);
    let center = vec2<i32>(uv * vec2<f32>(dimensions));
    let maximum = vec2<i32>(dimensions) - vec2<i32>(1);
    var visibility = 0.0;

    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let pixel = clamp(center + vec2<i32>(x, y), vec2<i32>(0), maximum);
            let stored_depth = textureLoad(shadow_depth_map, pixel, 0);
            // Reverse-Z: a receiver is visible when it is at least as close as the stored caster.
            visibility += select(0.0, 1.0, ndc.z + shadow.depth_bias >= stored_depth);
        }
    }

    return visibility / 9.0;
}

fn decode_normal_map(sample_value: vec4<f32>) -> vec3<f32> {
    var tangent_normal = sample_value.xyz * 2.0 - vec3<f32>(1.0);

    // Enable this if the asset uses the opposite green-channel convention.
    // tangent_normal.y = -tangent_normal.y;

    return safe_normal(tangent_normal, vec3<f32>(0.0, 0.0, 1.0));
}

fn triplanar_sample_normal(
    tex_index: u32,
    pos: vec3<f32>,
    geometric_normal: vec3<f32>,
    texture_scale: f32,
) -> vec3<f32> {
    let n = safe_normal(geometric_normal, pos);
    let an = abs(n);

    let weight_sum = max(an.x + an.y + an.z, 0.0001);
    let weights = an / weight_sum;

    let p = pos * texture_scale;

    let x_sign = select(-1.0, 1.0, n.x >= 0.0);
    let y_sign = select(-1.0, 1.0, n.y >= 0.0);
    let z_sign = select(-1.0, 1.0, n.z >= 0.0);

    // These reproduce your existing UV orientations.
    let x_uv = vec2<f32>(p.z * x_sign, p.y);
    let y_uv = vec2<f32>(p.x * y_sign, p.z);
    let z_uv = vec2<f32>(p.x * z_sign, p.y);

    let x_tangent = decode_normal_map(
        textureSample(my_textures[tex_index], default_sampler, x_uv)
    );

    let y_tangent = decode_normal_map(
        textureSample(my_textures[tex_index], default_sampler, y_uv)
    );

    let z_tangent = decode_normal_map(
        textureSample(my_textures[tex_index], default_sampler, z_uv)
    );

    // Convert each tangent-space normal into the same space as
    // geometric_normal.
    //
    // X projection:
    // U = signed Z, V = Y, outward = signed X
    let x_normal = vec3<f32>(
        x_sign * x_tangent.z,
        x_tangent.y,
        x_sign * x_tangent.x,
    );

    // Y projection:
    // U = signed X, V = Z, outward = signed Y
    let y_normal = vec3<f32>(
        y_sign * y_tangent.x,
        y_sign * y_tangent.z,
        y_tangent.y,
    );

    // Z projection:
    // U = signed X, V = Y, outward = signed Z
    let z_normal = vec3<f32>(
        z_sign * z_tangent.x,
        z_tangent.y,
        z_sign * z_tangent.z,
    );

    let blended = x_normal * weights.x +
        y_normal * weights.y +
        z_normal * weights.z;

    return safe_normal(blended, n);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let geometric_normal = safe_normal(in.normal, in.world_position);

    let material_a = sample_terrain_albedo(in.mat_a, in.texture_position, geometric_normal);
    let material_b = sample_terrain_albedo(in.mat_b, in.texture_position, geometric_normal);
    let albedo = mix(material_a, material_b, in.blend);

    let normal_a = sample_terrain_normal(in.mat_a, in.texture_position, geometric_normal);
    let normal_b = sample_terrain_normal(in.mat_b, in.texture_position, geometric_normal);
    let mapped_normal = safe_normal(mix(normal_a, normal_b, in.blend), geometric_normal);

    let light_dir = normalize(shadow.light_direction);
    let diffuse = max(dot(mapped_normal, light_dir), 0.0);
    let visibility = 1.0;//shadow_visibility(in.shadow_position);
    let lighting = 0.35 + 0.95 * diffuse * visibility;

    let distance = length(in.world_position - in.camera_position);
    let start = 5000.0;
    let end = 15000.0;
    var fog_factor = clamp((distance - start) / (end - start), 0.0, 1.0);

    return vec4<f32>(mix(albedo.rgb * lighting, vec3f(0.1, 0.2, 0.3), 0.0), albedo.a);
}
