//! Spatial interval evaluation of the same field used by the terrain sampler.
//! Large noise queries fall back to amplitude bounds; small ones bound the
//! actual lattice interpolation, without assuming that probe signs are enough.

use super::*;

#[derive(Clone, Copy)]
struct BoxRange {
    min: DVec3,
    max: DVec3,
}

impl BoxRange {
    fn padded(self) -> Self {
        let margin =
            self.min.abs().max(self.max.abs()) * (128.0 * f64::EPSILON) + DVec3::splat(1.0e-12);
        Self {
            min: self.min - margin,
            max: self.max + margin,
        }
    }

    fn scale(self, factor: f64) -> Self {
        let a = self.min * factor;
        let b = self.max * factor;
        Self {
            min: a.min(b),
            max: a.max(b),
        }
        .padded()
    }

    fn radius_range(self) -> TerrainValueRange {
        TerrainValueRange::new(
            DVec3::ZERO.clamp(self.min, self.max).length(),
            self.min.abs().max(self.max.abs()).length(),
        )
    }

    fn directions(self) -> Self {
        if self.radius_range().minimum <= 1.0e-10 {
            // Also includes the sampler's fallback direction at the origin.
            return Self {
                min: DVec3::splat(-1.0),
                max: DVec3::ONE,
            };
        }
        let nearest = DVec3::ZERO.clamp(self.min, self.max);
        let farthest = self.min.abs().max(self.max.abs());
        let mut min = DVec3::ZERO;
        let mut max = DVec3::ZERO;
        for axis in 0..3 {
            let a = (axis + 1) % 3;
            let b = (axis + 2) % 3;
            let near_squared = nearest[a] * nearest[a] + nearest[b] * nearest[b];
            let far_squared = farthest[a] * farthest[a] + farthest[b] * farthest[b];
            let lo = self.min[axis];
            let hi = self.max[axis];
            // x/length(p) increases with x. For positive x it decreases
            // with perpendicular distance; for negative x the reverse holds.
            min[axis] = lo / (lo * lo + if lo < 0.0 { near_squared } else { far_squared }).sqrt();
            max[axis] = hi / (hi * hi + if hi > 0.0 { near_squared } else { far_squared }).sqrt();
        }
        Self { min, max }.padded()
    }
}

impl TerrainValueRange {
    fn padded(self) -> Self {
        let margin = self.maximum_magnitude() * (128.0 * f64::EPSILON) + 1.0e-12;
        Self {
            minimum: self.minimum - margin,
            maximum: self.maximum + margin,
        }
    }

    fn absolute(self) -> Self {
        Self::new(
            if self.minimum <= 0.0 && self.maximum >= 0.0 {
                0.0
            } else {
                self.minimum.abs().min(self.maximum.abs())
            },
            self.maximum_magnitude(),
        )
    }

    fn union(self, other: Self) -> Self {
        Self::new(
            self.minimum.min(other.minimum),
            self.maximum.max(other.maximum),
        )
    }
}

