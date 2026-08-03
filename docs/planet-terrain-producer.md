# Implementing `PlanetTerrainProducer`

This guide is the target design for replacing the terrain rendering currently mixed into
`planet_system.rs`. It is intentionally close to an implementation, while keeping simulation,
mesh generation, and rendering ownership separate.

The resulting responsibilities are:

| Owner | Responsibility |
| --- | --- |
| Planet ECS systems | Decide which chunks should exist and schedule mesh generation. |
| Mesh workers | Produce `Vec<PlanetVertex>` and `Vec<u32>` only. |
| `PlanetTerrainProducer` | Upload/remove meshes, own terrain GPU resources, build batches, and record draws. |
| Render graph nodes | Bind frame/view resources and create the geometry and shadow passes. |
| Renderer backend | Allocate GPU resources and translate draw commands to wgpu. |

`planet_system.rs` should not know a material bind group, pipeline handle, vertex/index buffer,
or indirect buffer. It should submit terrain changes through a small queue resource.

## The indirect-drawing decision

Compute shaders are not required for indirect drawing.

The CPU can create a buffer with `BufferUsages::INDIRECT | BufferUsages::COPY_DST`, write
`DrawIndexedIndirectArgs` into it, and submit those commands with
`multi_draw_indexed_indirect`. A compute shader is useful only when the GPU itself must produce
or compact commands, for example for GPU culling. It is not a prerequisite.

Each indexed indirect command has this layout:

```rust
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DrawIndexedIndirectArgs {
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
}
```

Use `instance_count: 1` and `first_instance: 0`. That field being present does not mean terrain
is using instancing; it is simply part of the WebGPU indirect command format.

One warning matters: repeatedly calling the existing `draw_indexed_indirect` in a loop still
creates one render-pass API call per chunk. To submit many commands with one call, expose wgpu's
`multi_draw_indexed_indirect` through the engine. The CPU already knows the command count, so the
count-buffer variant and the `MULTI_DRAW_INDIRECT_COUNT` feature are not needed.

## Small engine prerequisites

The current renderer exposes only a single indexed-indirect draw. Add this method to
`RenderContext`:

```rust
fn multi_draw_indexed_indirect(
    &mut self,
    buffer: BufferHandle,
    offset: u64,
    count: u32,
);
```

The wgpu backend implementation is:

```rust
fn multi_draw_indexed_indirect(
    &mut self,
    buffer: BufferHandle,
    offset: u64,
    count: u32,
) {
    self.pass.multi_draw_indexed_indirect(
        self.backend.get_buffer(buffer).unwrap(),
        offset,
        count,
    );
}
```

This only requires normal indirect-execution support. wgpu may emulate multi-draw on hardware
without native support, but the producer API and data layout remain correct.

The second useful engine addition is explicit buffer destruction:

```rust
fn remove_buffer(&mut self, buffer: BufferHandle) -> bool;
```

The producer needs it when an indirect buffer grows and when the producer is removed. Until that
exists, allocate a conservative fixed-capacity indirect buffer. Replacing buffers without
removing the old handles is not production-safe.

## Recommended files

Keep the feature in a small module instead of growing `planet_system.rs`:

```text
game/logic/src/render/producers/
├── mod.rs
└── planet_terrain_producer.rs
```

Initially, one producer file is enough. Split command types or batching into sibling modules only
after the file becomes difficult to navigate.

## Public command boundary

The ECS-facing API should contain no GPU types. A nested map also makes chunk identity include its
planet; a bare `HashMap<NodeKey, _>` can collide when two planets use the same octree key.

