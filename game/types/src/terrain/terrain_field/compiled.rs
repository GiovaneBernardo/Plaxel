//! Density-only execution plan. Four independent positions share instruction
//! dispatch and SIMD noise arithmetic; scalar graph evaluation stays the oracle.
use super::*;
use std::simd::{
    Simd, StdFloat,
    cmp::SimdPartialEq,
    num::{SimdFloat, SimdInt, SimdUint},
};

type F = Simd<f64, 4>;
type I = Simd<i32, 4>;
type U = Simd<u64, 4>;
type Point = [F; 3];

#[derive(Clone, Debug)]
pub struct CompiledTerrainField {
    seed: u64,
    instructions: Vec<TerrainFieldLayer>,
}

impl TerrainFieldGraph {
    pub fn compile_density(&self) -> CompiledTerrainField {
        // Track the value *at each instruction*, including reads of the target
        // itself. An overwritten value and output-only climate work are dead.
        let mut live = [false; TERRAIN_CHANNEL_COUNT];
        live[TerrainFieldChannel::Height.index()] = true;
        live[TerrainFieldChannel::Density.index()] = true;
        let mut instructions = Vec::new();
        for layer in self.layers.iter().rev().filter(|l| l.enabled) {
            let target = layer.target.index();
            if !live[target] {
                continue;
            }
            live[target] = layer.operation != TerrainFieldOperation::Replace;
            if let TerrainFieldSource::Channel { channel, .. } = layer.source {
                live[channel.index()] = true;
            }
            if let Some(mask) = &layer.mask {
                live[mask.channel.index()] = true;
            }
            instructions.push(layer.clone());
        }
        instructions.reverse();
        CompiledTerrainField {
            seed: self.seed,
            instructions,
        }
    }
}

impl CompiledTerrainField {
    /// Planet-local positions, with the same f64 arithmetic and f32 output as
    /// the scalar sampler. Works on all targets; AVX2 is selected at runtime.
    pub fn densities(&self, positions: &[DVec3], radius: f64, output: &mut [f32]) {
        assert_eq!(positions.len(), output.len());
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the feature was checked on this machine.
            unsafe {
                self.densities_avx2(positions, radius, output);
            }
            return;
        }
        self.densities_impl(positions, radius, output);
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn densities_avx2(&self, positions: &[DVec3], radius: f64, output: &mut [f32]) {
        self.densities_impl(positions, radius, output);
    }

    #[inline(always)]
    fn densities_impl(&self, positions: &[DVec3], radius: f64, output: &mut [f32]) {
        for (points, densities) in positions.chunks(4).zip(output.chunks_mut(4)) {
            let p = std::array::from_fn(|axis| {
                F::from_array(std::array::from_fn(|lane| {
                    points[lane.min(points.len() - 1)][axis]
                }))
            });
            let distance = dot(p, p).sqrt();
            // Match both normalization steps in base_sdf_planet_local/evaluate.
            let mut direction = std::array::from_fn(|axis| p[axis] / distance);
            for lane in 0..4 {
                if distance[lane] <= 1.0e-10 {
                    direction[0][lane] = 0.0;
                    direction[1][lane] = 1.0;
                    direction[2][lane] = 0.0;
                }
            }
            let reciprocal = F::splat(1.0) / dot(direction, direction).sqrt();
            direction = direction.map(|v| v * reciprocal);
            let mut channels = [F::splat(0.0); TERRAIN_CHANNEL_COUNT];
            for layer in &self.instructions {
                let mask = layer.mask.as_ref().map(|m| {
                    let t = remap(channels[m.channel.index()], m.minimum, m.maximum, m.smooth);
                    if m.invert { F::splat(1.0) - t } else { t }
                });
                // Finite, validated sources make a zero mask independent of
                // the expensive source. Preserve the operation (Replace and
                // Multiply with zero are not no-ops).
                let mut value = if mask.is_some_and(|m| m.simd_eq(F::splat(0.0)).all()) {
                    F::splat(0.0)
                } else {
                    match &layer.source {
                        TerrainFieldSource::Constant { value } => F::splat(*value),
                        TerrainFieldSource::Latitude {
                            amplitude,
                            bias,
                            absolute,
                        } => {
                            let y = if *absolute {
                                direction[1].abs()
                            } else {
                                direction[1]
                            };
                            y * F::splat(*amplitude) + F::splat(*bias)
                        }
                        TerrainFieldSource::Channel {
                            channel,
                            input_min,
                            input_max,
                            output_min,
                            output_max,
                            smooth,
                        } => {
                            F::splat(*output_min)
                                + F::splat(output_max - output_min)
                                    * remap(
                                        channels[channel.index()],
                                        *input_min,
                                        *input_max,
                                        *smooth,
                                    )
                        }
                        TerrainFieldSource::Noise(node) => {
                            noise(self.seed, node, p, direction, radius)
                        }
                    }
                };
                if let Some(mask) = mask {
                    value *= mask;
                }
                let target = &mut channels[layer.target.index()];
                *target = match layer.operation {
                    TerrainFieldOperation::Add => *target + value,
                    TerrainFieldOperation::Subtract => *target - value,
                    TerrainFieldOperation::Multiply => *target * value,
                    TerrainFieldOperation::Minimum => target.simd_min(value),
                    TerrainFieldOperation::Maximum => target.simd_max(value),
                    TerrainFieldOperation::Replace => value,
                };
            }
            let result = distance
                - (F::splat(radius) + channels[TerrainFieldChannel::Height.index()])
                + channels[TerrainFieldChannel::Density.index()];
            for (lane, out) in densities.iter_mut().enumerate() {
                *out = result[lane] as f32;
            }
        }
    }
}

