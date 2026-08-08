# Initial Mesh Job System Design

## Goal

Move planet mesh generation off the main thread without changing the rest of the engine threading model yet.

For the initial version:

- the main loop stays single threaded
- ECS systems stay single threaded
- rendering stays single threaded
- worker threads only generate CPU-side mesh data
- GPU upload stays on the main thread
- completed mesh uploads are limited to a small per-frame budget, initially 2 ms

This keeps the engine simple while removing the large terrain generation stalls from the frame.

## Ownership Model

Use a strict ownership rule:

```text
Worker threads:
  generate CPU data only
  do not mutate World
  do not access renderer
  do not create GPU resources

Main thread:
  owns World
  owns renderer
  owns GPU upload
  decides which generated meshes are still valid
```

This avoids locking the ECS world or renderer and keeps most code in the current single-threaded model.

## Where The Job System Lives

The job system should be an engine-level service, not a component.

For the first version, store it as a resource accessible to systems. In this codebase there are two reasonable places:

- `GlobalResources`, if the service is shared engine infrastructure
- `World` resources, if the service is scene/game-specific

The better initial choice is `GlobalResources` for the generic `JobSystem`, because it is engine infrastructure similar to the renderer and asset manager.

Scene-specific terrain state should stay in `World` resources, for example:

```text
PlanetMeshJobs / PlanetWorkerCoord:
  completed mesh receiver
  in-flight counters
  scheduled/completed stats

GameState:
  current chunk meshes
  in-flight chunk keys
  empty chunks
  current leaves
  materials
```

Keep the generic worker pool separate from planet-specific state. That makes the job system reusable later for asset processing, physics preparation, factory simulation, and ECS parallel work.

## Are Non-Query ECS Systems OK?

Yes.

An ECS system does not have to iterate entities/components. A system is just a scheduled unit of engine logic. Some systems operate on resources, queues, renderer state, input state, or command buffers.

Examples of valid non-query systems:

```text
UploadGeneratedPlanetMeshes
DrainAssetLoadResults
SyncCameraToRenderer
ApplyInputState
RunPhysicsStep
FlushDespawnCommands
```

So an upload system that only drains a completed mesh queue and checks a 2 ms budget is a good fit.

## Components vs Resources

Do not make every request or completed mesh a component.

Use components for persistent per-entity state:

```text
Planet
TransformComponent
Renderable marker/state, later if needed
```

Use resources for global or scene-level queues and registries:

```text
Planet mesh request queue
Completed mesh receiver
In-flight chunk set
Current chunk mesh map
Chunk generation/version map
```

Mesh generation requests are transient work items. Completed meshes are transient results. They should be queued, not stored as components.

Components would make sense later if a chunk becomes a real entity with persistent identity:

```text
PlanetChunkComponent
ChunkLodComponent
ChunkMeshStateComponent
```

For now, chunk keys plus resource-owned maps are simpler and better aligned with the current code.

## Initial Data Flow

```text
Planet LOD / generation system
  detects required chunks
  skips chunks already loaded, empty, or in-flight
  submits mesh generation jobs
  marks chunk keys as in-flight

Job system worker
  receives mesh job
  builds height grid
  runs dual contouring
  builds CPU vertex/index buffers
  sends GeneratedPlanetMesh to completed queue

UploadGeneratedPlanetMeshes system
  drains completed queue for up to 2 ms
  removes chunk key from in-flight
  discards stale results
  uploads valid meshes to GPU
  stores RenderData in GameState

Render sync
  copies current RenderData values into the geometry pass
```

## Initial Types

The generic job system can start very small:

```rust
pub type Job = Box<dyn FnOnce() + Send + 'static>;

pub struct JobSystem {
    // fixed worker threads
    // sender for pending jobs
    // shutdown flag
}

impl JobSystem {
    pub fn new(worker_count: usize) -> Self;
    pub fn spawn(&self, job: impl FnOnce() + Send + 'static);
}
```

The planet-specific messages should be separate:

```rust
pub struct PlanetMeshJob {
    pub key: NodeKey,
    pub center: Point3<f32>,
    pub size: f32,
    pub neighbors: Vec<ChunkNeighbor>,
    pub neighbor_signature: NeighborSignature,
    pub planet_position: Vector3<f32>,
    pub planet_size: u32,
}

pub struct GeneratedPlanetMesh {
    pub key: NodeKey,
    pub neighbor_signature: NeighborSignature,
    pub vertices: Vec<PlanetVertex>,
    pub indices: Vec<u32>,
}
```

