# Game and Planet Rendering Refactor Plan

## Direction

A substantial refactor is justified. The underlying systems are decent, but the code still carries architecture from before the ECS and newer renderer existed.

The key decision is:

> Planet terrain deserves a specialized producer eventually, but not a separate render pass.

The central architectural boundary should be:

> Game systems decide what planet chunks should exist. The planet producer decides how those chunks live and render on the GPU.

This removes rendering mechanics from game logic without pretending planet terrain is an ordinary imported 3D model.

## 1. Improve Mesh Uploading and Establish Ownership

Replace `create_render_data()` with a focused upload API:

```rust
pub struct MeshUpload<'a> {
    pub label: &'a str,
    pub vertices: &'a [u8],
    pub indices: &'a [u32],
    pub vertex_layout: &'a VertexLayout,
}

pub fn upload_mesh(&mut self, upload: MeshUpload<'_>) -> GpuMeshHandle;
pub fn remove_mesh(&mut self, handle: GpuMeshHandle);
```

`upload_mesh()` should only:

- [ ] Validate the vertex layout and input data.
- [ ] Allocate from the vertex and index pools.
- [ ] Upload the bytes.
- [ ] Record the resulting draw ranges.
- [ ] Return a typed, generational GPU handle.

It should not know about:

- [ ] Materials.
- [ ] Pipelines.
- [ ] Render phases.
- [ ] Transforms.
- [ ] Planets.
- [ ] Bind groups.

Most importantly, mesh removal must release the pool allocations. That lifetime model should be solved before moving planet rendering around.

`RenderData` can probably be deleted entirely. Its `transform_index` and `sort_key` are already unused, and most of its remaining data merely passes through the function.

### Mesh upload tasks

- [ ] Remove the material and pipeline parameters from mesh upload.
- [ ] Accept slices or owned upload data instead of `&Vec<T>`.
- [ ] Avoid the current intermediate `Vec<u8>` and `MeshAsset` clones where possible.
- [ ] Stop inventing a random UUID and the name `"Cube"` for procedural meshes.
- [ ] Make the vertex layout an explicit property of the uploaded mesh.
- [ ] Introduce a generational GPU mesh store.
- [ ] Ensure removing the final owner of a GPU mesh frees its vertex and index allocations.
- [ ] Add stale-handle validation.
- [ ] Add tests for allocation, upload, lookup, removal, slot reuse, and stale handles.
- [ ] Replace the elapsed-time-only upload throttle with a byte budget, optionally retaining a CPU-time safety limit.

## 2. Remove `GameState` and Replace It with Focused Resources

A world resource is perfectly compatible with ECS. The problem is not that `GameState` is a resource; the problem is that it is a god resource containing unrelated subsystems.

Several fields are entirely obsolete:

- [ ] Remove `previous_leaves`.
- [ ] Remove `current_leaves`.
- [ ] Remove `mesh_neighbor_signatures`.
- [ ] Remove the old `in_flight` set.
- [ ] Remove `empty_chunks`.
- [ ] Remove `empty_neighbor_signatures`.
- [ ] Remove `update_octree`.
- [ ] Remove `octree_job_in_flight`.
- [ ] Remove `last_requested_camera_pos`.

They are only initialized and cleared, never meaningfully used by the current systems.

The remaining fields separate naturally:

```rust
struct PlanetRenderResources {
    material: Material,
    forward_pipeline: PipelineHandle,
    shadow_pipeline: PipelineHandle,
    terrain_materials_bind_group: BindGroupHandle,
}

struct PlanetChunkRenderState {
    chunks: HashMap<NodeKey, GpuMeshHandle>,
}

struct TerrainPhysicsState {
    enabled: bool,
    colliders: HashMap<NodeKey, RapierColliderHandle>,
}

struct TerrainBrushSettings {
    radius: f32,
}

struct PlanetDebugSettings {
    nodes: Vec<DebugNode>,
    depth: u32,
}
```

If a custom planet producer is used, `PlanetRenderResources` and `PlanetChunkRenderState` should be producer-owned instead of ECS resources.

### `GameState` migration tasks

