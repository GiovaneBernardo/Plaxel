# ECS systems

## Define components and resources

Any `'static + Send + Sync` Rust type is automatically an ECS component or resource. No derive is
required.

```rust
#[derive(Clone, Copy)]
pub struct Velocity {
    pub linear: engine::math::Vec3,
}

#[derive(Default)]
pub struct VehicleRegistry {
    pub vehicle_count: usize,
}
```

Insert initial resources while initializing the game or scene:

```rust
let world = state.active_scene_mut().unwrap().world_mut();
world.insert_resource(VehicleRegistry::default());
```

## Write a system

A system receives a `SystemContext` and a deferred `Commands` queue.

```rust
use engine::ecs::{commands::Commands, query::Query, system::SystemContext};
use engine::core::components::core::TransformComponent;

pub fn movement_system(ctx: &mut SystemContext, _commands: &mut Commands) {
    let dt = ctx
        .world
        .get_resource::<engine::core::time::Time>()
        .map(|time| time.delta_time)
        .unwrap_or_default();

    let mut query = Query::<(&mut TransformComponent, &Velocity)>::new(ctx.world);
    query.for_each(|_entity, (transform, velocity)| {
        transform.position += velocity.linear * dt;
    });
}
```

Mutable queries mark the component as changed at the system's current change tick. The retained
renderer uses this to update only changed transforms.

### Borrow resources in small scopes

World storage uses runtime-checked borrows. Do not hold a resource borrow while trying to borrow the
same resource mutably.

```rust
let count = {
    let registry = ctx.world.get_resource::<VehicleRegistry>().unwrap();
    registry.vehicle_count
}; // immutable borrow ends here

ctx.world
    .get_resource_mut::<VehicleRegistry>()
    .unwrap()
    .vehicle_count = count + 1;
```

## Structural changes

Use `Commands` when a query or another borrow is active. Commands run after the current system.

```rust
pub fn spawn_vehicle_system(ctx: &mut SystemContext, commands: &mut Commands) {
    let should_spawn = true;
    if !should_spawn {
        return;
    }

    commands.push(|ctx| {
        let entity = ctx.world.spawn();
        ctx.world.insert(entity, Velocity {
            linear: engine::math::vec3(0.0, 0.0, 1.0),
        });
        ctx.world.insert(entity, engine::core::components::core::TransformComponent {
            position: engine::math::Vec3::ZERO,
            rotation: engine::math::Quat::IDENTITY,
            scale: engine::math::Vec3::ONE,
            velocity: engine::math::Vec3::ZERO,
        });
    });
}
```

Direct `world.insert`, `world.remove`, and `world.despawn` calls are fine when no conflicting query
or storage borrow is alive.

## Register systems

System order is currently registration order. Register initialization work in `init_schedule` and
per-frame work in `update_schedule`.

```rust
fn register_static_schedule_systems(state: &mut engine::State) {
    let Some(scene) = state.active_scene_mut() else {
        return;
    };

    scene.init_schedule_mut().add_system(spawn_initial_vehicles);
    scene.update_schedule_mut().add_system(movement_system);
}
```

The available registration forms are:

- `add_system`: hotpatch-aware system using its Rust type name.
- `add_named_system`: hotpatch-aware system with an explicit stable diagnostic name.
- `add_static_system`: ordinary function call without hotpatch lookup.
- `add_static_named_system`: static system with an explicit diagnostic name.

Prefer explicit names in runner-side hot-reload registration:

```rust
schedule.add_named_system("game.movement", game::hot_movement_system);
```

This project has two build paths:

1. Static builds call `game_logic::register_systems` in `game/logic/src/lib.rs`.
2. Hot-reload builds register exported functions in both `game/runner/src/main.rs` and
   `editor/runner/src/lib.rs`.

When adding a hot-reloaded game system, update both runner registration lists. The system function
itself should live in game logic, with a stable exported wrapper when required by the hot loader.

## Change detection

Every scheduled system receives:

- `ctx.last_run_tick`: the previous tick on which that specific system ran.
- `ctx.this_run_tick`: the tick assigned to its current run.

For specialized incremental processing, query a storage directly:

```rust
if let Some(storage) = ctx.world.get_storage::<TransformComponent>() {
    for (entity, transform) in
        storage.iter_changed_since(ctx.last_run_tick, ctx.this_run_tick)
    {
        // Update a cache only for changed transforms.
    }
}
```

Use the world change journal for insert/remove/despawn events. Each consumer must keep its own
`ChangeCursor`; acknowledging one cursor does not consume events for other readers.

## Common mistakes

- Do not add a system that extracts every mesh into a temporary vector each frame. Ordinary meshes
  are already synchronized incrementally by the renderer.
- Do not keep `Ref` or `RefMut` guards across unrelated world operations.
- Do not assume deferred commands are visible inside the same system invocation.
- Do not register the same system in both game logic and a hot runner during the same build path.