```rust
use crossbeam_channel::Sender;
use engine::ecs::entity::Entity;
use game_types::{octree::NodeKey, planet::PlanetVertex};

pub struct PendingTerrainChunk {
    pub key: NodeKey,
    pub vertices: Vec<PlanetVertex>,
    pub indices: Vec<u32>,
}

pub enum PlanetTerrainCommand {
    EnsurePlanet {
        planet: Entity,
        frame: GpuTerrainFrame,
    },
    UpdatePlanetFrame {
        planet: Entity,
        frame: GpuTerrainFrame,
    },
    ReplaceChunks {
        planet: Entity,
        remove: Vec<NodeKey>,
        insert: Vec<PendingTerrainChunk>,
    },
    RemovePlanet {
        planet: Entity,
    },
}

#[derive(Clone)]
pub struct PlanetTerrainRenderQueue {
    sender: Sender<PlanetTerrainCommand>,
}

impl PlanetTerrainRenderQueue {
    pub fn send(
        &self,
        command: PlanetTerrainCommand,
    ) -> Result<(), crossbeam_channel::SendError<PlanetTerrainCommand>> {
        self.sender.send(command)
    }
}
```

`PlanetTerrainRenderQueue` automatically satisfies the engine's blanket `Resource`
implementation because it is `'static + Send + Sync`. It can also be cloned into mesh jobs if a
worker should send completed CPU meshes directly. Prefer sending from the main-thread result
system at first because it keeps LOD/version validation in one place.

`ReplaceChunks` is the primary operation. It represents an LOD transition as one transaction so
the producer can upload all replacement meshes before removing the visible old set.

For production error reporting, add a second channel from the producer back to gameplay:

```rust
pub enum PlanetTerrainEvent {
    ReplacementApplied { planet: Entity },
    ReplacementFailed { planet: Entity, reason: String },
}

pub struct PlanetTerrainEvents {
    receiver: crossbeam_channel::Receiver<PlanetTerrainEvent>,
}

impl PlanetTerrainEvents {
    pub fn try_iter(&self) -> crossbeam_channel::TryIter<'_, PlanetTerrainEvent> {
        self.receiver.try_iter()
    }
}
```

This lets gameplay complete an LOD transition only after upload succeeds. It is better than
silently considering a failed upload complete.

## Producer-owned state

The producer should own every terrain-specific rendering object:

```rust
use std::{any::Any, collections::HashMap};

use crossbeam_channel::{Receiver, Sender};
use engine::{
    assets::material::Material,
    ecs::entity::Entity,
    renderer::{
        BindGroupHandle, BindGroupLayoutHandle, BufferHandle, GpuMeshHandle,
        PipelineHandle, RenderProducerId, RenderRoute,
    },
};
use game_types::octree::NodeKey;

pub const PLANET_TERRAIN_PRODUCER: RenderProducerId =
    RenderProducerId::new("game.planet_terrain_producer");

pub struct PlanetTerrainProducer {
    routes: Vec<RenderRoute>,
    commands: Receiver<PlanetTerrainCommand>,
    events: Sender<PlanetTerrainEvent>,
    material: Material,
    pipelines: PlanetPipelines,
    terrain_layout: BindGroupLayoutHandle,
    material_palette: BufferHandle,
    planets: HashMap<Entity, PlanetGpuState>,
    indirect: IndirectBuffer,
    forward_batches: Vec<PreparedTerrainBatch>,
    shadow_batches: Vec<PreparedTerrainBatch>,
    batches_dirty: bool,
}

struct PlanetPipelines {
    forward: PipelineHandle,
    shadow: PipelineHandle,
}

struct PlanetGpuState {
    frame_buffer: BufferHandle,
    bind_group: BindGroupHandle,
    chunks: HashMap<NodeKey, ChunkGpuState>,
}

struct ChunkGpuState {
    mesh: GpuMeshHandle,
}

struct IndirectBuffer {
    buffer: BufferHandle,
    capacity: u32,
}

#[derive(Clone, Copy)]
struct PreparedTerrainBatch {
    planet: Entity,
    bind_group: BindGroupHandle,
    vertex_buffer: BufferHandle,
    index_buffer: BufferHandle,
    indirect_offset: u64,
    draw_count: u32,
}
```

The forward and shadow batch vectors can contain the same geometry ranges. Keeping separate
vectors allows their culling policy to diverge later without changing the public API.

