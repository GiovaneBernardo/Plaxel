struct CameraUniform {
    view_proj: mat4x4<f32>,
    position: vec3<f32>,
};
@group(0) @binding(0)
var<uniform> camera: CameraUniform;

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
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) world_position: vec3<f32>,
    @location(2) @interpolate(flat) mat_a: u32,
    @location(3) @interpolate(flat) mat_b: u32,
    @location(4) blend: f32,
    @location(5) camera_position: vec3<f32>,
};

@vertex
fn vs_main(
    model: VertexInput,
) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = vec4<f32>(model.position, 1.0);

    out.world_position = world_pos.xyz;
    out.clip_position = camera.view_proj * world_pos;

    out.normal = safe_normal(model.normal, model.position);
    out.mat_a = u32(model.mats & 0xFFFFu);
    out.mat_b = u32(model.mats >> 16u);
    out.blend = f32(model.blend_packed & 0xFFu) / 255.0;
    out.camera_position = camera.position;

    return out;
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

fn triplanar_sample(tex_index: u32, pos: vec3<f32>, normal: vec3<f32>) -> vec4<f32> {
    let texture_scale = 1.0;//0.0025;

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

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let normal = safe_normal(in.normal, in.world_position);

    let material_a = triplanar_sample(in.mat_a, in.world_position, normal);
    let material_b = triplanar_sample(in.mat_b, in.world_position, normal);
    let albedo = mix(material_a, material_b, in.blend);

    let light_dir = normalize(vec3<f32>(0.3, 0.6, 0.4));
    let diffuse = max(dot(normal, light_dir), 0.0);
    let lighting = 0.35 + 0.65 * diffuse;

    let distance = length(in.world_position - in.camera_position);
    let start = 5000.0;
    let end = 15000.0;
    var fog_factor = clamp((distance - start) / (end - start), 0.0, 1.0);

    return vec4<f32>(mix(albedo.rgb * lighting, vec3f(0.1, 0.2, 0.3), fog_factor), albedo.a);
}
