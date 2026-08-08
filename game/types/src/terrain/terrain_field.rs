use std::collections::HashSet;

use engine::ecs::entity::Entity;
use engine::math::{DVec3, dvec3};
use serde::{Deserialize, Serialize};

pub const TERRAIN_FIELD_GRAPH_VERSION: u32 = 1;
pub const TERRAIN_CHANNEL_COUNT: usize = 8;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerrainFieldGraph {
    pub version: u32,
    pub name: String,
    pub seed: u64,
    pub radius: f64,
    pub sea_level: f64,
    pub layers: Vec<TerrainFieldLayer>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerrainFieldLayer {
    pub id: u64,
    pub name: String,
    pub enabled: bool,
    pub target: TerrainFieldChannel,
    pub operation: TerrainFieldOperation,
    pub source: TerrainFieldSource,
    pub mask: Option<TerrainFieldMask>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TerrainFieldSource {
    Constant {
        value: f64,
    },
    Latitude {
        amplitude: f64,
        bias: f64,
        absolute: bool,
    },
    Channel {
        channel: TerrainFieldChannel,
        input_min: f64,
        input_max: f64,
        output_min: f64,
        output_max: f64,
        smooth: bool,
    },
    Noise(TerrainNoiseNode),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerrainNoiseNode {
    pub kind: TerrainNoiseKind,
    pub domain: TerrainNoiseDomain,
    pub scale: f64,
    pub amplitude: f64,
    pub bias: f64,
    pub octaves: u8,
    pub lacunarity: f64,
    pub persistence: f64,
    pub warp_scale: f64,
    pub warp_strength: f64,
    pub seed_offset: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerrainFieldMask {
    pub channel: TerrainFieldChannel,
    pub minimum: f64,
    pub maximum: f64,
    pub smooth: bool,
    pub invert: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerrainFieldChannel {
    #[default]
    Continents,
    Land,
    Mountains,
    Plains,
    Rivers,
    Climate,
    Height,
    Density,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerrainFieldOperation {
    #[default]
    Add,
    Subtract,
    Multiply,
    Minimum,
    Maximum,
    Replace,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerrainNoiseKind {
    #[default]
    Fbm,
    Ridged,
    Billow,
    Cellular,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerrainNoiseDomain {
    #[default]
    SurfaceMeters,
    Angular,
    PositionMeters,
}

#[derive(Clone, Copy, Debug)]
pub struct TerrainFieldContext {
    pub direction: DVec3,
    pub position: DVec3,
    pub radius: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct TerrainFieldSample {
    pub channels: [f64; TERRAIN_CHANNEL_COUNT],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainValueRange {
    pub minimum: f64,
    pub maximum: f64,
}

#[derive(Clone, Debug)]
pub struct TerrainGraphApplyRequest {
    pub target: Entity,
    pub graph: TerrainFieldGraph,
}

#[derive(Default)]
pub struct TerrainGraphApplyQueue {
    pub requests: Vec<TerrainGraphApplyRequest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainFieldValidationError {
    pub layer_id: Option<u64>,
    pub message: String,
}

impl TerrainFieldChannel {
    pub const ALL: [Self; TERRAIN_CHANNEL_COUNT] = [
        Self::Continents,
        Self::Land,
        Self::Mountains,
        Self::Plains,
        Self::Rivers,
        Self::Climate,
        Self::Height,
        Self::Density,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::Continents => 0,
            Self::Land => 1,
            Self::Mountains => 2,
            Self::Plains => 3,
            Self::Rivers => 4,
            Self::Climate => 5,
            Self::Height => 6,
            Self::Density => 7,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Continents => "Continents",
            Self::Land => "Land",
            Self::Mountains => "Mountains",
            Self::Plains => "Plains",
            Self::Rivers => "Rivers",
            Self::Climate => "Climate",
            Self::Height => "Height",
            Self::Density => "Density",
        }
    }
}

impl TerrainFieldOperation {
    pub const ALL: [Self; 6] = [
        Self::Add,
        Self::Subtract,
        Self::Multiply,
        Self::Minimum,
        Self::Maximum,
        Self::Replace,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Add => "Add",
            Self::Subtract => "Subtract",
            Self::Multiply => "Multiply",
            Self::Minimum => "Minimum",
            Self::Maximum => "Maximum",
            Self::Replace => "Replace",
        }
    }
}

impl TerrainNoiseKind {
    pub const ALL: [Self; 4] = [Self::Fbm, Self::Ridged, Self::Billow, Self::Cellular];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Fbm => "fBm",
            Self::Ridged => "Ridged",
            Self::Billow => "Billow",
            Self::Cellular => "Cellular",
        }
    }
}

impl TerrainNoiseDomain {
    pub const ALL: [Self; 3] = [Self::SurfaceMeters, Self::Angular, Self::PositionMeters];

    pub const fn name(self) -> &'static str {
        match self {
            Self::SurfaceMeters => "Surface metres",
            Self::Angular => "Angular",
            Self::PositionMeters => "Position metres",
        }
    }
}

impl TerrainFieldGraph {
    pub fn evaluate(&self, context: TerrainFieldContext) -> TerrainFieldSample {
        let direction = context.direction.normalize_or_zero();
        let context = TerrainFieldContext {
            direction,
            position: context.position,
            radius: context.radius,
        };
        let mut channels = [0.0; TERRAIN_CHANNEL_COUNT];
        for layer in self.layers.iter().filter(|layer| layer.enabled) {
            let mut value = layer.source.evaluate(self.seed, context, &channels);
            if let Some(mask) = &layer.mask {
                value *= mask.evaluate(&channels);
            }
            let target = &mut channels[layer.target.index()];
            *target = match layer.operation {
                TerrainFieldOperation::Add => *target + value,
                TerrainFieldOperation::Subtract => *target - value,
                TerrainFieldOperation::Multiply => *target * value,
                TerrainFieldOperation::Minimum => target.min(value),
                TerrainFieldOperation::Maximum => target.max(value),
                TerrainFieldOperation::Replace => value,
            };
        }
        TerrainFieldSample { channels }
    }

    pub fn evaluate_direction(&self, direction: DVec3) -> TerrainFieldSample {
        let direction = direction.normalize_or_zero();
        self.evaluate(TerrainFieldContext {
            direction,
            position: direction * self.radius,
            radius: self.radius,
        })
    }

    pub fn channel_range(&self, channel: TerrainFieldChannel) -> TerrainValueRange {
        let mut channels = [TerrainValueRange::ZERO; TERRAIN_CHANNEL_COUNT];
        for layer in self.layers.iter().filter(|layer| layer.enabled) {
            let mut value = layer.source.range(&channels);
            if layer.mask.is_some() {
                value = value.multiply(TerrainValueRange::UNIT);
            }
            let target = channels[layer.target.index()];
            channels[layer.target.index()] = match layer.operation {
                TerrainFieldOperation::Add => target.add(value),
                TerrainFieldOperation::Subtract => target.subtract(value),
                TerrainFieldOperation::Multiply => target.multiply(value),
                TerrainFieldOperation::Minimum => target.minimum(value),
                TerrainFieldOperation::Maximum => target.maximum(value),
                TerrainFieldOperation::Replace => value,
            };
        }
        channels[channel.index()]
    }

    pub fn validate(&self) -> Vec<TerrainFieldValidationError> {
        let mut errors = Vec::new();
        if self.version > TERRAIN_FIELD_GRAPH_VERSION {
            errors.push(TerrainFieldValidationError {
                layer_id: None,
                message: format!("unsupported graph version {}", self.version),
            });
        }
        if self.name.trim().is_empty() {
            errors.push(TerrainFieldValidationError {
                layer_id: None,
                message: "graph name is empty".to_string(),
            });
        }
        if !self.radius.is_finite() || self.radius <= 0.0 {
            errors.push(TerrainFieldValidationError {
                layer_id: None,
                message: "radius must be finite and positive".to_string(),
            });
        }
        if !self.sea_level.is_finite() {
            errors.push(TerrainFieldValidationError {
                layer_id: None,
                message: "sea level must be finite".to_string(),
            });
        }
        let mut ids = HashSet::new();
        for layer in &self.layers {
            if !ids.insert(layer.id) {
                errors.push(TerrainFieldValidationError {
                    layer_id: Some(layer.id),
                    message: "layer id is duplicated".to_string(),
                });
            }
            if layer.name.trim().is_empty() {
                errors.push(TerrainFieldValidationError {
                    layer_id: Some(layer.id),
                    message: "layer name is empty".to_string(),
                });
            }
            layer.source.validate(layer.id, &mut errors);
            if let Some(mask) = &layer.mask
                && (!mask.minimum.is_finite()
                    || !mask.maximum.is_finite()
                    || mask.minimum >= mask.maximum)
            {
                errors.push(TerrainFieldValidationError {
                    layer_id: Some(layer.id),
                    message: "mask range must be finite and increasing".to_string(),
                });
            }
        }
        errors
    }

    pub fn next_layer_id(&self) -> u64 {
        self.layers
            .iter()
            .map(|layer| layer.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }
}

impl TerrainFieldSource {
    fn evaluate(
        &self,
        seed: u64,
        context: TerrainFieldContext,
        channels: &[f64; TERRAIN_CHANNEL_COUNT],
    ) -> f64 {
        match self {
            Self::Constant { value } => *value,
            Self::Latitude {
                amplitude,
                bias,
                absolute,
            } => {
                let latitude = if *absolute {
                    context.direction.y.abs()
                } else {
                    context.direction.y
                };
                latitude * amplitude + bias
            }
            Self::Channel {
                channel,
                input_min,
                input_max,
                output_min,
                output_max,
                smooth,
            } => {
                let mut t = remap01(channels[channel.index()], *input_min, *input_max);
                if *smooth {
                    t = smootherstep(t);
                }
                output_min + (output_max - output_min) * t
            }
            Self::Noise(node) => node.evaluate(seed, context),
        }
    }

    fn validate(&self, layer_id: u64, errors: &mut Vec<TerrainFieldValidationError>) {
        match self {
            Self::Constant { value } if !value.is_finite() => {
                errors.push(validation_error(layer_id, "constant value must be finite"))
            }
            Self::Latitude {
                amplitude, bias, ..
            } if !amplitude.is_finite() || !bias.is_finite() => errors.push(validation_error(
                layer_id,
                "latitude parameters must be finite",
            )),
            Self::Channel {
                input_min,
                input_max,
                output_min,
                output_max,
                ..
            } if !input_min.is_finite()
                || !input_max.is_finite()
                || input_min >= input_max
                || !output_min.is_finite()
                || !output_max.is_finite() =>
            {
                errors.push(validation_error(layer_id, "channel mapping is invalid"));
            }
            Self::Noise(node) => node.validate(layer_id, errors),
            _ => {}
        }
    }

    fn range(&self, _channels: &[TerrainValueRange; TERRAIN_CHANNEL_COUNT]) -> TerrainValueRange {
        match self {
            Self::Constant { value } => TerrainValueRange::new(*value, *value),
            Self::Latitude {
                amplitude,
                bias,
                absolute,
            } => {
                let input = if *absolute {
                    TerrainValueRange::UNIT
                } else {
                    TerrainValueRange::new(-1.0, 1.0)
                };
                input.scale(*amplitude).offset(*bias)
            }
            Self::Channel {
                output_min,
                output_max,
                ..
            } => TerrainValueRange::new(*output_min, *output_max),
            Self::Noise(node) => node.range(),
        }
    }
}

impl TerrainNoiseNode {
    fn evaluate(&self, seed: u64, context: TerrainFieldContext) -> f64 {
        let scale = self.scale.max(f64::EPSILON);
        let domain = match self.domain {
            TerrainNoiseDomain::SurfaceMeters => context.direction * context.radius / scale,
            TerrainNoiseDomain::Angular => context.direction * scale,
            TerrainNoiseDomain::PositionMeters => context.position / scale,
        };
        let warped = if self.warp_strength.abs() > f64::EPSILON {
            let warp_scale = self.warp_scale.max(f64::EPSILON);
            let q = domain / warp_scale;
            domain
                + dvec3(
                    fractal_noise(seed, self.seed_offset + 101, q, 3, 2.03, 0.5),
                    fractal_noise(seed, self.seed_offset + 211, q, 3, 2.03, 0.5),
                    fractal_noise(seed, self.seed_offset + 307, q, 3, 2.03, 0.5),
                ) * self.warp_strength
        } else {
            domain
        };
        let base = match self.kind {
            TerrainNoiseKind::Fbm => fractal_noise(
                seed,
                self.seed_offset,
                warped,
                self.octaves,
                self.lacunarity,
                self.persistence,
            ),
            TerrainNoiseKind::Ridged => {
                1.0 - fractal_noise(
                    seed,
                    self.seed_offset,
                    warped,
                    self.octaves,
                    self.lacunarity,
                    self.persistence,
                )
                .abs()
            }
            TerrainNoiseKind::Billow => {
                fractal_noise(
                    seed,
                    self.seed_offset,
                    warped,
                    self.octaves,
                    self.lacunarity,
                    self.persistence,
                )
                .abs()
                    * 2.0
                    - 1.0
            }
            TerrainNoiseKind::Cellular => cellular_noise(seed, self.seed_offset, warped),
        };
        base * self.amplitude + self.bias
    }

    fn validate(&self, layer_id: u64, errors: &mut Vec<TerrainFieldValidationError>) {
        if !self.scale.is_finite() || self.scale <= 0.0 {
            errors.push(validation_error(layer_id, "noise scale must be positive"));
        }
        if self.octaves == 0 || self.octaves > 12 {
            errors.push(validation_error(
                layer_id,
                "noise octaves must be between 1 and 12",
            ));
        }
        if !self.lacunarity.is_finite() || self.lacunarity <= 1.0 {
            errors.push(validation_error(
                layer_id,
                "noise lacunarity must be greater than 1",
            ));
        }
        if !self.persistence.is_finite() || !(0.0..=1.0).contains(&self.persistence) {
            errors.push(validation_error(
                layer_id,
                "noise persistence must be between 0 and 1",
            ));
        }
        for value in [
            self.amplitude,
            self.bias,
            self.warp_scale,
            self.warp_strength,
        ] {
            if !value.is_finite() {
                errors.push(validation_error(
                    layer_id,
                    "noise parameters must be finite",
                ));
                break;
            }
        }
    }

    fn range(&self) -> TerrainValueRange {
        let base = match self.kind {
            TerrainNoiseKind::Ridged => TerrainValueRange::UNIT,
            TerrainNoiseKind::Fbm | TerrainNoiseKind::Billow | TerrainNoiseKind::Cellular => {
                TerrainValueRange::new(-1.0, 1.0)
            }
        };
        base.scale(self.amplitude).offset(self.bias)
    }
}

impl TerrainValueRange {
    pub const ZERO: Self = Self {
        minimum: 0.0,
        maximum: 0.0,
    };
    pub const UNIT: Self = Self {
        minimum: 0.0,
        maximum: 1.0,
    };

    pub fn new(a: f64, b: f64) -> Self {
        Self {
            minimum: a.min(b),
            maximum: a.max(b),
        }
    }

    fn add(self, other: Self) -> Self {
        Self::new(self.minimum + other.minimum, self.maximum + other.maximum)
    }

    fn subtract(self, other: Self) -> Self {
        Self::new(self.minimum - other.maximum, self.maximum - other.minimum)
    }

    fn multiply(self, other: Self) -> Self {
        let products = [
            self.minimum * other.minimum,
            self.minimum * other.maximum,
            self.maximum * other.minimum,
            self.maximum * other.maximum,
        ];
        Self::new(
            products.into_iter().fold(f64::INFINITY, f64::min),
            products.into_iter().fold(f64::NEG_INFINITY, f64::max),
        )
    }

    fn minimum(self, other: Self) -> Self {
        Self::new(
            self.minimum.min(other.minimum),
            self.maximum.min(other.maximum),
        )
    }

    fn maximum(self, other: Self) -> Self {
        Self::new(
            self.minimum.max(other.minimum),
            self.maximum.max(other.maximum),
        )
    }

    fn scale(self, factor: f64) -> Self {
        Self::new(self.minimum * factor, self.maximum * factor)
    }

    fn offset(self, value: f64) -> Self {
        Self::new(self.minimum + value, self.maximum + value)
    }
}

impl TerrainFieldMask {
    fn evaluate(&self, channels: &[f64; TERRAIN_CHANNEL_COUNT]) -> f64 {
        let mut value = remap01(channels[self.channel.index()], self.minimum, self.maximum);
        if self.smooth {
            value = smootherstep(value);
        }
        if self.invert { 1.0 - value } else { value }
    }
}

impl Default for TerrainFieldGraph {
    fn default() -> Self {
        let noise = |kind, domain, scale, amplitude, seed_offset| {
            TerrainFieldSource::Noise(TerrainNoiseNode {
                kind,
                domain,
                scale,
                amplitude,
                bias: 0.0,
                octaves: 5,
                lacunarity: 2.03,
                persistence: 0.5,
                warp_scale: 1.0,
                warp_strength: 0.08,
                seed_offset,
            })
        };
        Self {
            version: TERRAIN_FIELD_GRAPH_VERSION,
            name: "Earth-like prototype".to_string(),
            seed: 1,
            radius: 6_371_000.0,
            sea_level: 0.0,
            layers: vec![
                TerrainFieldLayer {
                    id: 1,
                    name: "Continental shapes".to_string(),
                    enabled: true,
                    target: TerrainFieldChannel::Continents,
                    operation: TerrainFieldOperation::Replace,
                    source: noise(
                        TerrainNoiseKind::Fbm,
                        TerrainNoiseDomain::Angular,
                        2.2,
                        1.0,
                        10,
                    ),
                    mask: None,
                },
                TerrainFieldLayer {
                    id: 2,
                    name: "Land mask".to_string(),
                    enabled: true,
                    target: TerrainFieldChannel::Land,
                    operation: TerrainFieldOperation::Replace,
                    source: TerrainFieldSource::Channel {
                        channel: TerrainFieldChannel::Continents,
                        input_min: -0.2,
                        input_max: 0.32,
                        output_min: 0.0,
                        output_max: 1.0,
                        smooth: true,
                    },
                    mask: None,
                },
                TerrainFieldLayer {
                    id: 3,
                    name: "Mountain belts".to_string(),
                    enabled: true,
                    target: TerrainFieldChannel::Mountains,
                    operation: TerrainFieldOperation::Replace,
                    source: noise(
                        TerrainNoiseKind::Ridged,
                        TerrainNoiseDomain::Angular,
                        10.0,
                        1.0,
                        30,
                    ),
                    mask: Some(TerrainFieldMask {
                        channel: TerrainFieldChannel::Land,
                        minimum: 0.15,
                        maximum: 0.8,
                        smooth: true,
                        invert: false,
                    }),
                },
                TerrainFieldLayer {
                    id: 4,
                    name: "Plains".to_string(),
                    enabled: true,
                    target: TerrainFieldChannel::Plains,
                    operation: TerrainFieldOperation::Replace,
                    source: TerrainFieldSource::Channel {
                        channel: TerrainFieldChannel::Mountains,
                        input_min: 0.2,
                        input_max: 0.75,
                        output_min: 1.0,
                        output_max: 0.0,
                        smooth: true,
                    },
                    mask: Some(TerrainFieldMask {
                        channel: TerrainFieldChannel::Land,
                        minimum: 0.1,
                        maximum: 0.8,
                        smooth: true,
                        invert: false,
                    }),
                },
                TerrainFieldLayer {
                    id: 5,
                    name: "Continental elevation".to_string(),
                    enabled: true,
                    target: TerrainFieldChannel::Height,
                    operation: TerrainFieldOperation::Replace,
                    source: TerrainFieldSource::Channel {
                        channel: TerrainFieldChannel::Land,
                        input_min: 0.0,
                        input_max: 1.0,
                        output_min: -3_000.0,
                        output_max: 650.0,
                        smooth: true,
                    },
                    mask: None,
                },
                TerrainFieldLayer {
                    id: 6,
                    name: "Mountain elevation".to_string(),
                    enabled: true,
                    target: TerrainFieldChannel::Height,
                    operation: TerrainFieldOperation::Add,
                    source: TerrainFieldSource::Channel {
                        channel: TerrainFieldChannel::Mountains,
                        input_min: 0.35,
                        input_max: 1.0,
                        output_min: 0.0,
                        output_max: 4_200.0,
                        smooth: true,
                    },
                    mask: Some(TerrainFieldMask {
                        channel: TerrainFieldChannel::Land,
                        minimum: 0.2,
                        maximum: 0.8,
                        smooth: true,
                        invert: false,
                    }),
                },
                TerrainFieldLayer {
                    id: 7,
                    name: "River potential".to_string(),
                    enabled: true,
                    target: TerrainFieldChannel::Rivers,
                    operation: TerrainFieldOperation::Replace,
                    source: noise(
                        TerrainNoiseKind::Ridged,
                        TerrainNoiseDomain::SurfaceMeters,
                        42_000.0,
                        1.0,
                        70,
                    ),
                    mask: Some(TerrainFieldMask {
                        channel: TerrainFieldChannel::Land,
                        minimum: 0.25,
                        maximum: 0.85,
                        smooth: true,
                        invert: false,
                    }),
                },
            ],
        }
    }
}

fn validation_error(layer_id: u64, message: &str) -> TerrainFieldValidationError {
    TerrainFieldValidationError {
        layer_id: Some(layer_id),
        message: message.to_string(),
    }
}

fn remap01(value: f64, minimum: f64, maximum: f64) -> f64 {
    ((value - minimum) / (maximum - minimum)).clamp(0.0, 1.0)
}

fn smootherstep(value: f64) -> f64 {
    let t = value.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn hash01(seed: u64, salt: u64, x: i32, y: i32, z: i32) -> f64 {
    let mut value = seed ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    value ^= (x as u32 as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93);
    value ^= (y as u32 as u64).wrapping_mul(0xA5A3_56E2_7F88_6A4D);
    value ^= (z as u32 as u64).wrapping_mul(0x9E37_79B1_85EB_CA87);
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^= value >> 31;
    (value >> 11) as f64 * (1.0 / ((1_u64 << 53) as f64))
}

fn value_noise(seed: u64, salt: u64, p: DVec3) -> f64 {
    let cell = p.floor().as_ivec3();
    let fraction = p - cell.as_dvec3();
    let blend = dvec3(
        smootherstep(fraction.x),
        smootherstep(fraction.y),
        smootherstep(fraction.z),
    );
    let sample = |x, y, z| hash01(seed, salt, cell.x + x, cell.y + y, cell.z + z);
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let x00 = lerp(sample(0, 0, 0), sample(1, 0, 0), blend.x);
    let x10 = lerp(sample(0, 1, 0), sample(1, 1, 0), blend.x);
    let x01 = lerp(sample(0, 0, 1), sample(1, 0, 1), blend.x);
    let x11 = lerp(sample(0, 1, 1), sample(1, 1, 1), blend.x);
    let y0 = lerp(x00, x10, blend.y);
    let y1 = lerp(x01, x11, blend.y);
    lerp(y0, y1, blend.z) * 2.0 - 1.0
}

fn rotate_octave(v: DVec3) -> DVec3 {
    const COS: f64 = 0.613_745_749_488_811_6;
    const SIN: f64 = 0.789_503_739_689_950_5;
    let axis = dvec3(0.267_261_241_9, 0.534_522_483_8, 0.801_783_725_7);
    v * COS + axis.cross(v) * SIN + axis * axis.dot(v) * (1.0 - COS)
}

fn fractal_noise(
    seed: u64,
    salt: u64,
    mut p: DVec3,
    octaves: u8,
    lacunarity: f64,
    persistence: f64,
) -> f64 {
    let mut value = 0.0;
    let mut amplitude = 1.0;
    let mut amplitude_sum = 0.0;
    for octave in 0..octaves {
        value += value_noise(seed, salt + u64::from(octave), p) * amplitude;
        amplitude_sum += amplitude;
        amplitude *= persistence;
        p = rotate_octave(p) * lacunarity;
    }
    if amplitude_sum > 0.0 {
        value / amplitude_sum
    } else {
        0.0
    }
}

fn cellular_noise(seed: u64, salt: u64, p: DVec3) -> f64 {
    let cell = p.floor().as_ivec3();
    let mut nearest = f64::INFINITY;
    let mut second = f64::INFINITY;
    for z in -1..=1 {
        for y in -1..=1 {
            for x in -1..=1 {
                let candidate = cell + engine::math::ivec3(x, y, z);
                let point = candidate.as_dvec3()
                    + dvec3(
                        hash01(seed, salt, candidate.x, candidate.y, candidate.z),
                        hash01(seed, salt + 1, candidate.x, candidate.y, candidate.z),
                        hash01(seed, salt + 2, candidate.x, candidate.y, candidate.z),
                    );
                let distance = p.distance_squared(point);
                if distance < nearest {
                    second = nearest;
                    nearest = distance;
                } else if distance < second {
                    second = distance;
                }
            }
        }
    }
    (second.sqrt() - nearest.sqrt()).clamp(0.0, 1.0) * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_graph_is_valid_and_deterministic() {
        let graph = TerrainFieldGraph::default();
        assert!(graph.validate().is_empty());
        let direction = dvec3(0.31, 0.82, -0.48).normalize();
        let first = graph.evaluate_direction(direction);
        let second = graph.evaluate_direction(direction);
        for channel in TerrainFieldChannel::ALL {
            assert_eq!(
                first.channels[channel.index()].to_bits(),
                second.channels[channel.index()].to_bits()
            );
        }
    }

    #[test]
    fn validation_rejects_duplicate_ids_and_invalid_noise() {
        let mut graph = TerrainFieldGraph::default();
        graph.layers[1].id = graph.layers[0].id;
        if let TerrainFieldSource::Noise(node) = &mut graph.layers[0].source {
            node.scale = 0.0;
        }
        assert!(graph.validate().len() >= 2);
    }

    #[test]
    fn channel_ranges_contain_sampled_values() {
        let graph = TerrainFieldGraph::default();
        for channel in TerrainFieldChannel::ALL {
            let range = graph.channel_range(channel);
            for index in 0..1024 {
                let y = 1.0 - (2.0 * index as f64 + 1.0) / 1024.0;
                let radial = (1.0 - y * y).sqrt();
                let angle = index as f64 * 2.399_963_229_728_653;
                let direction = dvec3(radial * angle.cos(), y, radial * angle.sin());
                let value = graph.evaluate_direction(direction).channels[channel.index()];
                assert!(value >= range.minimum && value <= range.maximum);
            }
        }
    }
}