Do not store terrain GPU state in `GameState`. `GameState` may temporarily retain simulation and
job bookkeeping during the refactor, but these fields belong in the producer:

- terrain material and pipelines;
- material palette buffer and layout;
- per-planet uniform buffers and bind groups;
- chunk `GpuMeshHandle`s;
- indirect commands and prepared batches.

## Per-planet frame data and shader layout

Keep one uniform buffer and one group-2 bind group per planet. This is the natural boundary because
all chunks of a planet share camera-relative frame data.

```rust
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuTerrainFrame {
    pub view_projection_rotation: [[f32; 4]; 4],
    pub camera_anchor_planet: [i32; 3],
    pub position_unit: f32,
    pub camera_remainder_planet: [f32; 3],
    pub _padding: f32,
    pub planet_world_position: [f32; 3],
    pub _planet_padding: f32,
}
```

The corresponding WGSL declarations are:

```wgsl
struct GpuTerrainFrame {
    view_projection_rotation: mat4x4<f32>,
    camera_anchor_planet: vec3<i32>,
    position_unit: f32,
    camera_remainder_planet: vec3<f32>,
    _padding: f32,
    planet_world_position: vec3<f32>,
    _planet_padding: f32,
};

@group(2) @binding(0)
var<storage, read> terrain_materials: array<GpuPlanetTerrainMaterial>;

@group(2) @binding(1)
var<uniform> terrain_frame: GpuTerrainFrame;
```

The current terrain shader does not consume `terrain_frame` yet, so update it before removing the
old path. Both forward and shadow vertex entry points must use the same camera-relative position
reconstruction. Otherwise the terrain and its shadow will disagree at large coordinates.

The terrain pipeline only needs `PlanetVertex::layout()` if transforms come from the group-2
uniform. Remove `PlanetInstance::layout()` rather than creating a dummy instance vertex buffer.

## Creating the material, layout, buffers, and pipelines

Use a constructor that receives the initialized `Renderer`. Pipeline target formats and shared
layouts exist only after the geometry and shadow graph nodes have compiled, which is why creation
belongs in an init system rather than a `Default` implementation.

