# Materials, passes, and views

The renderer deliberately separates four concepts:

- A **graph pass** owns attachments and execution order, such as geometry or shadow rendering.
- A **material pass** selects a shader/pipeline variant, such as forward, depth-only, or shadow.
- A **render phase** categorizes surface ordering, such as opaque, water, or transparent.
- A **view** represents a camera-like use of a graph pass, such as main, editor, or a shadow cascade.

Keeping these independent avoids special cases such as “the geometry pass knows how every object
must draw its shadow.”

## Extensible IDs

IDs are stable compile-time hashes, not closed Rust enums. Game code can define its own values:

```rust
use engine::renderer::{GraphPassId, MaterialPassId, RenderPhaseId, RenderViewId};

pub const GALAXY_PHASE: RenderPhaseId = RenderPhaseId::new("game.galaxy");
pub const STAR_MATERIAL_PASS: MaterialPassId =
    MaterialPassId::new("game.star_forward");
pub const REFLECTION_PASS: GraphPassId = GraphPassId::new("game.water_reflection");
pub const REFLECTION_VIEW: RenderViewId = RenderViewId::new("game.water_reflection_view");
```

Use globally namespaced strings. Defining the same string in two crates intentionally produces the
same ID.

## Ordinary multipass materials

`Material::new` creates ordinary surface variants for forward, depth-only, and shadow rendering,
plus the existing debug/fullscreen variants. Builders such as `with_vertex_layouts`, `with_cull`,
and `with_depth` update all variants.

```rust
let material = Material::new("shaders/vehicle.wgsl".into())
    .with_vertex_layouts(vec![VehicleVertex::layout(), TransformInstance::layout()])
    .with_cull(CullMode::Back);
```

Depth-only and shadow variants default to `vs_main` with no fragment entry. Change a specific variant
when the shader uses another entry point:

```rust
material.configure_pass(material_passes::SHADOW, |pass| {
    pass.vertex_entry = "vs_shadow".into();
    pass.fragment_entry = None;
});
```

Use `with_pass_variant` to add a game-defined variant while inheriting the material's existing
pipeline state.

## Specialized materials that opt out

Use `Material::for_pass` for a surface that must participate in exactly one material pass. Water can
therefore omit both depth-prepass and shadow variants explicitly:

```rust
let water = Material::for_pass(
    "shaders/voxel_water.wgsl".into(),
    material_passes::WATER,
)
.with_depth(Some(DepthState {
    write_enabled: false,
    compare: CompareFunction::Greater,
}));
```

The corresponding render object should also opt out through flags:

```rust
let object = RenderObject::new(mesh, water, transform)
    .with_phase(phases::WATER)
    .with_flags(RenderFlags::VISIBLE_MAIN);
```

Material support answers “does a shader variant exist?” while flags answer “should this object
participate?” Both must permit a draw.

## Routes

A producer route connects the four concepts:

```rust
RenderRoute {
    graph_pass: graph_passes::SHADOWS,
    material_pass: material_passes::SHADOW,
    phase: phases::OPAQUE,
    views: RenderViewSelector::ShadowCascades,
}
```

A building-chunk producer can expose several routes without duplicating its chunk data:

```rust
let routes = vec![
    RenderRoute {
        graph_pass: graph_passes::GEOMETRY,
        material_pass: material_passes::FORWARD_OPAQUE,
        phase: phases::OPAQUE,
        views: RenderViewSelector::Main,
    },
    RenderRoute {
        graph_pass: graph_passes::DEPTH_PREPASS,
        material_pass: material_passes::DEPTH_ONLY,
        phase: phases::OPAQUE,
        views: RenderViewSelector::Main,
    },
    RenderRoute {
        graph_pass: graph_passes::SHADOWS,
        material_pass: material_passes::SHADOW,
        phase: phases::OPAQUE,
        views: RenderViewSelector::ShadowCascades,
    },
];
```

Its `record` implementation examines `pass.route.material_pass` and chooses the matching pipeline,
while reusing the same vertex, index, instance, and indirect buffers.

## Register views

The main geometry view is registered by the renderer. A pass that introduces other views must add
them to the view registry:

```rust
const CASCADE_VIEW_IDS: [RenderViewId; 4] = [
    RenderViewId::new("game.shadow_cascade.0"),
    RenderViewId::new("game.shadow_cascade.1"),
    RenderViewId::new("game.shadow_cascade.2"),
    RenderViewId::new("game.shadow_cascade.3"),
];

renderer.views().set_views(
    graph_passes::SHADOWS,
    cascades
        .iter()
        .enumerate()
        .map(|(index, cascade)| RenderView {
            id: CASCADE_VIEW_IDS[index],
            kind: RenderViewKind::ShadowCascade {
                cascade: index as u32,
            },
            view_bind_group: Some(cascade.bind_group),
        })
        .collect(),
);
```

Prefer fixed ID constants rather than creating formatted names every frame.

## Adding a graph pass

Implement `RenderNode` when a feature needs new attachments or transforms graph resources. The node
describes its inputs/outputs, creates pass-local resources in `compile`, updates transient state in
`prepare`, and records pass-owned work in `run`.

Register the node under a stable `GraphPassId` and compile the graph after changing its structure.
Geometry producers whose `RenderRoute::graph_pass` matches that ID are invoked after the node's
`run` method inside the active render pass.

Current status: geometry routing and a single directional shadow cascade are active. Retained opaque
objects with `CASTS_SHADOWS` and a shadow material variant are routed through the depth-only shadow
pass. The shadow texture is currently one 2048x2048 `D2` texture; array layers and multiple cascade
views have not been implemented yet. The depth-prepass IDs and participation flags exist, but its
graph node is still intentionally absent.

## Common mistakes

- A material pass does not create a graph node or attachments.
- A render flag does not create a missing shader variant.
- A route to an unregistered graph pass never records.
- A graph pass with no registered matching views does not invoke routed producers.
- Do not duplicate one producer per shadow cascade unless their GPU state is genuinely different;
  normally one producer exposes a shadow route and receives each cascade view.
