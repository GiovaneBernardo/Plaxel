# Aster — Highlands & Hollow Stone

Open `planet_aster.plxterrain` using **Open** in the Terrain Graph editor, select
the intended planet, and click **Apply to planet**. This queues a live rebuild.
The existing presets and startup defaults are unchanged.

Aster is an Earth-sized planet (radius 6,371 km), with 24 layers built entirely
from the existing graph nodes:

- Continental basins and elevated inland shelves.
- Broad alpine provinces, folded mountain ridges, and broken summit relief.
- Rolling lowlands, sandstone-style mesas, escarpments, and incised dry valleys.
- Localized volumetric rock formations: undercuts, cave passages, and arch-like
  openings, made from position-space noise rather than height displacement alone.

Water generation is deferred. Below-sea-level ground still uses the current
engine material-selection behavior. This preset does not implement biomes or
change material rules.

## Exploration

One sampled formation is at **longitude 63.113360°, latitude 57.642651°**.
Its base height is approximately **1,785 m** above the planet radius. The radial
transect here crosses solid/air five times. This is an exploration reference,
not an exact safe camera spawn altitude. Longitude uses `atan2(z, x)` and latitude
uses `asin(y / radius)`, matching the editor.

Inspect the live mesh near the surface. The graph's map/globe preview samples
channels on a reference sphere; it cannot display underground topology. Fine
passages require close terrain LOD. Procedural noise does not guarantee that
every rock formation is connected or every cavity is accessible.

## Tuning

| Layers | Purpose |
| --- | --- |
| 1–3 | Continental distribution and baseline elevation |
| 4–8 | Mountain placement, uplift, ridges, and summits |
| 9–16 | Hills, mesas, valleys, and smaller surface relief |
| 17–19 | Inland regions where volumetric formations are permitted |
| 20–21 | Cliff undercuts and larger cavern folds |
| 22–24 | Intersected noise bands that excavate passages |

To make formations more common, lower the input thresholds of **Karst province
coverage**. To make passages wider, lower the input thresholds of **Carve
passages through buttresses**. Increasing its output amplitude makes excavation
deeper. Large amplitudes also widen the range that terrain bounds must consider.

Keep the layer order: `Plains` and `Rivers` are reused as temporary channels after
their earlier height contributions have been consumed. The position-space
layers deliberately use few octaves and no domain warp, with regional masks
that allow the existing SIMD zero-mask optimization to skip work.

## Validation

The preset inspector can be run with:

```powershell
cargo run -p editor-logic --example inspect_terrain_preset -- planet_aster.plxterrain
```

Final sampled results:

- Graph deserialization and validation passed.
- 1,024 approximately equal-area directions: 505 base heights above sea level.
- Sampled base heights: −2,200 to 6,967 m (sampled extrema, not global bounds).
- 69 radial transects had at least three solid/air crossings at 8 m spacing.
- 114,688 finite density samples agreed between scalar and SIMD evaluation
  within 0.002; periodic spatial-bound checks passed.
- A 1.6 km-wide density cross-section was inspected while tuning the cavities.

The normal editor example build encountered corrupt dependency metadata after
the power outage. Validation therefore ran in an isolated harness using copies
of the current graph, compiled evaluator, and bounds modules, removing only
engine/reflection dependencies. This verifies field behavior, not live meshing,
rendering, cave connectivity, or in-game frame time.