```rust
impl PlanetTerrainProducer {
    fn create(
        renderer: &mut engine::renderer::Renderer,
        commands: Receiver<PlanetTerrainCommand>,
        events: Sender<PlanetTerrainEvent>,
    ) -> Self {
        use engine::{
            assets::material::Material,
            model::Vertex,
            renderer::*,
        };
        use game_types::planet::PlanetVertex;

        let mut material = Material::new("shaders/planet_terrain.wgsl".into())
            .with_vertex_layouts(vec![PlanetVertex::layout()])
            .with_cull(CullMode::Back);

        material.configure_pass(material_passes::SHADOW, |pass| {
            pass.pipeline.shader = "shaders/shadow_depth.wgsl".into();
            pass.vertex_entry = "vs_shadow".into();
            pass.fragment_entry = None;
            pass.pipeline.cull_mode = CullMode::None;
            pass.pipeline.depth_state = Some(DepthState {
                write_enabled: true,
                compare: CompareFunction::Less,
            });
        });

        let camera_layout = renderer
            .render_graph
            .get_node_mut::<GeometryPassNode>(graph_passes::GEOMETRY)
            .and_then(|node| node.camera_bind_group_layout)
            .expect("geometry pass must be compiled before terrain initialization");

        let frame = renderer
            .render_resources
            .get_labeled::<FrameBindings>("frame_bindings")
            .expect("frame bindings must exist before terrain initialization");
        let textures_layout = frame.textures_layout;

        let shadow = *renderer
            .render_resources
            .get_labeled::<ShadowBindings>("shadow_bindings")
            .expect("shadow bindings must exist before terrain initialization");

        let terrain_layout = renderer.renderer_api.create_bind_group_layout(
            &BindGroupLayoutDescriptor {
                label: "planet_terrain_layout".into(),
                entries: vec![
                    BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::Fragment,
                        entry_type: BindingType::StorageBuffer { read_only: true },
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ShaderStages::Vertex,
                        entry_type: BindingType::UniformBuffer,
                        count: None,
                    },
                ],
            },
        );

        let palette = create_terrain_palette();
        let material_palette = renderer.renderer_api.create_buffer(&BufferDescriptor {
            label: "planet_terrain_palette".into(),
            size: std::mem::size_of_val(&palette) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        });
        renderer
            .renderer_api
            .write_buffer(material_palette, bytemuck::cast_slice(&palette));

        let geometry_target = renderer.renderer_api.target_info_for_pass(
            &GeometryPassNode::pass_descriptor(),
            &renderer.render_graph.resources,
        );
        let forward = renderer.renderer_api.create_pipeline(
            &material,
            material_passes::FORWARD_OPAQUE,
            &[
                camera_layout,
                textures_layout,
                terrain_layout,
                shadow.sampling_layout,
            ],
            &geometry_target,
        );

        let shadow_target = renderer.renderer_api.target_info_for_pass(
            &ShadowPassNode::pass_descriptor(),
            &renderer.render_graph.resources,
        );
        let shadow_pipeline = renderer.renderer_api.create_pipeline(
            &material,
            material_passes::SHADOW,
            &[shadow.view_layout, textures_layout, terrain_layout],
            &shadow_target,
        );

        let indirect_capacity = 4_096;
        let indirect_buffer = renderer.renderer_api.create_buffer(&BufferDescriptor {
            label: "planet_terrain_indirect".into(),
            size: indirect_capacity as u64
                * std::mem::size_of::<DrawIndexedIndirectArgs>() as u64,
            usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST,
        });

        Self {
            routes: vec![
                RenderRoute {
                    graph_pass: graph_passes::GEOMETRY,
                    material_pass: material_passes::FORWARD_OPAQUE,
                    phase: phases::OPAQUE,
                    views: RenderViewSelector::Main,
                },
                RenderRoute {
                    graph_pass: graph_passes::SHADOWS,
                    material_pass: material_passes::SHADOW,
                    phase: phases::OPAQUE,
                    views: RenderViewSelector::ShadowCascades,
                },
            ],
            commands,
            events,
            material,
            pipelines: PlanetPipelines {
                forward,
                shadow: shadow_pipeline,
            },
            terrain_layout,
            material_palette,
            planets: HashMap::new(),
            indirect: IndirectBuffer {
                buffer: indirect_buffer,
                capacity: indirect_capacity,
            },
            forward_batches: Vec::new(),
            shadow_batches: Vec::new(),
            batches_dirty: false,
        }
    }
}
```

`create_terrain_palette()` should return the existing array of
`GpuPlanetTerrainMaterial`. Texture loading should also move beside this initialization, or into a
general texture asset loader. It should not remain in planet LOD logic.

## Registering the producer from an init system

Yes, use a system to initialize it. The system is orchestration: it creates channels, inserts the
ECS-facing queue, constructs the GPU-facing producer, and registers it once.

```rust
pub fn planet_terrain_producer_init(
    ctx: &mut SystemContext,
    _commands: &mut Commands,
) {
    if ctx
        .globals
        .renderer
        .producer_mut::<PlanetTerrainProducer>(PLANET_TERRAIN_PRODUCER)
        .is_some()
    {
        return;
    }

    let (command_sender, command_receiver) = crossbeam_channel::unbounded();
    let (event_sender, event_receiver) = crossbeam_channel::unbounded();

    let producer = PlanetTerrainProducer::create(
        &mut ctx.globals.renderer,
        command_receiver,
        event_sender,
    );

    ctx.world.insert_resource(PlanetTerrainRenderQueue {
        sender: command_sender,
    });
    ctx.world.insert_resource(PlanetTerrainEvents {
        receiver: event_receiver,
    });

    ctx.globals
        .renderer
        .register_producer(producer)
        .expect("planet terrain producer must only be registered once");
}
```

Register this system before any init system that can submit terrain:

```rust
let init = scene.init_schedule_mut();
init.add_system(planet_terrain_producer_init);
init.add_system(hot_planet_system_init);
init.add_system(universe_system_init);
```

The current registration order puts the terrain producer after planet initialization. Reverse that
order when the planet init path starts using `PlanetTerrainRenderQueue`.

## Creating and updating per-planet resources

The producer handles `EnsurePlanet` during `prepare_frame`:

```rust
fn ensure_planet(
    &mut self,
    api: &mut dyn RendererAPI,
    planet: Entity,
    frame: GpuTerrainFrame,
) {
    if let Some(state) = self.planets.get(&planet) {
        api.write_buffer(state.frame_buffer, bytemuck::bytes_of(&frame));
        return;
    }

    let frame_buffer = api.create_buffer(&BufferDescriptor {
        label: format!("planet_terrain_frame_{planet:?}"),
        size: std::mem::size_of::<GpuTerrainFrame>() as u64,
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    });
    api.write_buffer(frame_buffer, bytemuck::bytes_of(&frame));

    let bind_group = api.create_bind_group(&BindGroupDescriptor {
        label: format!("planet_terrain_bindings_{planet:?}"),
        layout: self.terrain_layout,
        entries: vec![
            (0, BindGroupEntry::Buffer(self.material_palette)),
            (1, BindGroupEntry::Buffer(frame_buffer)),
        ],
    });

    self.planets.insert(
        planet,
        PlanetGpuState {
            frame_buffer,
            bind_group,
            chunks: HashMap::new(),
        },
    );
}
```

`UpdatePlanetFrame` only calls `write_buffer`. Do it every frame if camera-relative data changes
every frame; it is a tiny uniform upload. Do not rebuild bind groups or pipelines.

## Uploading and atomically replacing chunks

The upload path uses the typed pooled-mesh API directly:

```rust
fn upload_chunk(
    api: &mut dyn RendererAPI,
    chunk: &PendingTerrainChunk,
) -> Result<GpuMeshHandle, MeshUploadError> {
    api.upload_mesh(MeshUpload {
        label: "planet_terrain_chunk",
        vertices: bytemuck::cast_slice(&chunk.vertices),
        indices: &chunk.indices,
        vertex_layout: &PlanetVertex::layout(),
    })
}
```

An atomic replacement should follow this order:

1. Validate that the planet exists and discard stale job results before sending the command.
2. Upload every non-empty inserted chunk into a temporary vector.
3. If any upload fails, remove every temporary `GpuMeshHandle` and keep the old visible chunks.
4. On success, remove old handles for both `remove` keys and replaced keys.
5. Insert the new handles and mark batches dirty.
6. Emit `ReplacementApplied`.

```rust
fn replace_chunks(
    &mut self,
    api: &mut dyn RendererAPI,
    planet: Entity,
    remove: Vec<NodeKey>,
    insert: Vec<PendingTerrainChunk>,
) {
    if !self.planets.contains_key(&planet) {
        let _ = self.events.send(PlanetTerrainEvent::ReplacementFailed {
            planet,
            reason: "planet was not initialized".into(),
        });
        return;
    }

    let mut uploaded = Vec::with_capacity(insert.len());
    for chunk in &insert {
        if chunk.indices.is_empty() {
            continue;
        }
        match upload_chunk(api, chunk) {
            Ok(mesh) => uploaded.push((chunk.key, mesh)),
            Err(error) => {
                for (_, mesh) in uploaded {
                    api.remove_mesh(mesh);
                }
                let _ = self.events.send(PlanetTerrainEvent::ReplacementFailed {
                    planet,
                    reason: error.to_string(),
                });
                return;
            }
        }
    }

    let state = self.planets.get_mut(&planet).unwrap();
    let mut keys = remove;
    keys.extend(insert.iter().map(|chunk| chunk.key));
    keys.sort_unstable();
    keys.dedup();

    for key in keys {
        if let Some(old) = state.chunks.remove(&key) {
            api.remove_mesh(old.mesh);
        }
    }
    for (key, mesh) in uploaded {
        state.chunks.insert(key, ChunkGpuState { mesh });
    }

    self.batches_dirty = true;
    let _ = self
        .events
        .send(PlanetTerrainEvent::ReplacementApplied { planet });
}
```