impl TerrainFieldGraph {
    /// Conservative density range over a planet-local box, before player edits.
    /// Bounds follow all graph operations and all three noise domains. `None`
    /// means that the query could not be bounded, never that it is empty.
    pub fn density_range_in_box(
        &self,
        local_min: DVec3,
        local_max: DVec3,
        planet_radius: f64,
    ) -> Option<TerrainValueRange> {
        if !local_min.is_finite()
            || !local_max.is_finite()
            || !local_min.cmple(local_max).all()
            || !planet_radius.is_finite()
        {
            return None;
        }
        let position = BoxRange {
            min: local_min,
            max: local_max,
        }
        .padded();
        let directions = position.directions();
        let mut channels = [TerrainValueRange::ZERO; TERRAIN_CHANNEL_COUNT];
        for layer in self.layers.iter().filter(|layer| layer.enabled) {
            let mask = layer.mask.as_ref().map(|mask| mask.range(&channels));
            let source = if mask == Some(TerrainValueRange::ZERO) {
                TerrainValueRange::ZERO
            } else {
                match &layer.source {
                    TerrainFieldSource::Noise(noise) => {
                        let scale = noise.scale.max(f64::EPSILON);
                        let domain = match noise.domain {
                            TerrainNoiseDomain::PositionMeters => position.scale(1.0 / scale),
                            TerrainNoiseDomain::Angular => directions.scale(scale),
                            TerrainNoiseDomain::SurfaceMeters => {
                                directions.scale(planet_radius / scale)
                            }
                        };
                        noise.range_in_box(self.seed, domain)
                    }
                    TerrainFieldSource::Latitude {
                        amplitude,
                        bias,
                        absolute,
                    } => {
                        let latitude = TerrainValueRange::new(directions.min.y, directions.max.y);
                        let latitude = if *absolute {
                            latitude.absolute()
                        } else {
                            latitude
                        };
                        latitude.scale(*amplitude).offset(*bias)
                    }
                    source => source.range(&channels),
                }
            };
            let value = mask.map_or(source, |mask| source.multiply(mask));
            let target = channels[layer.target.index()];
            let result = match layer.operation {
                TerrainFieldOperation::Add => target.add(value),
                TerrainFieldOperation::Subtract => target.subtract(value),
                TerrainFieldOperation::Multiply => target.multiply(value),
                TerrainFieldOperation::Minimum => target.minimum(value),
                TerrainFieldOperation::Maximum => target.maximum(value),
                TerrainFieldOperation::Replace => value,
            };
            if !result.minimum.is_finite() || !result.maximum.is_finite() {
                return None;
            }
            channels[layer.target.index()] = result.padded();
        }
        // Keep the radius interval separate: a local height bound must not
        // inherit the worst-case radial slope in every spatial direction.
        let result = position
            .radius_range()
            .subtract(
                channels[TerrainFieldChannel::Height.index()]
                    .offset(planet_radius)
                    .padded(),
            )
            .add(channels[TerrainFieldChannel::Density.index()])
            .padded();
        (result.minimum.is_finite() && result.maximum.is_finite()).then_some(result)
    }
}

impl TerrainNoiseNode {
    fn range_in_box(&self, seed: u64, domain: BoxRange) -> TerrainValueRange {
        if self.kind == TerrainNoiseKind::Cellular {
            return self.range();
        }
        let mut warped = domain;
        if self.warp_strength.abs() > f64::EPSILON {
            let q = domain.scale(1.0 / self.warp_scale.max(f64::EPSILON));
            for (axis, salt) in [101, 211, 307].into_iter().enumerate() {
                let displacement =
                    fractal_noise_range(seed, self.seed_offset + salt, q, 3, 2.03, 0.5)
                        .scale(self.warp_strength);
                warped.min[axis] += displacement.minimum;
                warped.max[axis] += displacement.maximum;
            }
            warped = warped.padded();
        }
        let noise = fractal_noise_range(
            seed,
            self.seed_offset,
            warped,
            self.octaves,
            self.lacunarity,
            self.persistence,
        );
        let shaped = match self.kind {
            TerrainNoiseKind::Fbm => noise,
            TerrainNoiseKind::Ridged => noise.absolute().scale(-1.0).offset(1.0),
            TerrainNoiseKind::Billow => noise.absolute().scale(2.0).offset(-1.0),
            TerrainNoiseKind::Cellular => unreachable!(),
        };
        shaped.scale(self.amplitude).offset(self.bias).padded()
    }
}

fn fractal_noise_range(
    seed: u64,
    salt: u64,
    domain: BoxRange,
    octaves: u8,
    lacunarity: f64,
    persistence: f64,
) -> TerrainValueRange {
    let mut center = (domain.min + domain.max) * 0.5;
    let extents = (domain.max - domain.min) * 0.5;
    let mut axes = [DVec3::X, DVec3::Y, DVec3::Z];
    let mut amplitude = 1.0;
    let mut amplitude_sum = 0.0;
    let mut range = TerrainValueRange::ZERO;
    for octave in 0..octaves {
        // Transform the original box, rather than reboxing the previous
        // octave: repeated rotations of an AABB inflate it unnecessarily.
        let half =
            axes[0].abs() * extents.x + axes[1].abs() * extents.y + axes[2].abs() * extents.z;
        let query = BoxRange {
            min: center - half,
            max: center + half,
        }
        .padded();
        range = range
            .add(value_noise_range(seed, salt + u64::from(octave), query).scale(amplitude))
            .padded();
        amplitude_sum += amplitude;
        amplitude *= persistence;
        center = rotate_octave(center) * lacunarity;
        axes = axes.map(|axis| rotate_octave(axis) * lacunarity);
    }
    if amplitude_sum > 0.0 {
        range.scale(1.0 / amplitude_sum).padded()
    } else {
        TerrainValueRange::ZERO
    }
}

