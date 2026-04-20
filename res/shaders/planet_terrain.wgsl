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
};

@vertex
fn vs_main(
    model: VertexInput,
    instance: InstanceInput,
) -> VertexOutput {
    let model_matrix = mat4x4<f32>(
        instance.model_matrix_0,
        instance.model_matrix_1,
        instance.model_matrix_2,
        instance.model_matrix_3,
    );

    var out: VertexOutput;
    out.normal = model.normal;
    out.world_position = model.position;
    out.clip_position = camera.view_proj * vec4<f32>(model.position, 1.0);
    out.mat_a = u32(model.mats & 0xFFFFu);
    out.mat_b = u32(model.mats >> 16u);
    out.blend = f32(model.blend_packed & 0xFFu) / 255.0;
    return out;
}

fn get_material_color(index: u32) -> vec4<f32> {
    if index == 0 {
        return vec4(0.9, 0.2, 0.2, 1.0);
    } else if index == 1 {
        return vec4(0.2, 0.9, 0.2, 1.0);
    }
    return vec4(0.2, 0.2, 0.9, 1.0);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let distance = length(camera.position - in.clip_position.xyz);
    let attenuation = 1.0 / (1.0 + 0.09 * distance + 
    		    0.032 * (distance * distance));
    let tex_color = vec4(1.0, 1.0, 1.0, 1.0) * dot(in.normal, vec3(0.3, 0.5, 0.0)) + (attenuation);//textureSample(t_diffuse, s_diffuse, in.uv);
    let color_a = get_material_color(in.mat_a);
    let color_b = get_material_color(in.mat_b);
    let color_final = mix(color_a, color_b, in.blend) * dot(in.normal, vec3(0.3, 0.5, 0.0)) + (attenuation);
    return vec4<f32>(color_final);
}