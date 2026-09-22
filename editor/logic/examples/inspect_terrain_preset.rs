//! Run with: cargo run -p editor-logic --example inspect_terrain_preset -- planet_aster.plxterrain
use engine::math::DVec3;
use game_types::terrain::terrain_field::{
    TerrainFieldChannel, TerrainFieldContext, TerrainFieldGraph,
};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("expected a .plxterrain path");
    let graph: TerrainFieldGraph = ron::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(graph.validate().is_empty(), "{:?}", graph.validate());
    let compiled = graph.compile_density();
    let mut min_height = f64::INFINITY;
    let mut max_height = f64::NEG_INFINITY;
    let mut land = 0;
    let mut multilayer = 0;
    let mut checked = 0;
    let mut first_site = None;
    let start = std::time::Instant::now();
    // Equal-area spherical sampling; each radial transect spans all possible
    // density displacement, at 8 m spacing, independently of mesh LOD.
    let density_range = graph.channel_range(TerrainFieldChannel::Density);
    for i in 0..1024 {
        let y = 1.0 - 2.0 * (i as f64 + 0.5) / 1024.0;
        let angle = i as f64 * 2.399963229728653;
        let r = (1.0 - y * y).sqrt();
        let direction = DVec3::new(r * angle.cos(), y, r * angle.sin());
        let height =
            graph.evaluate_direction(direction).channels[TerrainFieldChannel::Height.index()];
        min_height = min_height.min(height);
        max_height = max_height.max(height);
        land += usize::from(height > graph.sea_level);
        let low = height - density_range.maximum - 16.0;
        let high = height - density_range.minimum + 16.0;
        let count = ((high - low) / 8.0).ceil() as usize + 1;
        let positions: Vec<_> = (0..count)
            .map(|j| direction * (graph.radius + low + j as f64 * 8.0))
            .collect();
        let mut values = vec![0.0; count];
        compiled.densities(&positions, graph.radius, &mut values);
        let mut crossings = 0;
        for (j, (&p, &value)) in positions.iter().zip(&values).enumerate() {
            let sample = graph.evaluate(TerrainFieldContext {
                position: p,
                direction,
                radius: graph.radius,
            });
            let expected =
                (p.length() - graph.radius - sample.channels[TerrainFieldChannel::Height.index()]
                    + sample.channels[TerrainFieldChannel::Density.index()]) as f32;
            assert!(
                (value - expected).abs() < 0.002,
                "compiled mismatch at {p:?}"
            );
            assert!(value.is_finite());
            if j > 0 && (values[j - 1] < 0.0) != (value < 0.0) {
                crossings += 1;
            }
            if j % 32 == 0 {
                let bound = graph
                    .density_range_in_box(
                        p - DVec3::splat(4.0),
                        p + DVec3::splat(4.0),
                        graph.radius,
                    )
                    .unwrap();
                assert!(
                    f64::from(value) >= bound.minimum - 0.002
                        && f64::from(value) <= bound.maximum + 0.002
                );
            }
            checked += 1;
        }
        assert!(values[0] < 0.0 && values[count - 1] > 0.0);
        if crossings >= 3 {
            multilayer += 1;
            if first_site.is_none_or(|(_, _, previous)| crossings > previous) {
                first_site = Some((direction, height, crossings));
            }
        }
    }
    println!(
        "{}: {} layers; validation passed",
        graph.name,
        graph.layers.len()
    );
    println!("Sampled base height {min_height:.1}..{max_height:.1} m; land {land}/1024");
    println!("Multiple solid/air crossings: {multilayer}/1024 radial transects");
    println!(
        "Verified {checked} scalar/SIMD samples and periodic box bounds in {:?}",
        start.elapsed()
    );
    if let Some((direction, height, crossings)) = first_site {
        println!(
            "Example formation: longitude {:.6}, latitude {:.6}, base altitude {height:.1} m, {crossings} crossings",
            direction.z.atan2(direction.x).to_degrees(),
            direction.y.asin().to_degrees()
        );
    }
}
