use crate::{
    render::producers::{
        planet_ocean_producer::PlanetOceanProducerPlugin,
        planet_terrain_producer::PlanetTerrainProducerPlugin,
    },
    systems::{
        self, planet_ocean_update::PlanetOceanMeshGeneration,
        planet_octree_update::PlanetMeshGeneration, universe::star_system::create_star_system,
    },
};
use engine::prelude::*;
use game_types::terrain::terrain_field::TerrainGraphApplyQueue;

pub struct UniversePlugin;
impl Plugin for UniversePlugin {
    fn build(&self, app: &mut engine::App) {
        app.add_plugin(TerrainPlugin)
            //.add_plugin(WaterPlugin)
            .add_system(CoreSchedule::Startup, create_star_system)
            .add_system(
                CoreSchedule::Update,
                systems::universe::planet_system_update,
            );
    }
}

pub struct TerrainPlugin;
impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut engine::App) {
        app.add_plugin(PlanetTerrainProducerPlugin)
            .init_resource::<PlanetMeshGeneration>()
            .init_resource::<TerrainGraphApplyQueue>()
            .add_system(
                CoreSchedule::Update,
                systems::planet_octree_update::drain_completed_requests,
            )
            .add_system(
                CoreSchedule::Update,
                systems::planet_octree_update::apply_terrain_graph_changes,
            )
            .add_system(
                CoreSchedule::Update,
                systems::planet_octree_update::planet_octree_update,
            );
    }
}

pub struct WaterPlugin;
impl Plugin for WaterPlugin {
    fn build(&self, app: &mut engine::App) {
        app.add_plugin(PlanetOceanProducerPlugin)
            .init_resource::<PlanetOceanMeshGeneration>()
            .add_system(
                CoreSchedule::Update,
                systems::planet_ocean_update::drain_completed_requests,
            )
            .add_system(
                CoreSchedule::Update,
                systems::planet_ocean_update::planet_ocean_octree_update,
            );
    }
}
