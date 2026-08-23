use crate::{
    render::producers::planet_terrain_producer::PlanetTerrainProducerPlugin,
    systems::universe::star_system::create_star_system,
};
use engine::prelude::*;

pub struct UniversePlugin;
impl Plugin for UniversePlugin {
    fn build(&self, app: &mut engine::App) {
        app.add_plugin(PlanetTerrainProducerPlugin)
            .add_system(CoreSchedule::Startup, create_star_system);
    }
}
