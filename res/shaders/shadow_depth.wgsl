struct ShadowUniform {
    view_proj: mat4x4<f32>,
    light_direction: vec3<f32>,
    depth_bias: f32,
};

@group(0) @binding(0)
var<uniform> shadow: ShadowUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(4) chunk_index: u32,
};

struct GpuTerrainFrame {
    view_projection_rotation: mat4x4<f32>,
    camera_anchor_planet: vec3<i32>,
    position_unit: f32,
    camera_remainder_planet: vec3<f32>,
    _padding: f32,
    planet_world_position: vec3<f32>,
    _planet_padding: f32,
};

struct GpuPlanetChunk {
    node_origin_planet: vec3<i32>,
    level: i32,
};

@group(2) @binding(1)
var<uniform> terrain_frame: GpuTerrainFrame;

@group(2) @binding(2)
var<storage, read> terrain_chunks: array<GpuPlanetChunk>;

@vertex
fn vs_shadow(
    vertex: VertexInput,
) -> @builtin(position) vec4<f32> {
    let chunk = terrain_chunks[vertex.chunk_index];
    let relative_anchor = chunk.node_origin_planet - terrain_frame.camera_anchor_planet;
    let camera_relative_position =
        vec3<f32>(relative_anchor) * terrain_frame.position_unit
        + vertex.position
        - terrain_frame.camera_remainder_planet;
    return shadow.view_proj * vec4<f32>(camera_relative_position, 1.0);
}
