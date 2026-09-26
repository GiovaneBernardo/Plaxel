// Set to false and hot reload to restore normal terrain shading.
const DEBUG_LOD: bool = false;

const LOD_COLORS = array<vec3<f32>, 8>(
    vec3<f32>(1.0, 0.15, 0.15), // 0: red
    vec3<f32>(0.15, 1.0, 0.15), // 1: green
    vec3<f32>(0.2, 0.4, 1.0),   // 2: blue
    vec3<f32>(1.0, 1.0, 0.15),  // 3: yellow
    vec3<f32>(1.0, 0.15, 1.0),  // 4: magenta
    vec3<f32>(0.15, 1.0, 1.0),  // 5: cyan
    vec3<f32>(1.0, 0.5, 0.1),   // 6: orange
    vec3<f32>(0.65, 0.3, 1.0),  // 7: purple
);

struct CameraUniform {
    view_proj: mat4x4<f32>,
    position: vec3<f32>,
    time: f32,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

struct GpuTerrainFrame {
    view_projection_rotation: mat4x4<f32>,
    camera_anchor_planet: vec3<i32>,
    position_unit: f32,
    camera_remainder_planet: vec3<f32>,

    // Used as water animation time in seconds.
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
    @location(5)
    model_matrix_0: vec4<f32>,

    @location(6)
    model_matrix_1: vec4<f32>,

    @location(7)
    model_matrix_2: vec4<f32>,

    @location(8)
    model_matrix_3: vec4<f32>,
};

// Packed to match PlanetVertex in game/types/src/planet.rs:
//
// mats  = mat_a | (mat_b << 16)
// blend = low byte holds the 0..255 blend factor
struct VertexInput {
    @location(0)
    position: vec3<f32>,

    @location(1)
    normal: vec3<f32>,

    @location(2)
    mats: u32,

    @location(3)
    blend_packed: u32,

    @location(4)
    chunk_index: u32,
};

struct VertexOutput {
    @builtin(position)
    clip_position: vec4<f32>,

    @location(0)
    normal: vec3<f32>,

    // Camera-relative position.
    @location(1)
    world_position: vec3<f32>,

    @location(2) @interpolate(flat)
    mat_a: u32,

    @location(3) @interpolate(flat)
    mat_b: u32,

    @location(4)
    blend: f32,

    @location(5)
    camera_position: vec3<f32>,

    @location(6)
    shadow_position: vec4<f32>,

    // Planet/chunk anchored coordinates.
    // This is what the water waves use.
    @location(7)
    texture_position: vec3<f32>,