An empty generated mesh still participates in replacement: its key removes the previous visible
mesh. This preserves the behavior needed when terrain edits make a formerly solid chunk empty.

Apply an upload-time budget in `prepare_frame` if large transitions cause frame spikes. Preserve
the atomic transaction: defer the whole replacement or stage all of its uploads; never reveal
half of an LOD replacement.

## Preparing the frame

`prepare_frame` is the only place that mutates producer GPU state. Drain commands, update buffers,
then rebuild the command stream only when topology or visibility changed.

```rust
impl RenderProducer for PlanetTerrainProducer {
    fn id(&self) -> RenderProducerId {
        PLANET_TERRAIN_PRODUCER
    }

    fn routes(&self) -> &[RenderRoute] {
        &self.routes
    }

    fn prepare_frame(&mut self, ctx: &mut ProducerPrepareContext<'_>) {
        while let Ok(command) = self.commands.try_recv() {
            match command {
                PlanetTerrainCommand::EnsurePlanet { planet, frame } => {
                    self.ensure_planet(ctx.api, planet, frame);
                }
                PlanetTerrainCommand::UpdatePlanetFrame { planet, frame } => {
                    if let Some(state) = self.planets.get(&planet) {
                        ctx.api.write_buffer(
                            state.frame_buffer,
                            bytemuck::bytes_of(&frame),
                        );
                    }
                }
                PlanetTerrainCommand::ReplaceChunks {
                    planet,
                    remove,
                    insert,
                } => self.replace_chunks(ctx.api, planet, remove, insert),
                PlanetTerrainCommand::RemovePlanet { planet } => {
                    self.remove_planet(ctx.api, planet);
                }
            }
        }

        if self.batches_dirty {
            self.rebuild_batches(ctx.api);
            self.batches_dirty = false;
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
```

The renderer already calls producer preparation before recording the render graph, so command
buffers written here are ready for both geometry and shadow passes in the same frame.

## Why batches must follow pooled GPU buffers

A single multi-draw call shares its currently bound:

- pipeline;
- bind groups;
- vertex buffer;
- index buffer.

`upload_mesh` suballocates from pooled vertex and index pages. Two `GpuMeshHandle`s may therefore
resolve to different buffer handles. Commands cannot be combined merely because both are terrain.

The batch key should be:

```rust
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct TerrainBatchKey {
    planet: Entity,
    vertex_buffer: BufferHandle,
    index_buffer: BufferHandle,
}
```

The pipeline is selected per route, so it does not need to be stored in this key yet. The planet
is present because each planet has a different group-2 bind group.

This means the number of render calls becomes approximately:

```text
number of planets × number of occupied vertex/index page pairs × number of passes/views
```

rather than `number of chunks × number of passes/views`.

## Building the CPU-written indirect stream

Resolve every visible mesh with `get_gpu_mesh_binding`, group it by `TerrainBatchKey`, then place
each group's commands contiguously in one indirect buffer.

```rust
fn indirect_args(binding: GpuMeshBinding) -> DrawIndexedIndirectArgs {
    DrawIndexedIndirectArgs {
        index_count: binding.draw_range.index_count,
        instance_count: 1,
        first_index: binding.draw_range.first_index,
        base_vertex: binding.draw_range.base_vertex,
        first_instance: 0,
    }
}
```

Conceptually, `rebuild_batches` is:

```rust
fn rebuild_batches(&mut self, api: &mut dyn RendererAPI) {
    let mut grouped = HashMap::<TerrainBatchKey, Vec<DrawIndexedIndirectArgs>>::new();

    for (&planet, state) in &self.planets {
        for chunk in state.chunks.values() {
            let Some(binding) = api.get_gpu_mesh_binding(chunk.mesh) else {
                continue;
            };
            grouped
                .entry(TerrainBatchKey {
                    planet,
                    vertex_buffer: binding.vertex_buffer,
                    index_buffer: binding.index_buffer,
                })
                .or_default()
                .push(indirect_args(binding));
        }
    }

    let command_count: usize = grouped.values().map(Vec::len).sum();
    self.ensure_indirect_capacity(api, command_count as u32);

    let stride = std::mem::size_of::<DrawIndexedIndirectArgs>() as u64;
    let mut commands = Vec::with_capacity(command_count);
    let mut batches = Vec::with_capacity(grouped.len());

    for (key, draws) in grouped {
        let first_command = commands.len() as u64;
        let draw_count = draws.len() as u32;
        commands.extend(draws);

        let state = &self.planets[&key.planet];
        batches.push(PreparedTerrainBatch {
            planet: key.planet,
            bind_group: state.bind_group,
            vertex_buffer: key.vertex_buffer,
            index_buffer: key.index_buffer,
            indirect_offset: first_command * stride,
            draw_count,
        });
    }

    if !commands.is_empty() {
        api.write_buffer(self.indirect.buffer, bytemuck::cast_slice(&commands));
    }
    self.forward_batches = batches.clone();
    self.shadow_batches = batches;
}
```

For deterministic capture/debugging, sort batch keys and chunk keys before emitting commands;
`HashMap` iteration order is otherwise unspecified.

Grow capacity geometrically, not exactly one command at a time:

```rust
let new_capacity = required.max(1).next_power_of_two();
```

Create the replacement buffer, remove the old one through the proposed `remove_buffer`, and mark
all prepared offsets rebuilt. The buffer contents only need rewriting when chunks or CPU culling
results change; per-planet uniform updates do not dirty the indirect stream.

## CPU culling and LOD

The existing octree/LOD logic already chooses the chunk set, so start by drawing every retained
chunk. Add CPU frustum or horizon culling later inside `rebuild_batches` or `prepare_views`.

If visibility changes every frame, rebuild a compact command list of visible chunks. Writing a few
hundred kilobytes of indirect arguments is normally preferable to issuing thousands of Rust/wgpu
calls. Setting `index_count` to zero also works, but a compact list reduces work consumed by the
render pass.

GPU culling can be added later without changing producer ownership: give the indirect buffer
`STORAGE | INDIRECT` usage and let compute produce the same command layout. That is an optimization,
not part of the first refactor.

## Recording geometry and shadows

The graph nodes already bind shared groups:

- geometry binds the camera at group 0, textures at group 1, and shadow sampling at group 3;
- shadows bind the shadow view at group 0 and textures at group 1.

The producer therefore binds only its pipeline, terrain group 2, and mesh pool pages. It should not
recreate or fetch material/view bind groups in game logic.

```rust
fn record(
    &self,
    ctx: &mut dyn RenderContext,
    _resources: &RenderResources,
    pass: &RenderPassContext<'_>,
) {
    let (pipeline, batches) = match pass.route.material_pass {
        material_passes::FORWARD_OPAQUE => {
            (self.pipelines.forward, &self.forward_batches)
        }
        material_passes::SHADOW => {
            (self.pipelines.shadow, &self.shadow_batches)
        }
        _ => return,
    };

    ctx.bind_pipeline(pipeline);

    for batch in batches {
        ctx.bind_bind_group(2, batch.bind_group);
        ctx.bind_vertex_buffer(0, batch.vertex_buffer);
        ctx.bind_index_buffer(batch.index_buffer);
        ctx.multi_draw_indexed_indirect(
            self.indirect.buffer,
            batch.indirect_offset,
            batch.draw_count,
        );
    }
}
```

Do not bind `pass.view.view_bind_group` here. The current geometry and shadow nodes already bind
group 0 immediately before producers record. Binding it again is redundant and makes the producer
depend on graph internals it does not own.

