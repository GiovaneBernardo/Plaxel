use engine::ecs::{commands::Commands, system::SystemContext};

use crate::systems::galaxy_system;

pub fn universe_system_init(ctx: &mut SystemContext, commands: &mut Commands) {
    for i in 0..1 {
        galaxy_system::create_galaxy(ctx, commands);
    }
}
