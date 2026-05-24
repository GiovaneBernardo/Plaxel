use crate::ecs::{
    commands::Commands,
    system::{System, SystemContext},
};

pub struct Schedule {
    systems: Vec<System>,
}

impl Schedule {
    pub fn new() -> Self {
        Self {
            systems: Vec::new(),
        }
    }

    pub fn add_system(&mut self, system: impl FnMut(&mut SystemContext, &mut Commands) + 'static) {
        self.systems.push(Box::new(system));
    }

    pub fn run(&mut self, ctx: &mut SystemContext) {
        for system in &mut self.systems {
            let mut commands = Commands::new();
            system(ctx, &mut commands);
            commands.apply(ctx);
        }
    }
}
