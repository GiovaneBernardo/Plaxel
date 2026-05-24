use crate::{
    ecs::{commands::Commands, world::World},
    global_resources::GlobalResources,
};
pub struct SystemContext<'a> {
    pub world: &'a mut World,
    pub globals: &'a mut GlobalResources,
}

pub type System = Box<dyn FnMut(&mut SystemContext, &mut Commands)>;
