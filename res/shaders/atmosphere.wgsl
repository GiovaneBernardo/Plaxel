struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(3.0, 1.0),
        vec2<f32>(-1.0, 1.0),
    );

    var out: VertexOutput;
    out.clip_position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let pos = vec2<f32>(in.clip_position.x, in.clip_position.y);
    let uv = pos.xy / vec2<f32>(3840.0, 2160.0);
    return vec4<f32>(uv, 0.0, 0.5);
    //return vec4<f32>(in.clip_position.x / 3840, in.clip_position.z / 2160, 0.9, 0.08);
}