fn value_noise_range(seed: u64, salt: u64, query: BoxRange) -> TerrainValueRange {
    let global = TerrainValueRange::new(-1.0, 1.0);
    if !query.min.is_finite()
        || !query.max.is_finite()
        || query.min.min_element() < f64::from(i32::MIN + 1)
        || query.max.max_element() > f64::from(i32::MAX - 2)
    {
        return global;
    }
    let first = query.min.floor().as_ivec3();
    let last = query.max.floor().as_ivec3();
    let counts = last.as_dvec3() - first.as_dvec3() + DVec3::ONE;
    // Bound the classification cost independently of world/node size.
    if counts.element_product() > 8.0 {
        return global;
    }
    let mut result: Option<TerrainValueRange> = None;
    for z in first.z..=last.z {
        for y in first.y..=last.y {
            for x in first.x..=last.x {
                let origin = dvec3(f64::from(x), f64::from(y), f64::from(z));
                let lo = (query.min - origin).clamp(DVec3::ZERO, DVec3::ONE);
                let hi = (query.max - origin).clamp(DVec3::ZERO, DVec3::ONE);
                let lo = dvec3(smootherstep(lo.x), smootherstep(lo.y), smootherstep(lo.z));
                let hi = dvec3(smootherstep(hi.x), smootherstep(hi.y), smootherstep(hi.z));
                let corners: [f64; 8] = std::array::from_fn(|i| {
                    hash01(
                        seed,
                        salt,
                        x + (i & 1) as i32,
                        y + ((i >> 1) & 1) as i32,
                        z + ((i >> 2) & 1) as i32,
                    )
                });
                let mut min = f64::INFINITY;
                let mut max = f64::NEG_INFINITY;
                // Smoothed trilinear noise is multilinear in the three
                // monotone blend weights. Its extrema over their rectangle
                // occur at these endpoints, even across a lattice boundary.
                for i in 0..8 {
                    let t = dvec3(
                        if i & 1 == 0 { lo.x } else { hi.x },
                        if i & 2 == 0 { lo.y } else { hi.y },
                        if i & 4 == 0 { lo.z } else { hi.z },
                    );
                    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
                    let a = lerp(
                        lerp(corners[0], corners[1], t.x),
                        lerp(corners[2], corners[3], t.x),
                        t.y,
                    );
                    let b = lerp(
                        lerp(corners[4], corners[5], t.x),
                        lerp(corners[6], corners[7], t.x),
                        t.y,
                    );
                    let value = lerp(a, b, t.z) * 2.0 - 1.0;
                    min = min.min(value);
                    max = max.max(value);
                }
                let cell = TerrainValueRange::new(min, max).padded();
                result = Some(result.map_or(cell, |range| range.union(cell)));
            }
        }
    }
    result.unwrap_or(global)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_contains(range: TerrainValueRange, value: f64) {
        assert!(
            value >= range.minimum && value <= range.maximum,
            "{value} outside {range:?}"
        );
    }

    #[test]
    fn lattice_bounds_cover_interiors_and_cell_boundaries() {
        for origin in [
            dvec3(0.17, 0.42, 0.8),
            dvec3(-2.01, -0.001, 0.97),
            DVec3::ZERO,
        ] {
            for size in [0.00001, 0.1, 0.8, 8.0] {
                let query = BoxRange {
                    min: origin,
                    max: origin + DVec3::splat(size),
                };
                let range = value_noise_range(41, 13, query);
                for i in 0..128 {
                    let t = dvec3(
                        hash01(1, 0, i, 0, 0),
                        hash01(2, 0, i, 0, 0),
                        hash01(3, 0, i, 0, 0),
                    );
                    assert_contains(range, value_noise(41, 13, origin + t * size));
                }
                assert_contains(range, value_noise(41, 13, query.min));
                assert_contains(range, value_noise(41, 13, query.max));
                if size < 0.001 {
                    assert!(range.maximum - range.minimum < 0.001);
                }
            }
        }
    }

    #[test]
    fn warped_octave_bounds_cover_all_noise_shapes() {
        for kind in TerrainNoiseKind::ALL {
            for warp_strength in [0.0, 0.3, -0.2] {
                for size in [0.0001, 0.07, 0.9, 4.0] {
                    let domain = BoxRange {
                        min: dvec3(-2.05, 1.93, 0.4),
                        max: dvec3(-2.05, 1.93, 0.4) + DVec3::splat(size),
                    };
                    let noise = TerrainNoiseNode {
                        kind,
                        domain: TerrainNoiseDomain::PositionMeters,
                        scale: 1.0,
                        amplitude: -2.0,
                        bias: 0.7,
                        octaves: 6,
                        lacunarity: 2.03,
                        persistence: 0.48,
                        warp_scale: 0.65,
                        warp_strength,
                        seed_offset: 70,
                    };
                    let range = noise.range_in_box(71, domain);
                    for i in 0..64 {
                        let t = dvec3(
                            hash01(4, 0, i, 0, 0),
                            hash01(5, 0, i, 0, 0),
                            hash01(6, 0, i, 0, 0),
                        );
                        let position = domain.min + t * size;
                        assert_contains(
                            range,
                            noise.evaluate(
                                71,
                                TerrainFieldContext {
                                    position,
                                    direction: position.normalize_or_zero(),
                                    radius: 100.0,
                                },
                            ),
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn directional_bounds_cover_boxes_on_all_sides_and_at_origin() {
        for min in [
            dvec3(-1.0, -1.0, -1.0),
            dvec3(-10.0, -2.0, -3.0),
            dvec3(0.0, 10.0, 0.0),
            dvec3(6_371_000.0, 0.0, 0.0),
            dvec3(2_000_000.0, -4_000_000.0, 1_000_000.0),
        ] {
            let position = BoxRange {
                min,
                max: min + DVec3::splat(32.0),
            };
            let directions = position.directions();
            for i in 0..128 {
                let t = dvec3(
                    hash01(7, 0, i, 0, 0),
                    hash01(8, 0, i, 0, 0),
                    hash01(9, 0, i, 0, 0),
                );
                let direction = (min + t * 32.0).normalize_or_zero();
                assert!(
                    direction.cmpge(directions.min).all() && direction.cmple(directions.max).all()
                );
            }
        }
    }

    #[test]
    fn composed_density_bounds_preserve_volumetric_features_and_masks() {
        let mut graph = TerrainFieldGraph::default();
        // Test every graph operation with a genuinely 3D source, including
        // min/max CSG-style operations and a channel-dependent mask.
        for operation in TerrainFieldOperation::ALL {
            for domain in TerrainNoiseDomain::ALL {
                graph.layers.push(TerrainFieldLayer {
                    id: 100,
                    name: "3D feature".into(),
                    enabled: true,
                    target: TerrainFieldChannel::Density,
                    operation,
                    source: TerrainFieldSource::Noise(TerrainNoiseNode {
                        kind: TerrainNoiseKind::Fbm,
                        domain,
                        scale: 50.0,
                        amplitude: 250.0,
                        bias: -20.0,
                        octaves: 4,
                        lacunarity: 2.03,
                        persistence: 0.5,
                        warp_scale: 0.7,
                        warp_strength: 0.2,
                        seed_offset: 20,
                    }),
                    mask: Some(TerrainFieldMask {
                        channel: TerrainFieldChannel::Land,
                        minimum: 0.1,
                        maximum: 0.9,
                        smooth: true,
                        invert: true,
                    }),
                });
                for direction in [DVec3::X, dvec3(0.31, 0.82, -0.48).normalize(), -DVec3::Y] {
                    let min = direction * graph.radius - DVec3::splat(16.0);
                    let range = graph
                        .density_range_in_box(min, min + DVec3::splat(32.0), graph.radius)
                        .unwrap();
                    for i in 0..64 {
                        let p = min
                            + dvec3(
                                hash01(10, 0, i, 0, 0),
                                hash01(11, 0, i, 0, 0),
                                hash01(12, 0, i, 0, 0),
                            ) * 32.0;
                        let sample = graph.evaluate(TerrainFieldContext {
                            position: p,
                            direction: p.normalize_or_zero(),
                            radius: graph.radius,
                        });
                        let density = p.length()
                            - (graph.radius + sample.channels[TerrainFieldChannel::Height.index()])
                            + sample.channels[TerrainFieldChannel::Density.index()];
                        assert_contains(range, density);
                    }
                }
                graph.layers.pop();
            }
        }
    }
}