The completed queue can initially be a channel:

```rust
crossbeam_channel::Sender<GeneratedPlanetMesh>
crossbeam_channel::Receiver<GeneratedPlanetMesh>
```

`std::sync::mpsc` is acceptable for a prototype, but `crossbeam-channel` is usually a better long-term default.

## Upload Budget System

Create a dedicated system or frame step for draining completed meshes.

It does not need to query components. It should operate on:

- `GameState`
- planet mesh job coordinator resource
- renderer from `GlobalResources`

Pseudo-code:

```rust
fn upload_generated_planet_meshes(ctx: &mut SystemContext, _commands: &mut Commands) {
    let start = Instant::now();
    let budget = Duration::from_millis(2);

    while start.elapsed() < budget {
        let Ok(mesh) = completed_meshes.try_recv() else {
            break;
        };

        game_state.in_flight.remove(&mesh.key);

        if !is_still_current(&game_state, &mesh) {
            continue;
        }

        if mesh.vertices.is_empty() {
            remember_empty_chunk(&mut game_state, mesh);
            continue;
        }

        let render_data = renderer.create_render_data(...);
        game_state.current_meshes.insert(mesh.key, render_data);
    }
}
```

The 2 ms budget should cover CPU-side integration and GPU resource creation calls made from the main thread. If GPU upload still causes spikes, lower the budget or limit the number of meshes uploaded per frame as well.

## Validity Checks

Do not blindly upload completed worker results.

Before upload, check that the result still matches the current planet/chunk state:

- the chunk key is still wanted
- the chunk is not superseded by a newer LOD request
- the neighbor signature still matches, if skirts depend on neighbors
- later: the generation/version ID still matches

The current code already uses `NodeKey` and `NeighborSignature`; a later version should add an explicit `generation: u64` per chunk request.

## Priorities

The first implementation can sort chunk jobs before submission:

```text
near camera first
missing visible mesh before LOD upgrade
physics-needed chunks before visual-only chunks
```

This is enough while the worker pool uses a simple queue.

Later, move priority into the job system:

```text
High queue:
  nearby missing chunks

Normal queue:
  visible LOD improvements

Low queue:
  far/preload/speculative chunks
```

Workers should consume high priority before normal, and normal before low.

## Cancellation

The initial version can rely on stale-result rejection.

That means old jobs may still finish, but their result is discarded if the chunk is no longer needed. This is simple and safe.

Later, add cooperative cancellation:

```rust
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}
```

Mesh generation checks the token between expensive stages:

```text
before grid generation
after grid generation
after contouring
before sending result
```

Do not forcibly kill worker threads.

## Work Stealing

Do not start with work stealing.

The first scalable step is:

```text
fixed worker pool
global queue
completed result channel
2 ms upload budget
stale result rejection
```

Add work stealing when jobs begin spawning sub-jobs or when profiling shows queue contention/load imbalance.

The future work-stealing version should use:

```text
global injector for main-thread submissions
local deque per worker
stealers for idle workers
```

`crossbeam-deque` is the right Rust crate for that shape.

## Frame Order

For the current single-threaded engine, the frame should roughly be:

```text
1. input
2. ECS update
   - planet LOD system submits missing mesh jobs
   - upload generated mesh system drains completed results for up to 2 ms
3. sync world/render resources
4. render
```

Uploading before render means meshes completed by workers can become visible in the same frame if budget allows.

## What To Avoid For Now

Avoid these until the basic path is stable:

- ECS world access from workers
- renderer/GPU access from workers
- blocking waits on the main thread for terrain
- job dependencies
- nested jobs
- parallel ECS iteration
- render thread
- physics thread
- making every chunk request an entity/component

The first version should solve one concrete problem: planet mesh generation should no longer freeze the main thread.

## Implementation Milestone

Milestone 1 is complete when:

- planet mesh generation jobs run on worker threads
- main thread remains responsive while chunks generate
- completed meshes are uploaded only on the main thread
- upload draining is capped at about 2 ms per frame
- stale chunk results are discarded instead of uploaded
- existing render sync uses the uploaded `RenderData`

After that, profiling should decide the next step: priorities in the queue, cancellation tokens, or work stealing.