    @location(8) @interpolate(flat)
    level: i32,
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

// -----------------------------------------------------------------------------
// Utility
// -----------------------------------------------------------------------------

fn safe_normal(
    normal: vec3<f32>,
    fallback_position: vec3<f32>,
) -> vec3<f32> {
    let normal_len2 = dot(normal, normal);

    if normal_len2 > 0.000001 {
        return normal * inverseSqrt(normal_len2);
    }

    let fallback_len2 = dot(fallback_position, fallback_position);

    if fallback_len2 > 0.000001 {
        return fallback_position *
            inverseSqrt(fallback_len2);
    }

    return vec3<f32>(0.0, 1.0, 0.0);
}

// -----------------------------------------------------------------------------
// Vertex shader
// -----------------------------------------------------------------------------

@vertex
fn vs_main(
    model: VertexInput,
) -> VertexOutput {
    var out: VertexOutput;

    let chunk = terrain_chunks[model.chunk_index];

    let relative_anchor = chunk.node_origin_planet -
        terrain_frame.camera_anchor_planet;

    let camera_relative_position = vec3<f32>(relative_anchor) *
            terrain_frame.position_unit +
        model.position -
        terrain_frame.camera_remainder_planet;

    // Same coordinate system used by the terrain triplanar textures.
    //
    // Keeping this wrapped avoids huge floating point texture coordinates
    // while keeping neighboring chunks aligned.
    let texture_anchor = chunk.node_origin_planet %
        vec3<i32>(4096);

    let camera_relative = vec4<f32>(
        camera_relative_position,
        1.0,
    );

    out.world_position = camera_relative_position;

    out.texture_position = vec3<f32>(texture_anchor) *
            terrain_frame.position_unit +
        model.position;

    out.clip_position = terrain_frame.view_projection_rotation *
        camera_relative;

    out.normal = safe_normal(
        model.normal,
        model.position,
    );

    out.mat_a = u32(model.mats & 0xFFFFu);

    out.mat_b = u32(model.mats >> 16u);

    out.blend = f32(model.blend_packed & 0xFFu) /
        255.0;

    // world_position is camera-relative, therefore camera is origin.
    out.camera_position = vec3<f32>(0.0);

    out.shadow_position = shadow.view_proj *
        camera_relative;

    out.level = chunk.level;

    return out;
}

@vertex
fn vs_shadow(
    model: VertexInput,
) -> @builtin(position) vec4<f32> {
    let chunk = terrain_chunks[model.chunk_index];

    let relative_anchor = chunk.node_origin_planet -
        terrain_frame.camera_anchor_planet;

    let camera_relative_position = vec3<f32>(relative_anchor) *
            terrain_frame.position_unit +
        model.position -
        terrain_frame.camera_remainder_planet;

    return terrain_frame.view_projection_rotation *
        vec4<f32>(
        camera_relative_position,
        1.0,
    );
}

// -----------------------------------------------------------------------------
// Existing terrain/triplanar functions
// -----------------------------------------------------------------------------

fn get_material_color(
    index: u32,
) -> vec4<f32> {
    if index == 0 {
        return vec4<f32>(
            0.9,
            0.2,
            0.2,
            1.0,
        );
    } else if index == 1 {
        return vec4<f32>(
            0.2,
            0.9,
            0.2,
            1.0,
        );
    }

    return vec4<f32>(
        0.2,
        0.2,
        0.9,
        1.0,
    );
}

fn triplanar_sample(
    tex_index: u32,
    pos: vec3<f32>,
    normal: vec3<f32>,
    texture_scale: f32,
) -> vec4<f32> {
    let n = safe_normal(normal, pos);

    let an = abs(n);

    let weights = an /
        max(
        an.x + an.y + an.z,
        0.0001,
    );

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

    let x_sample = textureSample(
        my_textures[tex_index],
        default_sampler,
        x_uv,
    );

    let y_sample = textureSample(
        my_textures[tex_index],
        default_sampler,
        y_uv,
    );

    let z_sample = textureSample(
        my_textures[tex_index],
        default_sampler,
        z_uv,
    );

    return x_sample * weights.x +
        y_sample * weights.y +
        z_sample * weights.z;
}

fn sample_terrain_albedo(
    material_index: u32,
    pos: vec3<f32>,
    normal: vec3<f32>,
) -> vec4<f32> {
    let material = terrain_materials[material_index];

    return triplanar_sample(
        material.diffuse_texture_index,
        pos,
        normal,
        0.1,
    );
}

fn decode_normal_map(
    sample_value: vec4<f32>,
) -> vec3<f32> {
    var tangent_normal = sample_value.xyz * 2.0 -
        vec3<f32>(1.0);

    // Enable if your normal map uses the opposite
    // green-channel convention.
    //
    // tangent_normal.y = -tangent_normal.y;

    return safe_normal(
        tangent_normal,
        vec3<f32>(0.0, 0.0, 1.0),
    );
}

fn triplanar_sample_normal(
    tex_index: u32,
    pos: vec3<f32>,
    geometric_normal: vec3<f32>,
    texture_scale: f32,
) -> vec3<f32> {
    let n = safe_normal(
        geometric_normal,
        pos,
    );

    let an = abs(n);

    let weight_sum = max(
        an.x + an.y + an.z,
        0.0001,
    );

    let weights = an / weight_sum;

    let p = pos * texture_scale;

    let x_sign = select(
        -1.0,
        1.0,
        n.x >= 0.0,
    );

    let y_sign = select(
        -1.0,
        1.0,
        n.y >= 0.0,
    );

    let z_sign = select(
        -1.0,
        1.0,
        n.z >= 0.0,
    );

    // Same UV orientation used by the water below.
    let x_uv = vec2<f32>(
        p.z * x_sign,
        p.y,
    );

    let y_uv = vec2<f32>(
        p.x * y_sign,
        p.z,
    );

    let z_uv = vec2<f32>(
        p.x * z_sign,
        p.y,
    );

    let x_tangent = decode_normal_map(
        textureSample(
            my_textures[tex_index],
            default_sampler,
            x_uv,
        ),
    );

    let y_tangent = decode_normal_map(
        textureSample(
            my_textures[tex_index],
            default_sampler,
            y_uv,
        ),
    );

    let z_tangent = decode_normal_map(
        textureSample(
            my_textures[tex_index],
            default_sampler,
            z_uv,
        ),
    );

    // X projection:
    //
    // U = signed Z
    // V = Y
    // outward = signed X
    let x_normal = vec3<f32>(
        x_sign * x_tangent.z,
        x_tangent.y,
        x_sign * x_tangent.x,
    );

    // Y projection:
    //
    // U = signed X
    // V = Z
    // outward = signed Y
    let y_normal = vec3<f32>(
        y_sign * y_tangent.x,
        y_sign * y_tangent.z,
        y_tangent.y,
    );

    // Z projection:
    //
    // U = signed X
    // V = Y
    // outward = signed Z
    let z_normal = vec3<f32>(
        z_sign * z_tangent.x,
        z_tangent.y,
        z_sign * z_tangent.z,
    );

    let blended = x_normal * weights.x +
        y_normal * weights.y +
        z_normal * weights.z;

    return safe_normal(
        blended,
        n,
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

// -----------------------------------------------------------------------------
// Shadows
// -----------------------------------------------------------------------------

fn shadow_visibility(
    shadow_position: vec4<f32>,
) -> f32 {
    let ndc = shadow_position.xyz /
        shadow_position.w;

    if abs(ndc.x) > 1.0 ||
        abs(ndc.y) > 1.0 ||
        ndc.z < 0.0 ||
        ndc.z > 1.0 {
        return 1.0;
    }

    // WGPU framebuffer Y is opposite NDC Y when
    // addressed as a texture.
    let uv = vec2<f32>(
        ndc.x * 0.5 + 0.5,
        0.5 - ndc.y * 0.5,
    );

    let dimensions = textureDimensions(
        shadow_depth_map,
    );

    let center = vec2<i32>(
        uv *
            vec2<f32>(dimensions),
    );

    let maximum = vec2<i32>(dimensions) -
        vec2<i32>(1);

    var visibility = 0.0;

    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let pixel = clamp(
                center +
                    vec2<i32>(x, y),
                vec2<i32>(0),
                maximum,
            );

            let stored_depth = textureLoad(
                shadow_depth_map,
                pixel,
                0,
            );

            // The shadow map uses conventional depth, independently of the main camera.
            visibility += select(
                0.0,
                1.0,
                ndc.z -
                    shadow.depth_bias <=
                    stored_depth,
            );
        }
    }

    return visibility / 9.0;
}

// =============================================================================
// WATER
// =============================================================================

// Water color when looking more directly into the surface.
const WATER_DEEP_COLOR: vec3<f32> = vec3<f32>(
    0.004,
    0.025,
    0.055,
);

// Water color under direct light.
const WATER_SURFACE_COLOR: vec3<f32> = vec3<f32>(
    0.015,
    0.12,
    0.19,
);

// Fake reflected sky color.
const WATER_REFLECTION_COLOR: vec3<f32> = vec3<f32>(
    0.18,
    0.38,
    0.58,
);

// This makes the procedural water coordinate scale correspond
// roughly to your terrain triplanar texture scale.
//
// Terrain currently uses 0.1.
const WATER_TEXTURE_SCALE: f32 = 1.0;

// -----------------------------------------------------------------------------
// Single procedural 2D wave
// -----------------------------------------------------------------------------
//
// Returns the slope in UV space.
//
// It behaves somewhat like generating a procedural tangent-space
// normal map instead of sampling one from a texture.
//
fn water_wave_2d(
    uv: vec2<f32>,
    direction: vec2<f32>,
    frequency: f32,
    speed: f32,
    strength: f32,
    time: f32,
) -> vec2<f32> {
    let dir = normalize(direction);

    let phase = dot(uv, dir) *
            frequency +
        time *
            speed;

    return dir *
        cos(phase) *
        strength;
}

// -----------------------------------------------------------------------------
// Procedural tangent-space water normal
// -----------------------------------------------------------------------------

fn procedural_water_normal(
    uv: vec2<f32>,
    time: f32,
) -> vec3<f32> {
    var slope = vec2<f32>(0.0);

    // Large rolling wave.
    slope += water_wave_2d(
        uv,
        vec2<f32>(
            1.0,
            0.35,
        ),
        0.35,
        0.70,
        0.14,
        time,
    );

    // Crossing wave.
    slope += water_wave_2d(
        uv,
        vec2<f32>(
            -0.4,
            1.0,
        ),
        0.55,
        -0.90,
        0.09,
        time,
    );

    // Medium detail.
    slope += water_wave_2d(
        uv,
        vec2<f32>(
            0.75,
            -0.65,
        ),
        1.20,
        1.30,
        0.04,
        time,
    );

    // Fine ripples.
    slope += water_wave_2d(
        uv,
        vec2<f32>(
            -0.85,
            -0.25,
        ),
        2.1,
        -1.75,
        0.015,
        time,
    );

    // Tangent-space normal:
    //
    // X/Y = wave slope
    // Z   = outward from projection plane
    return safe_normal(
        vec3<f32>(
            -slope.x,
            -slope.y,
            1.0,
        ),
        vec3<f32>(
            0.0,
            0.0,
            1.0,
        ),
    );
}

// -----------------------------------------------------------------------------
// Triplanar procedural water normal
// -----------------------------------------------------------------------------
//
// This follows the SAME coordinate projection convention as
// triplanar_sample_normal().
//
// X-facing surface:
//     UV = signed Z, Y
//
// Y-facing surface:
//     UV = signed X, Z
//
// Z-facing surface:
//     UV = signed X, Y
//
// This is the important part that keeps the waves planet/world anchored
// instead of camera anchored.
//
fn water_normal_triplanar(
    pos: vec3<f32>,
    geometric_normal: vec3<f32>,
    time: f32,
    distance_to_camera: f32,
) -> vec3<f32> {
    let n = safe_normal(
        geometric_normal,
        pos,
    );

    let an = abs(n);

    let weight_sum = max(
        an.x + an.y + an.z,
        0.0001,
    );

    let weights = an / weight_sum;

    // Apply the same base coordinate scaling that terrain triplanar
    // textures use.
    let p = pos *
        WATER_TEXTURE_SCALE;

    // Match the sign conventions from triplanar_sample_normal().
    let x_sign = select(
        -1.0,
        1.0,
        n.x >= 0.0,
    );

    let y_sign = select(
        -1.0,
        1.0,
        n.y >= 0.0,
    );

    let z_sign = select(
        -1.0,
        1.0,
        n.z >= 0.0,
    );

    // -------------------------------------------------------------------------
    // World/planet coordinate projections
    // -------------------------------------------------------------------------

    let x_uv = vec2<f32>(
        p.z * x_sign,
        p.y,
    );

    let y_uv = vec2<f32>(
        p.x * y_sign,
        p.z,
    );

    let z_uv = vec2<f32>(
        p.x * z_sign,
        p.y,
    );

    // Generate what are effectively three procedural
    // tangent-space normal maps.
    let x_tangent = procedural_water_normal(
        x_uv,
        time,
    );

    let y_tangent = procedural_water_normal(
        y_uv,
        time,
    );

    let z_tangent = procedural_water_normal(
        z_uv,
        time,
    );

    // -------------------------------------------------------------------------
    // Tangent -> planet/world normal
    // -------------------------------------------------------------------------

    // X projection:
    //
    // U = signed Z
    // V = Y
    // outward = signed X
    let x_normal = vec3<f32>(
        x_sign *
                x_tangent.z,
        x_tangent.y,
        x_sign *
                x_tangent.x,
    );

    // Y projection:
    //
    // U = signed X
    // V = Z
    // outward = signed Y
    let y_normal = vec3<f32>(
        y_sign *
                y_tangent.x,
        y_sign *
                y_tangent.z,
        y_tangent.y,
    );

    // Z projection:
    //
    // U = signed X
    // V = Y
    // outward = signed Z
    let z_normal = vec3<f32>(
        z_sign *
                z_tangent.x,
        z_tangent.y,
        z_sign *
                z_tangent.z,
    );

    // Blend the three projections exactly like normal-map
    // triplanar mapping.
    let blended = x_normal *
            weights.x +
        y_normal *
            weights.y +
        z_normal *
            weights.z;

    let mapped_normal = safe_normal(
        blended,
        n,
    );

    // -------------------------------------------------------------------------
    // Distance fade
    // -------------------------------------------------------------------------
    //
    // Tiny high-frequency normal changes get very noisy near the
    // planetary horizon. Blend them back toward the geometric normal.
    //
    let wave_fade = 1.0 -
        smoothstep(
        3000.0,
        14000.0,
        distance_to_camera,
    );

    return safe_normal(
        mix(
            n,
            mapped_normal,
            wave_fade,
        ),
        n,
    );
}

// =============================================================================
// WATER FRAGMENT SHADER
// =============================================================================

@fragment
fn fs_main(
    in: VertexOutput,
) -> @location(0) vec4<f32> {
    // Geometric planet/ocean normal.
    let geometric_normal = safe_normal(
        in.normal,
        in.world_position,
    );

    // Since world_position is camera-relative and camera_position is 0,
    // this vector points from surface -> camera.
    let view_vector = in.camera_position -
        in.world_position;

    let distance_to_camera = length(view_vector);

    let view_dir = safe_normal(
        view_vector,
        geometric_normal,
    );

    // Feed elapsed seconds into this field from the CPU.
    //
    // If you don't have time wired up yet, temporarily use:
    //
    // let time = 0.0;
    //
    let time = camera.time * 10.0; //terrain_frame._padding;

    // IMPORTANT:
    //
    // Use texture_position here, NOT world_position.
    //
    // world_position is camera-relative.
    // texture_position is anchored to the planet/chunks just like
    // your terrain triplanar textures.
    let normal = water_normal_triplanar(
        in.texture_position,
        geometric_normal,
        time,
        distance_to_camera,
    );

    let light_dir = safe_normal(
        shadow.light_direction,
        vec3<f32>(
            0.0,
            1.0,
            0.0,
        ),
    );

    // -------------------------------------------------------------------------
    // Fresnel
    // -------------------------------------------------------------------------

    let ndotv = clamp(
        dot(
            normal,
            view_dir,
        ),
        0.0,
        1.0,
    );

    // Schlick-ish Fresnel.
    //
    // Straight down:
    //     mostly deep water color
    //
    // Grazing angle:
    //     much more sky/reflection
    //
    let one_minus_ndotv = 1.0 - ndotv;

    let fresnel = 0.02 +
        0.98 *
        pow(
        one_minus_ndotv,
        5.0,
    );

    // -------------------------------------------------------------------------
    // Diffuse / water body color
    // -------------------------------------------------------------------------

    let diffuse = max(
        dot(
            normal,
            light_dir,
        ),
        0.0,
    );

    var water_color = mix(
        WATER_DEEP_COLOR,
        WATER_SURFACE_COLOR,
        0.35 +
            diffuse * 0.35,
    );

    // -------------------------------------------------------------------------
    // Fake environment / sky reflection
    // -------------------------------------------------------------------------

    water_color = mix(
        water_color,
        WATER_REFLECTION_COLOR,
        fresnel * 0.75,
    );

    // -------------------------------------------------------------------------
    // Sun specular
    // -------------------------------------------------------------------------

    let half_dir = safe_normal(
        light_dir +
            view_dir,
        normal,
    );

    let ndoth = max(
        dot(
            normal,
            half_dir,
        ),
        0.0,
    );

    // Tight glint + softer surrounding highlight.
    let sun_specular = pow(
        ndoth,
        256.0,
    ) * 2.5 +

        pow(
        ndoth,
        64.0,
    ) * 0.20;

    // Replace with:
    //
    // let visibility =
    //     shadow_visibility(in.shadow_position);
    //
    // when you want the water specular to receive terrain shadows.
    let visibility = 1.0;

    let sun_color = vec3<f32>(
        1.0,
        0.92,
        0.72,
    );

    water_color += sun_color *
        sun_specular *
        visibility;

    // -------------------------------------------------------------------------
    // Slight Fresnel horizon brightening
    // -------------------------------------------------------------------------

    water_color += vec3<f32>(
        0.015,
        0.035,
        0.045,
    ) *
        fresnel;

    // -------------------------------------------------------------------------
    // Distance fog
    // -------------------------------------------------------------------------

    let fog_start = 5000.0;

    let fog_end = 15000.0;

    let fog = clamp(
        (distance_to_camera -
                fog_start) /
            (fog_end -
                fog_start),
        0.0,
        1.0,
    );

    let fog_color = vec3<f32>(
        0.10,
        0.20,
        0.30,
    );

    //water_color = mix(
    //    water_color,
    //    fog_color,
    //    fog * 0.35,
    //);

    let water_alpha = mix(
        0.75,
        0.92,
        fresnel,
    );

    // -------------------------------------------------------------------------
    // Debug LOD
    // -------------------------------------------------------------------------

    if DEBUG_LOD {
        let lod_color = LOD_COLORS[u32(
            max(
                in.level,
                0,
            ),
        ) %
                8u];

        return vec4<f32>(
            water_color *
            lod_color,
            1.0,
        );
    }

    return vec4<f32>(
        water_color,
        1.0,
    );
}
