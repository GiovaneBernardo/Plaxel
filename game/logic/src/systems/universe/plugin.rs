use crate::{
    render::producers::planet_terrain_producer::PlanetTerrainProducerPlugin,
    systems::{
        self, planet_octree_update::PlanetMeshGeneration, universe::star_system::create_star_system,
    },
};
use engine::prelude::*;

pub struct UniversePlugin;
impl Plugin for UniversePlugin {
    fn build(&self, app: &mut engine::App) {
        app.add_plugin(PlanetTerrainProducerPlugin)
            .init_resource::<PlanetMeshGeneration>()
            .add_system(CoreSchedule::Startup, create_star_system)
            .add_system(
                CoreSchedule::Update,
                systems::universe::planet_system_update,
            )
            .add_system(
                CoreSchedule::Update,
                systems::planet_octree_update::drain_completed_requests,
            )
            .add_system(
                CoreSchedule::Update,
                systems::planet_octree_update::planet_octree_update,
            );
    }
}