- [ ] Delete fields that are no longer used.
- [ ] Move collider ownership and physics settings into `TerrainPhysicsState`.
- [ ] Move the terrain brush radius into `TerrainBrushSettings`.
- [ ] Move debug state into `PlanetDebugSettings`.
- [ ] Move planet render resources and chunk handles into the planet producer.
- [ ] Remove `GameState` after its final responsibility has moved.

### Camera state

`GameCamera` should also be examined. It currently duplicates data between:

- `TransformComponent`
- `CameraComponent`
- `GameCamera.camera`
- `GameCamera.uniform`
- `GameCamera.controller`

A better eventual shape is:

```rust
struct ActiveCamera(Entity);
struct CameraControllerComponent(...);
```

The renderer should derive camera matrices and uniforms from ECS components.

- [ ] Introduce an `ActiveCamera` resource containing only the active camera entity.
- [ ] Move controller state into a component or a focused controller resource.
- [ ] Make ECS camera and transform components authoritative.
- [ ] Make renderer synchronization derive GPU camera data from ECS state.
- [ ] Remove duplicated camera and uniform state.

## 3. Planet Rendering

Planet terrain is unusual in generation, streaming, ownership, LOD transitions, and culling.

Once uploaded, however, each chunk is still:

- An indexed triangle mesh.
- Using an ordinary graphics pipeline.
- Participating in the geometry pass.
- Participating in shadow passes.

Therefore, do not create a `PlanetRenderNode` or a separate planet render pass. Route a `PlanetTerrainProducer` into the existing geometry and shadow passes.

```text
Planet simulation / LOD
        |
        v
PlanetRenderCommand channel
        |
        v
PlanetTerrainProducer::prepare_frame
  - upload/remove meshes
  - apply atomic replacements
  - prepare draw list
        |
        v
existing geometry and shadow passes
        |
        v
PlanetTerrainProducer::record
```

The producer could accept:

```rust
enum PlanetRenderCommand {
    Replace {
        remove: Vec<NodeKey>,
        insert: Vec<(NodeKey, CpuMesh)>,
        completion: ReplacementToken,
    },
    Remove {
        keys: Vec<NodeKey>,
    },
}
```

Internally:

```rust
struct PlanetTerrainProducer {
    chunks: HashMap<NodeKey, GpuMeshHandle>,
    pending: Receiver<PlanetRenderCommand>,
    material: Material,
    pipelines: PlanetPipelines,
    routes: Vec<RenderRoute>,
}
```

This is a good producer workload because it can own:

- Atomic chunk replacement.
- Mesh upload throttling.
- GPU mesh lifetimes.
- Terrain-specific bind groups.
- Chunk draw ordering.
- Later frustum, horizon, or occlusion culling.
- Later indirect drawing or batching.

The existing producer abstraction explicitly targets workloads such as galaxies, voxel chunks, and particles, so planet terrain matches its intended purpose.

Retain the generic `upload_mesh()` underneath the producer. A producer should not reimplement pool allocation.

### When standard rendering would still be fine

The standard retained-object path is sufficient if:

- Chunk counts remain modest.
- Every chunk is simply one normal draw.
- Renderer-level frustum culling is added.
- Atomic replacements happen outside the render database.
- GPU mesh lifetime is fixed.

A producer is not required because the geometry is procedural. It is suitable because the chunk collection has specialized lifetime and update policies.

The preferred direction for Plaxel is the specialized producer because the terrain already has atomic LOD transitions and high churn.

### Planet producer tasks

- [ ] Define renderer-facing `CpuMesh` or `MeshUploadOwned` data.
- [ ] Define `PlanetRenderCommand` without ECS borrows or game-world references.
- [ ] Register a `PlanetTerrainProducer` from game initialization.
- [ ] Route it through the existing geometry pass.
- [ ] Route it through the existing shadow pass.
- [ ] Move terrain material, pipeline, and bind-group ownership into the producer.
- [ ] Drain render commands during `prepare_frame` using a byte budget.
- [ ] Upload every mesh in a replacement before changing the visible chunk map.
- [ ] Swap a completed replacement atomically.
- [ ] Free all replaced or removed GPU meshes.
- [ ] Report replacement completion back to game logic without exposing renderer internals.
- [ ] Add culling and batching only after profiling demonstrates a need.

