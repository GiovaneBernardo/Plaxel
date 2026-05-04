use crate::ecs::{commands::Commands, world::World};
pub type System = Box<dyn FnMut(&mut World, &mut Commands)>;
