use engine::ecs::{commands::Commands, system::SystemContext};

use crate::systems::star_system::create_star_system;

pub fn create_galaxy(ctx: &mut SystemContext, commands: &mut Commands) {
    create_star_system(ctx, commands);
}