## 4. Split Game Logic by Responsibility

Target approximately this shape:

```text
game/logic/src/
  lib.rs                  registration only
  prelude.rs              added last

  camera/
    mod.rs
    controller.rs
    systems.rs

  player/
    actions.rs
    interaction.rs
    movement.rs

  planets/
    mod.rs
    spawn.rs
    lod.rs
    terrain_edits.rs

    meshing/
      mod.rs
      requests.rs
      generation.rs
      scheduler.rs
      results.rs

    physics/
      mod.rs
      colliders.rs

    rendering/
      mod.rs
      commands.rs
      producer.rs
      resources.rs

    debug/
      mod.rs
      systems.rs
      settings.rs
```

The current `planet_system_update()` should become multiple scheduled systems:

- [ ] `update_planet_lod`
- [ ] `apply_planet_lod_changes`
- [ ] `collect_completed_mesh_jobs`
- [ ] `submit_pending_mesh_jobs`
- [ ] `send_planet_render_commands`
- [ ] `update_planet_atmosphere`
- [ ] `log_camera_altitude`

Not every small function needs its own system, but systems should have a single observable responsibility.

Pure computation should remain ordinary functions:

```rust
fn generate_density_grid(...)
fn build_requested_mesh(...)
fn priority_for_requests(...)
fn collect_affected_neighbors(...)
```

### General cleanup tasks

- [ ] Delete or archive the dead duplicate `game/logic/src/systems/planet_system.rs`.
- [ ] Remove the incomplete `PlanetNodeInstance` experiment and restore compilation.
- [ ] Update or archive `mesh_job_system_initial_design.md`, whose `GameState` ownership model is now outdated.
- [ ] Keep worker jobs CPU-only and independent of ECS and renderer access.
- [ ] Keep transient requests and completed results in queues/resources rather than creating entities for them.
- [ ] Keep persistent planet state on planet entities.
- [ ] Only turn individual chunks into ECS entities if they later require persistent editor identity, selection, independent visibility, or component queries.

## 5. Improve Input Around Actions

`InputState` as a world resource is correct. `InputMap` is also heading in the right direction.

What should disappear is gameplay code repeatedly interpreting raw keys:

```rust
input.pressed.contains(&KeyCode::...)
```

Prefer an engine-supported action layer:

```rust
enum PlayerAction {
    MoveForward,
    Jump,
    Interact,
    IncreaseBrushRadius,
}

struct ActionState<A> {
    pressed: HashSet<A>,
    just_pressed: HashSet<A>,
    values: HashMap<A, f32>,
}
```

Gameplay systems consume `ActionState<PlayerAction>`, while one input-mapping system translates keyboard, mouse, and gamepad state into actions.

Debug toggles should also be scheduled systems consuming actions, instead of `handle_key_press()` directly mutating several resources outside the normal schedule.

### Input tasks

- [ ] Define generic `ActionState<A>` and action bindings in the engine.
- [ ] Translate raw `InputState` into game actions once per frame.
- [ ] Make player movement and interaction consume actions.
- [ ] Make terrain editing consume actions.
- [ ] Move debug shortcuts into scheduled debug systems.
- [ ] Remove direct game-state mutation from `handle_key_press()`.
- [ ] Check whether the old `GlobalResources.input` copy is still needed after world `InputState` is authoritative.

## 6. Handle Design

Do not use one handle concept for both persistent assets and runtime GPU allocations. They have different identity, lifetime, serialization, and ownership rules.

### Asset handles

Use `Handle<T>` for CPU/source assets managed by `AssetManager`:

```rust
Handle<MeshAsset>
Handle<TextureAsset>
Handle<Material>
```

These represent stable asset identity and may be serialized into scenes or prefabs. UUID identity is appropriate here.

The current `Handle<T>` is a typed UUID and is acceptable as a starting point. Possible later refinements are:

```rust
pub struct AssetId<T> {
    uuid: Uuid,
    marker: PhantomData<fn() -> T>,
}

pub enum Handle<T> {
    Strong(Arc<StrongHandle>),
    Weak(AssetId<T>),
}
```

