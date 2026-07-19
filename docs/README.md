# Plaxel development guides

These guides describe the intended public workflows for adding gameplay and rendering features.
They focus on choosing the highest-level path that still gives the feature enough control.

## Guides

- [ECS systems](ecs-systems.md): components, queries, resources, commands, scheduling, hot reload, and change detection.
- [Rendering](rendering.md): choosing between `MeshRenderer`, retained render objects, and custom render producers.
- [Materials, passes, and views](render-passes-and-materials.md): material variants, render flags, extensible IDs, graph routes, and shadow views.

## Which rendering path should I use?

| Workload | Preferred path | Why |
| --- | --- | --- |
| A loaded model or a few ordinary meshes | `MeshRendererComponent` | Minimal game code; automatically synchronized into retained rendering. |
| Generated chunks that still behave like ordinary meshes | `RenderObject` | Stable handles and incremental updates without requiring ECS renderer components. |
| A galaxy, particles, GPU-generated terrain, or specialized batching | `RenderProducer` | Owns its GPU buffers, update policy, batching, and direct/indirect draw recording. |
| A fullscreen effect such as bloom | A render graph node | The effect transforms graph resources rather than submitting world geometry. |

These paths are composable. A building system can use retained objects initially and later replace
only its high-volume chunk drawing with a producer. Other models can continue using
`MeshRendererComponent`.

## Core rule

Gameplay owns persistent simulation state. Render objects and producers own persistent rendering
state. A frame should upload only data that changed; it should not rebuild a second copy of the
entire world every frame.