Each shadow cascade is a separate render view. The registry calls `record` for every matching
shadow view, and the shadow node supplies the correct group-0 view binding. The same terrain
geometry batches can therefore be reused for all cascades initially.

## Removing a planet and shutting down

Removal must release every owned mesh before dropping the map entry:

```rust
fn remove_planet(&mut self, api: &mut dyn RendererAPI, planet: Entity) {
    let Some(state) = self.planets.remove(&planet) else {
        return;
    };
    for chunk in state.chunks.into_values() {
        api.remove_mesh(chunk.mesh);
    }
    self.batches_dirty = true;
}
```

Once the engine exposes removal for buffers, bind groups, layouts, and pipelines, use an explicit
producer teardown hook or renderer-owned RAII wrappers to release the remaining resources. Mesh
removal is already essential because pooled allocations must be returned for chunk churn.

## What changes in `planet_system.rs`

Replace render-object creation and material/bind-group extraction with one command:

```rust
let queue = ctx
    .world
    .get_resource::<PlanetTerrainRenderQueue>()
    .expect("planet terrain producer must be initialized first")
    .clone();

queue
    .send(PlanetTerrainCommand::ReplaceChunks {
        planet: replacement.planet_entity,
        remove: replacement.keys_to_remove,
        insert: replacement
            .meshes
            .into_iter()
            .map(|mesh| PendingTerrainChunk {
                key: mesh.key,
                vertices: mesh.vertices,
                indices: mesh.indices,
            })
            .collect(),
    })
    .expect("terrain producer command channel must remain connected");
```

Delete these concerns from `planet_system.rs` after migration:

- cloning `solid_material`;
- reading forward or shadow pipeline handles;
- creating `RenderData`;
- adding group-2 bind groups;
- retaining/removing render objects;
- tracking `planets_meshes`.

Keep these concerns in gameplay:

- octree state and desired leaves;
- job/version tracking;
- empty chunk knowledge;
- transition state;
- collision generation and removal;
- deciding when a replacement is still current.

## Implementation order

- [ ] Add `RenderContext::multi_draw_indexed_indirect` and its wgpu implementation.
- [ ] Add renderer buffer removal, or temporarily choose a fixed indirect capacity.
- [ ] Update the forward and shadow terrain shaders to consume `GpuTerrainFrame` consistently.
- [ ] Make the terrain material use only `PlanetVertex::layout()`.
- [ ] Define the command queue and optional result-event queue.
- [ ] Move material, palette, layouts, pipelines, and per-planet resources into the producer.
- [ ] Register the producer before planet initialization.
- [ ] Implement `EnsurePlanet`, frame updates, atomic chunk replacement, and planet removal.
- [ ] Build page-aware CPU indirect batches from `GpuMeshBinding`.
- [ ] Record forward and shadow routes with one multi-draw per prepared batch.
- [ ] Replace render-object code in `planet_system.rs` with queue submission.
- [ ] Remove the terrain rendering fields from `GameState`.
- [ ] Add upload failure events and complete LOD transitions only after success.
- [ ] Add profiling counters for visible chunks, batches, indirect commands, uploaded bytes, and
  upload time.
- [ ] Add CPU culling only after the first producer path is stable and measured.

## Invariants worth preserving

- A `GpuMeshHandle` is owned by exactly one live chunk entry.
- Replacing or removing a chunk calls `remove_mesh` exactly once.
- Gameplay never stores renderer handles.
- The producer never owns octree or collision state.
- Material and bind groups are created once, not per chunk or per frame.
- Uniform data may update every frame; indirect data updates only when visibility or mesh bindings
  change.
- One multi-draw batch never crosses a planet, vertex buffer, or index buffer boundary.
- Forward and shadow passes reconstruct identical terrain positions.
- Failed replacements leave the previously visible set intact.

With these boundaries, planet terrain remains a specialized renderer without contaminating the
ordinary model path. Models continue through `MeshRendererComponent` or retained render objects;
terrain uses a producer because it owns high-volume streaming, batching, and draw submission.