Do not introduce strong/weak handles until automatic asset unloading is actually required. A typed, copyable asset ID is simpler for the current engine.

The `asset_type` field inside typed `Handle<T>` is redundant because `T: Asset` already provides `T::ASSET_TYPE`. Keep the type tag on an `UntypedHandle`, but consider removing it from the typed handle after serialization compatibility is considered.

### GPU handles

Use a separate non-serializable, generational runtime handle:

```rust
pub struct GpuHandle<T> {
    index: u32,
    generation: u32,
    marker: PhantomData<fn() -> T>,
}

pub enum GpuMesh {}
pub type GpuMeshHandle = GpuHandle<GpuMesh>;
```

Alternatively, use explicit newtypes if the generic abstraction does not reduce implementation code:

```rust
pub struct GpuMeshHandle {
    index: u32,
    generation: u32,
}
```

`GpuMeshHandle` is preferable to `Handle<GpuMesh>` in the current architecture because:

- A GPU allocation is not an asset.
- It should not use a UUID.
- It should not be serialized.
- It is owned by one renderer/backend instance.
- Its slot can be reused after removal, requiring a generation check.
- Device recreation invalidates or rebuilds it differently from an asset.

The relationship should be:

```text
Handle<MeshAsset> / AssetId<MeshAsset>
    stable source identity
              |
              v
renderer preparation or procedural upload
              |
              v
GpuMeshHandle
    transient allocation identity
```

Imported meshes may have both identities. A procedural planet chunk may only have a `GpuMeshHandle`, because it does not need to become a serialized asset.

### Handle tasks

- [ ] Document asset handles as persistent source identities.
- [ ] Introduce a generic generational GPU arena or a mesh-specific generational store.
- [ ] Add `GpuMeshHandle` using index plus generation.
- [ ] Make GPU handles non-serializable.
- [ ] Validate the generation on every lookup and removal.
- [ ] Decide whether buffers, textures, pipelines, samplers, and bind groups should migrate from raw `u32` handles to the same generational GPU handle mechanism.
- [ ] Keep asset-to-GPU preparation maps keyed by `AssetId<T>` or `Handle<T>`, not by `Handle<GpuT>`.
- [ ] Keep procedural GPU resources directly owned by their producer or render workload.
- [ ] Defer strong/weak asset reference counting until automatic unloading is needed.

## 7. Derive Macros for `Resource`, `Component`, and `Asset`

Do not add Bevy-style `Resource` or `Component` derives yet.

Currently the engine has blanket implementations:

```rust
impl<T: 'static + Send + Sync> Resource for T {}
impl<T: 'static + Send + Sync> Component for T {}
```

That means every compatible Rust type is automatically a resource and a component. A derive macro would add no behavior, and a derive that emits another trait implementation would conflict with the blanket implementation.

First decide whether ECS participation should be explicit.

### Recommended long-term direction

Explicit opt-in is preferable for a maturing engine:

```rust
#[derive(Component)]
struct TransformComponent { ... }

#[derive(Resource)]
struct TerrainBrushSettings { ... }
```

Benefits include:

- Intent is visible at the type definition.
- Arbitrary types cannot accidentally be inserted into ECS storage.
- Derives can later provide registration, reflection, stable names, serialization metadata, storage strategy, or change-detection metadata.
- Compiler errors occur closer to the missing declaration.

But macros are worthwhile only after the traits have meaningful semantics beyond `Send + Sync + 'static`.

### `Asset` derives

An `Asset` derive could eventually be useful if asset types share standardized metadata:

```rust
#[derive(Asset)]
#[asset(type = "mesh")]
struct MeshAsset {
    #[asset(id)]
    uuid: Uuid,
    // ...
}
```

It could generate:

- The `Asset` implementation.
- Static asset type metadata.
- UUID access.
- Loader or registry registration hooks.
- Reflection or editor metadata.

Do not add it merely to replace the current small explicit implementations. Design asset registration, IDs, loading, and unloading first.

### Macro implementation guidance

