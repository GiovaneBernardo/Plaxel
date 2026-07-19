# Rendering world geometry

The renderer supports three levels of control. Start with ordinary mesh components, use retained
objects for generated conventional geometry, and use a producer when the workload needs to own its
GPU representation.

## 1. Ordinary models with `MeshRendererComponent`

Use this for loaded models, props, scale references, and other conventional entities.

```rust
let entity = world.spawn();
world.insert(entity, TransformComponent {
    position,
    rotation: engine::math::Quat::IDENTITY,
    scale: engine::math::Vec3::ONE,
    velocity: engine::math::Vec3::ZERO,
});
world.insert(entity, MeshRendererComponent {
    mesh,
    material: material.uuid,
});
```

The renderer observes component change ticks and maintains a retained `RenderObject`. Static
entities are not re-extracted or re-uploaded every frame. A transform mutation generates a ranged
GPU transform-buffer update; mesh, material, phase, or binding changes cause a structural rebuild.

## 2. Generated conventional geometry with `RenderObject`

Use this for voxel/building chunks, procedural planet chunks, or any generated mesh that still maps
cleanly to one or more conventional indexed draws.

```rust
use engine::renderer::{RenderObject, RenderObjectId};

let transform = engine::model::TransformInstance {
    model_matrix: model_matrix.to_cols_array_2d(),
    material_index: material.material_index,
};

let object_id: RenderObjectId = renderer.objects().insert(
    RenderObject::new(mesh, material, transform)
        .with_bind_group(2, chunk_materials_bind_group),
);
```

Keep the returned `RenderObjectId` in the owning game system. It is generational, so an ID cannot
accidentally address a later object that reused the same slot.

Update only the transform:

```rust
renderer.objects().update_transform(object_id, new_transform);
```

Replace structural data:

```rust
renderer.objects().update(object_id, replacement_object);
```

Remove it when the chunk is destroyed or replaced:

```rust
renderer.objects().remove(object_id);
```

Do not insert the same generated mesh again every frame. Retain the ID and mutate only when the
simulation data changes.

## 3. Custom `RenderProducer`

Use a producer when a workload needs specialized batching, instancing, indirect commands, its own
GPU buffers, or a representation that does not map to one render object per logical entity.

A producer has three responsibilities:

1. Declare the graph pass, material pass, phase, and views it participates in.
2. Create/update its GPU-owned state in `prepare_frame` or `prepare_views`.
3. Record commands in `record` when one of its routes is active.

```rust
use std::any::Any;
use engine::renderer::*;

pub const STAR_PRODUCER: RenderProducerId =
    RenderProducerId::new("game.star_producer");

pub struct StarProducer {
    routes: Vec<RenderRoute>,
    pipeline: PipelineHandle,
    star_buffer: BufferHandle,
    indirect_buffer: BufferHandle,
    bind_group: BindGroupHandle,
}

impl StarProducer {
    pub fn new(
        pipeline: PipelineHandle,
        star_buffer: BufferHandle,
        indirect_buffer: BufferHandle,
        bind_group: BindGroupHandle,
    ) -> Self {
        Self {
            routes: vec![RenderRoute {
                graph_pass: graph_passes::GEOMETRY,
                material_pass: material_passes::FORWARD_OPAQUE,
                phase: phases::OPAQUE,
                views: RenderViewSelector::Main,
            }],
            pipeline,
            star_buffer,
            indirect_buffer,
            bind_group,
        }
    }
}

impl RenderProducer for StarProducer {
    fn id(&self) -> RenderProducerId {
        STAR_PRODUCER
    }

    fn routes(&self) -> &[RenderRoute] {
        &self.routes
    }

    fn prepare_frame(&mut self, _ctx: &mut ProducerPrepareContext<'_>) {
        // Upload only dirty ranges, resize buffers when capacity is exceeded,
        // or update indirect arguments. Static star data needs no work here.
    }

    fn record(
        &self,
        ctx: &mut dyn RenderContext,
        _resources: &RenderResources,
        pass: &RenderPassContext<'_>,
    ) {
        ctx.bind_pipeline(self.pipeline);
        if let Some(view_bind_group) = pass.view.view_bind_group {
            ctx.bind_bind_group(0, view_bind_group);
        }
        ctx.bind_bind_group(1, self.bind_group);
        ctx.bind_vertex_buffer(0, self.star_buffer);

        PreparedDraw::Indirect {
            buffer: self.indirect_buffer,
            offset: 0,
        }
        .record(ctx);
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
```

Register it once after its persistent pipeline/bindings have been created:

```rust
state
    .global_resources
    .renderer
    .register_producer(star_producer)
    .expect("star producer should only be registered once");
```

Update it from game code without searching through scene mesh components:

```rust
let producer = ctx
    .globals
    .renderer
    .producer_mut::<StarProducer>(STAR_PRODUCER)
    .expect("star producer must be registered");

producer.apply_generated_galaxy(changed_stars);
```

The producer owns the policy behind `apply_generated_galaxy`: it may replace a buffer, write dirty
ranges, append instances, or update only indirect arguments.

### Indirect argument buffers

Create indirect buffers with `BufferUsages::INDIRECT`. Add `COPY_DST` when the CPU writes commands,
and `STORAGE` when a future compute pass writes them.

```rust
let indirect_buffer = renderer.renderer_api.create_buffer(&BufferDescriptor {
    label: "star_indirect".into(),
    size: std::mem::size_of::<[u32; 4]>() as u64,
    usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST | BufferUsages::STORAGE,
});
```

The non-indexed indirect layout is four `u32` values: vertex count, instance count, first vertex,
and first instance. Indexed indirect drawing uses index count, instance count, first index, signed
base vertex, and first instance.

### Multiple drawing techniques in one graph pass

Different producers can route to the same graph pass. One may issue instanced direct draws, another
may issue indexed-indirect draws, and the standard producer may draw retained meshes. A producer can
also choose between command types internally. The geometry graph node owns the attachment; producers
own how their geometry is submitted.

Mesh-shader support will require corresponding backend and `RenderContext` commands, but it does not
require changing retained objects, material IDs, producer registration, or graph routing.

## Performance checklist

- Create pipelines, layouts, bind groups, and long-lived buffers outside the hot draw loop.
- Store capacity separately from logical length and grow geometrically.
- Upload dirty ranges with `write_buffer_at`; do not recreate static buffers.
- Keep CPU sorting/batching results until a structural revision changes.
- Prefer one producer for a large homogeneous workload over millions of render objects.
- Keep logical simulation entities independent from GPU batching layout.
- Use `RenderObject` when custom GPU ownership would add complexity without reducing work.