#[inline(always)]
fn dot(a: Point, b: Point) -> F {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

#[inline(always)]
fn smooth(t: F) -> F {
    let t = t.simd_clamp(F::splat(0.0), F::splat(1.0));
    t * t * t * (t * (t * F::splat(6.0) - F::splat(15.0)) + F::splat(10.0))
}

#[inline(always)]
fn remap(v: F, lo: f64, hi: f64, smoothing: bool) -> F {
    let t = ((v - F::splat(lo)) / F::splat(hi - lo)).simd_clamp(F::splat(0.0), F::splat(1.0));
    if smoothing { smooth(t) } else { t }
}

#[inline(always)]
fn hash(seed: u64, salt: u64, cell: [I; 3]) -> F {
    let mut v = U::splat(seed ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    v ^= cell[0].cast::<u32>().cast::<u64>() * U::splat(0xD6E8_FEB8_6659_FD93);
    v ^= cell[1].cast::<u32>().cast::<u64>() * U::splat(0xA5A3_56E2_7F88_6A4D);
    v ^= cell[2].cast::<u32>().cast::<u64>() * U::splat(0x9E37_79B1_85EB_CA87);
    v ^= v >> 30;
    v *= U::splat(0xBF58_476D_1CE4_E5B9);
    v ^= v >> 27;
    v *= U::splat(0x94D0_49BB_1331_11EB);
    v ^= v >> 31;
    (v >> 11).cast::<f64>() * F::splat(1.0 / ((1_u64 << 53) as f64))
}

#[inline(always)]
fn value(seed: u64, salt: u64, p: Point) -> F {
    let cell = p.map(|v| v.floor().cast::<i32>());
    let t: Point = std::array::from_fn(|i| smooth(p[i] - cell[i].cast::<f64>()));
    // Nearby samples usually share a lattice cell, especially at fine LOD.
    // Hash its corners once, then interpolate all four positions in SIMD.
    let shared_cell = cell.iter().all(|v| v.simd_eq(I::splat(v[0])).all());
    let sample = |x, y, z| {
        if shared_cell {
            F::splat(hash01(
                seed,
                salt,
                cell[0][0] + x,
                cell[1][0] + y,
                cell[2][0] + z,
            ))
        } else {
            hash(
                seed,
                salt,
                [
                    cell[0] + I::splat(x),
                    cell[1] + I::splat(y),
                    cell[2] + I::splat(z),
                ],
            )
        }
    };
    let lerp = |a: F, b: F, t: F| a + (b - a) * t;
    let x00 = lerp(sample(0, 0, 0), sample(1, 0, 0), t[0]);
    let x10 = lerp(sample(0, 1, 0), sample(1, 1, 0), t[0]);
    let x01 = lerp(sample(0, 0, 1), sample(1, 0, 1), t[0]);
    let x11 = lerp(sample(0, 1, 1), sample(1, 1, 1), t[0]);
    lerp(lerp(x00, x10, t[1]), lerp(x01, x11, t[1]), t[2]) * F::splat(2.0) - F::splat(1.0)
}

#[inline(always)]
fn rotate(p: Point) -> Point {
    const COS: f64 = 0.613_745_749_488_811_6;
    const SIN: f64 = 0.789_503_739_689_950_5;
    let axis = [0.267_261_241_9, 0.534_522_483_8, 0.801_783_725_7].map(F::splat);
    let d = dot(axis, p);
    std::array::from_fn(|i| {
        let j = (i + 1) % 3;
        let k = (i + 2) % 3;
        p[i] * F::splat(COS)
            + (axis[j] * p[k] - axis[k] * p[j]) * F::splat(SIN)
            + axis[i] * d * F::splat(1.0 - COS)
    })
}

#[inline(always)]
fn fractal(
    seed: u64,
    salt: u64,
    mut p: Point,
    octaves: u8,
    lacunarity: f64,
    persistence: f64,
) -> F {
    let mut result = F::splat(0.0);
    let mut amplitude = 1.0;
    let mut sum = 0.0;
    for octave in 0..octaves {
        result += value(seed, salt + u64::from(octave), p) * F::splat(amplitude);
        sum += amplitude;
        amplitude *= persistence;
        p = rotate(p).map(|v| v * F::splat(lacunarity));
    }
    if sum > 0.0 {
        result / F::splat(sum)
    } else {
        F::splat(0.0)
    }
}

#[inline(always)]
fn noise(seed: u64, n: &TerrainNoiseNode, p: Point, direction: Point, radius: f64) -> F {
    let scale = F::splat(n.scale.max(f64::EPSILON));
    let mut domain = match n.domain {
        TerrainNoiseDomain::SurfaceMeters => direction.map(|v| v * F::splat(radius) / scale),
        TerrainNoiseDomain::Angular => direction.map(|v| v * scale),
        TerrainNoiseDomain::PositionMeters => p.map(|v| v / scale),
    };
    if n.warp_strength.abs() > f64::EPSILON {
        let q = domain.map(|v| v / F::splat(n.warp_scale.max(f64::EPSILON)));
        for (i, salt) in [101, 211, 307].into_iter().enumerate() {
            domain[i] +=
                fractal(seed, n.seed_offset + salt, q, 3, 2.03, 0.5) * F::splat(n.warp_strength);
        }
    }
    let base = match n.kind {
        TerrainNoiseKind::Cellular => F::from_array(std::array::from_fn(|i| {
            cellular_noise(
                seed,
                n.seed_offset,
                dvec3(domain[0][i], domain[1][i], domain[2][i]),
            )
        })),
        _ => {
            let f = fractal(
                seed,
                n.seed_offset,
                domain,
                n.octaves,
                n.lacunarity,
                n.persistence,
            );
            match n.kind {
                TerrainNoiseKind::Ridged => F::splat(1.0) - f.abs(),
                TerrainNoiseKind::Billow => f.abs() * F::splat(2.0) - F::splat(1.0),
                _ => f,
            }
        }
    };
    base * F::splat(n.amplitude) + F::splat(n.bias)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_density_matches_scalar_domains_shapes_masks_and_operations() {
        let mut graph = TerrainFieldGraph::default();
        let positions: Vec<_> = (0..39)
            .map(|i| {
                if i == 0 {
                    DVec3::ZERO
                } else {
                    dvec3((i as f64 * 0.731).sin(), (i as f64 * 1.79).cos(), -0.37).normalize()
                        * (graph.radius + i as f64 * 17.0)
                }
            })
            .collect();
        for domain in TerrainNoiseDomain::ALL {
            for kind in TerrainNoiseKind::ALL {
                for operation in TerrainFieldOperation::ALL {
                    for layer in &mut graph.layers {
                        if let TerrainFieldSource::Noise(n) = &mut layer.source {
                            n.domain = domain;
                            n.kind = kind;
                            n.warp_strength = 0.37;
                            n.scale = 173.0;
                        }
                    }
                    let last = graph.layers.last_mut().unwrap();
                    last.operation = operation;
                    let compiled = graph.compile_density();
                    let mut actual = vec![0.0; positions.len()];
                    compiled.densities(&positions, graph.radius, &mut actual);
                    let mut portable = vec![0.0; positions.len()];
                    compiled.densities_impl(&positions, graph.radius, &mut portable);
                    assert_eq!(actual, portable, "runtime AVX2 and portable paths disagree");
                    for (&p, &a) in positions.iter().zip(&actual) {
                        let distance = p.length();
                        let s = graph.evaluate(TerrainFieldContext {
                            position: p,
                            direction: if distance > 1.0e-10 {
                                p / distance
                            } else {
                                DVec3::Y
                            },
                            radius: graph.radius,
                        });
                        let expected =
                            (distance - (graph.radius + s.channels[6]) + s.channels[7]) as f32;
                        assert_eq!(a, expected, "{domain:?}/{kind:?}/{operation:?} at {p:?}");
                    }
                }
            }
        }
    }
}
