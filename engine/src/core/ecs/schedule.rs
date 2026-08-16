use std::collections::HashMap;

use crate::ecs::{
    access::SystemAccess,
    change::ChangeTick,
    commands::Commands,
    system::{
        HotSystem, IntoSystem, LegacyFunctionSystem, ParamSystemMarker, System, SystemContext,
        SystemParamFunction,
    },
};

struct ScheduledSystem {
    name: &'static str,
    system: System,
    last_run_tick: ChangeTick,
}

pub struct Schedule {
    systems: Vec<ScheduledSystem>,
}

pub struct Schedules {
    pub schedules: HashMap<CoreSchedule, Schedule>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CoreSchedule {
    Startup,
    First,
    PreUpdate,
    Update,
    PostUpdate,
    Last,
    RenderExtract,
    RenderPrepare,
    Render,
}

impl CoreSchedule {
    pub const ALL: [Self; 9] = [
        Self::Startup,
        Self::First,
        Self::PreUpdate,
        Self::Update,
        Self::PostUpdate,
        Self::Last,
        Self::RenderExtract,
        Self::RenderPrepare,
        Self::Render,
    ];
}

impl Schedules {
    pub fn new() -> Self {
        Self {
            schedules: CoreSchedule::ALL
                .into_iter()
                .map(|label| (label, Schedule::new()))
                .collect(),
        }
    }

    pub fn get_mut(&mut self, label: CoreSchedule) -> &mut Schedule {
        self.schedules.entry(label).or_insert_with(Schedule::new)
    }

    pub fn initialize(&mut self, world: &mut crate::ecs::world::World) {
        for schedule in self.schedules.values_mut() {
            schedule.initialize(world);
        }
    }

    /// Runs one labeled schedule. Returns `false` when the label has not been
    /// registered instead of silently creating an empty schedule.
    pub fn run(&mut self, label: CoreSchedule, context: &mut SystemContext<'_>) -> bool {
        let Some(schedule) = self.schedules.get_mut(&label) else {
            return false;
        };
        schedule.run(context);
        true
    }
}

impl Default for Schedules {
    fn default() -> Self {
        Self::new()
    }
}

impl Schedule {
    pub fn new() -> Self {
        Self {
            systems: Vec::new(),
        }
    }

    /// Adds a system whose parameters are automatically fetched from the world.
    pub fn add_system<Marker, F>(&mut self, system: F)
    where
        F: SystemParamFunction<Marker>,
        Marker: 'static,
    {
        self.add_named_system(std::any::type_name::<F>(), system);
    }

    /// Adds a named system whose parameters are automatically fetched.
    pub fn add_named_system<Marker, F>(&mut self, name: &'static str, system: F)
    where
        F: SystemParamFunction<Marker>,
        Marker: 'static,
    {
        let system = IntoSystem::<ParamSystemMarker<Marker>>::into_system(system);
        self.systems.push(ScheduledSystem {
            name,
            system: Box::new(HotSystem::new(name, system)),
            last_run_tick: ChangeTick::default(),
        });
    }

    pub fn add_static_system<Marker, F>(&mut self, system: F)
    where
        F: SystemParamFunction<Marker>,
        Marker: 'static,
    {
        self.add_static_named_system(std::any::type_name::<F>(), system);
    }

    pub fn add_static_named_system<Marker, F>(&mut self, name: &'static str, system: F)
    where
        F: SystemParamFunction<Marker>,
        Marker: 'static,
    {
        let system = IntoSystem::<ParamSystemMarker<Marker>>::into_system(system);
        self.systems.push(ScheduledSystem {
            name,
            system: Box::new(system),
            last_run_tick: ChangeTick::default(),
        });
    }

    /// Compatibility path for systems that still request the complete
    /// `SystemContext`. These systems are recorded as exclusive.
    pub fn add_legacy_system<F>(&mut self, system: F)
    where
        F: for<'world> FnMut(&mut SystemContext<'world>, &mut Commands) + Send + 'static,
    {
        self.add_named_legacy_system(std::any::type_name::<F>(), system);
    }

    pub fn add_named_legacy_system<F>(&mut self, name: &'static str, system: F)
    where
        F: for<'world> FnMut(&mut SystemContext<'world>, &mut Commands) + Send + 'static,
    {
        let system = LegacyFunctionSystem::new(system);
        self.systems.push(ScheduledSystem {
            name,
            system: Box::new(HotSystem::new(name, system)),
            last_run_tick: ChangeTick::default(),
        });
    }

    pub fn add_static_legacy_system<F>(&mut self, system: F)
    where
        F: for<'world> FnMut(&mut SystemContext<'world>, &mut Commands) + Send + 'static,
    {
        self.add_static_named_legacy_system(std::any::type_name::<F>(), system);
    }

    pub fn add_static_named_legacy_system<F>(&mut self, name: &'static str, system: F)
    where
        F: for<'world> FnMut(&mut SystemContext<'world>, &mut Commands) + Send + 'static,
    {
        self.systems.push(ScheduledSystem {
            name,
            system: Box::new(LegacyFunctionSystem::new(system)),
            last_run_tick: ChangeTick::default(),
        });
    }

    pub fn initialize(&mut self, world: &mut crate::ecs::world::World) {
        for scheduled in &mut self.systems {
            scheduled.system.initialize(world);
        }
    }

    pub fn system_accesses(&self) -> impl Iterator<Item = (&'static str, &SystemAccess)> + '_ {
        self.systems
            .iter()
            .map(|scheduled| (scheduled.name, scheduled.system.access()))
    }

    pub fn run(&mut self, ctx: &mut SystemContext) {
        crate::profile_scope!("ecs.schedule");
        for scheduled in &mut self.systems {
            crate::profile_dynamic_scope!("ecs.system", format!("ecs.system.{}", scheduled.name));
            let this_run_tick = ctx.world.advance_change_tick();
            ctx.last_run_tick = scheduled.last_run_tick;
            ctx.this_run_tick = this_run_tick;
            scheduled.system.run(ctx);
            scheduled.last_run_tick = this_run_tick;
        }
    }
}

impl Default for Schedule {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::{
        event::{EventReader, EventWriter},
        query::Query,
        resource::{Res, ResMut},
        world::World,
    };

    #[derive(Default)]
    struct Counter(u32);

    struct Value(u32);

    struct Ping;

    #[test]
    fn parameterized_functions_register_without_type_annotations() {
        fn update(
            counter: Res<Counter>,
            mut values: Query<(&mut Value,)>,
            mut writer: EventWriter<Ping>,
            commands: &mut Commands,
        ) {
            let _ = counter.0;
            values.for_each(|_, (value,)| value.0 += 1);
            writer.send(Ping);
            let _ = commands.len();
        }

        fn observe(_events: EventReader<Ping>, _counter: ResMut<Counter>) {}

        let mut world = World::new();
        world.add_event::<Ping>();
        world.insert_opaque_resource(Counter::default());
        let mut schedule = Schedule::new();
        schedule.add_system(update);
        schedule.add_system(observe);
        schedule.initialize(&mut world);

        assert_eq!(schedule.system_accesses().count(), 2);
        assert!(
            schedule
                .system_accesses()
                .all(|(_, access)| !access.is_exclusive())
        );
    }
}
