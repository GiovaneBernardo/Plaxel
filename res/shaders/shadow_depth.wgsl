struct ShadowUniform {
    view_proj: mat4x4<f32>,
    light_direction: vec3<f32>,
    depth_bias: f32,
};

@group(0) @binding(0)
var<uniform> shadow: ShadowUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
};

@vertex
fn vs_shadow(vertex: VertexInput) -> @builtin(position) vec4<f32> {
    return shadow.view_proj * vec4<f32>(vertex.position, 1.0);
}