The existing inspector derive manually parses `TokenStream::to_string()`. Do not extend that parser for foundational ECS derives. Use `syn` and `quote` in a general derive crate, for example:

```text
engine_derive/
  Component
  Resource
  Asset
  Inspector
```

Keep the traits in `engine`; keep their procedural macros in `engine_derive`; re-export derives from `engine` for ergonomic imports.

### Macro tasks

- [ ] Finish the major game and renderer boundaries before adding derives.
- [ ] Decide whether `Component` and `Resource` should require explicit opt-in.
- [ ] If explicit opt-in is chosen, remove the blanket implementations.
- [ ] Add compile-fail tests proving unmarked types cannot enter ECS storage.
- [ ] Create or rename a general `engine_derive` proc-macro crate.
- [ ] Use `syn` and `quote` rather than extending the current string parser.
- [ ] Implement `Component` and `Resource` derives only after their trait contracts are settled.
- [ ] Design asset registration and lifetime semantics before implementing `Asset` derive.
- [ ] Re-export derive macros without placing low-level internals in the general prelude.

## 8. Add Preludes Last

Add preludes after module boundaries stabilize. Otherwise every module move will repeatedly churn imports and obscure dependency problems.

Keep preludes conservative:

```rust
// engine::prelude
pub use crate::{
    ecs::{Commands, Entity, Query, SystemContext, World},
    math::{Quat, Vec2, Vec3},
    core::components::{TransformComponent, CameraComponent},
};
```

Avoid exporting renderer internals such as:

- `RendererAPI`
- Raw buffer handles
- Render graph node types
- Backend-specific objects

Use separate preludes where appropriate:

```rust
engine::prelude
engine::render::prelude
game_types::prelude
```

This prevents ordinary gameplay modules from accidentally depending on low-level rendering APIs.

### Prelude tasks

- [ ] Wait until the module and ownership refactors are stable.
- [ ] Add a small `engine::prelude` for common game-facing ECS and math types.
- [ ] Add a separate `engine::render::prelude` if it materially improves renderer extension code.
- [ ] Add `game_types::prelude` only for genuinely common shared game types.
- [ ] Do not export backend types or broad wildcard module contents.
- [ ] Check dependency direction after converting modules to use the preludes.

## Recommended Execution Order

- [ ] **1. Restore the baseline:** remove the incomplete `PlanetNodeInstance` experiment and make the workspace compile again.
- [ ] **2. Remove obvious legacy code:** delete dead `GameState` fields and the dead duplicate `planet_system.rs`.
- [ ] **3. Fix GPU ownership:** implement `GpuMeshHandle`, focused `upload_mesh()`, correct deallocation, and tests.
- [ ] **4. Establish the planet render boundary:** add the command queue and `PlanetTerrainProducer`.
- [ ] **5. Move render ownership:** move material, pipeline, bind-group, chunk-map, and GPU lifetime management into the producer.
- [ ] **6. Dissolve `GameState`:** move remaining physics, brush, and debug data into focused resources.
- [ ] **7. Split systems and modules:** separate planet LOD, meshing, job coordination, physics, rendering, atmosphere, and debug code.
- [ ] **8. Refactor camera ownership:** make ECS camera and transform data authoritative.
- [ ] **9. Introduce action-based input:** migrate player, editing, camera, and debug controls.
- [ ] **10. Reassess ECS traits:** decide whether components and resources require explicit opt-in and only then add derives.
- [ ] **11. Reassess asset semantics:** decide asset identity, loading, reference counting, and registration before adding an `Asset` derive.
- [ ] **12. Add preludes:** expose the now-stable, game-facing API surface.

## Out of Scope Until Profiling Requires It

- [ ] Do not add GPU-driven indirect planet rendering yet.
- [ ] Do not add a planet-specific render graph pass without a genuinely different framebuffer dependency.
- [ ] Do not make every planet chunk an ECS entity by default.
- [ ] Do not add reference-counted asset handles until automatic unloading is needed.
- [ ] Do not add derive macros that provide no semantics beyond existing blanket implementations.
- [ ] Do not optimize pool slab sizes, batching, or staging transfers without measurements.

