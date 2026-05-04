use crate::ecs::{commands::Commands, system::System, world::World};

pub struct Schedule {
    systems: Vec<System>,
}

impl Schedule {
    pub fn new() -> Self {
        Self {
            systems: Vec::new(),
        }
    }

    pub fn add_system(&mut self, system: impl FnMut(&mut World, &mut Commands) + 'static) {
        self.systems.push(Box::new(system));
    }

    pub fn run(&mut self, world: &mut World) {
        for system in &mut self.systems {
            let mut commands = Commands::new();
            system(world, &mut commands);
            commands.apply(world);
        }
    }
}
