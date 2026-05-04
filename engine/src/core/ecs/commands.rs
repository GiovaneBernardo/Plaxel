use crate::ecs::{component::Component, entity::Entity, world::World};

pub trait Command {
    fn apply(self: Box<Self>, world: &mut World);
}

impl<F> Command for F
where
    F: FnOnce(&mut World),
{
    fn apply(self: Box<Self>, world: &mut World) {
        self(world);
    }
}

pub struct Commands {
    queue: Vec<Box<dyn Command>>,
}

impl Commands {
    pub fn new() -> Self {
        Self { queue: Vec::new() }
    }

    pub fn push(&mut self, command: impl FnOnce(&mut World) + 'static) {
        self.queue.push(Box::new(command));
    }

    pub fn spawn(&mut self) {
        self.push(|world: &mut World| {
            world.spawn();
        });
    }

    pub fn insert<T: Component>(&mut self, entity: Entity, component: T) {
        self.push(move |world: &mut World| {
            world.insert(entity, component);
        });
    }

    pub fn apply(&mut self, world: &mut World) {
        for command in self.queue.drain(..) {
            command.apply(world);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }
}

impl Default for Commands {
    fn default() -> Self {
        Self::new()
    }
}
