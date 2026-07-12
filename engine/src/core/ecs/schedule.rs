use crate::ecs::{
    commands::Commands,
    system::{HotSystem, StaticSystem, System, SystemContext},
};

struct ScheduledSystem {
    name: &'static str,
    system: System,
}

pub struct Schedule {
    systems: Vec<ScheduledSystem>,
}

impl Schedule {
    pub fn new() -> Self {
        Self {
            systems: Vec::new(),
        }
    }

    pub fn add_system<F>(&mut self, system: F)
    where
        F: FnMut(&mut SystemContext, &mut Commands) + 'static,
    {
        self.add_named_system(std::any::type_name::<F>(), system);
    }

    pub fn add_named_system(
        &mut self,
        name: &'static str,
        system: impl FnMut(&mut SystemContext, &mut Commands) + 'static,
    ) {
        self.systems.push(ScheduledSystem {
            name,
            system: Box::new(HotSystem::new(name, system)),
        });
    }

    pub fn add_static_system<F>(&mut self, system: F)
    where
        F: FnMut(&mut SystemContext, &mut Commands) + 'static,
    {
        self.add_static_named_system(std::any::type_name::<F>(), system);
    }

    pub fn add_static_named_system(
        &mut self,
        name: &'static str,
        system: impl FnMut(&mut SystemContext, &mut Commands) + 'static,
    ) {
        self.systems.push(ScheduledSystem {
            name,
            system: Box::new(StaticSystem::new(system)),
        });
    }

    pub fn run(&mut self, ctx: &mut SystemContext) {
        crate::profile_scope!("ecs.schedule");
        for scheduled in &mut self.systems {
            let _profile_scope =
                crate::profiling::Scope::new_owned(format!("ecs.system.{}", scheduled.name));
            let mut commands = Commands::new();
            scheduled.system.run(ctx, &mut commands);
            commands.apply(ctx);
        }
    }
}
