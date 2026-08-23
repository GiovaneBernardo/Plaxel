use engine::prelude::*;
use engine::{
    model::{ModelVertex, TransformInstance, Vertex},
    renderer::{CullMode, material_passes},
};
use game_types::universe::StarSystemComponent;
use game_types::{octree::PlanetLodSettings, planet::Planet};

use crate::{
    GameCamera, GameState,
    systems::planet_system::{self, PendingPlanetMeshRequests},
};

pub fn create_star_system(
    asset_server: Res<AssetServer>,
    default_meshes: Res<DefaultMeshes>,
    mut camera: ResMut<GameCamera>,
    game_state: Res<GameState>,
    lod_settings: Res<PlanetLodSettings>,
    mut pending_mesh_requests: ResMut<PendingPlanetMeshRequests>,
    mut planets: Query<(&Planet,)>,
    mut camera_transforms: Query<(&mut TransformComponent,)>,
    commands: &mut Commands,
) {
    let star_entity = commands.spawn_empty().id();
    let mut star = StarSystemComponent {
        planets: Vec::new(),
        mass: 1e9,
        radius: 1e5,
        surface_temperature: 5000.0,
        luminosity: luminosity(1e5, 5000.0),
        emission_color: star_color(5000.0),
    };
    let mut occupied_planet_positions = Vec::new();
    planets.for_each(|_, (planet,)| occupied_planet_positions.push(planet.position));

    create_star_render_object(asset_server, default_meshes, star_entity, commands);

    // Create planets
    for i in 0..1 {
        let position = if i == 0 {
            Some(vec3(0.0, 0.0, 0.0))
        } else {
            None
        };

        if let Some(planet) = planet_system::create_planet(
            &mut camera,
            &game_state,
            &lod_settings,
            &mut pending_mesh_requests,
            &mut occupied_planet_positions,
            &mut camera_transforms,
            commands,
            star_entity,
            position,
            star.planets.len(),
        ) {
            star.planets.push(planet);
        }
    }

    commands.entity(star_entity).insert_bundle((
        star,
        TransformComponent {
            position: vec3(149.0 * 1_000_000_000.0, 1e6, 0.0),
            rotation: Quat::IDENTITY,
            scale: vec3(1e5, 1e5, 1e5),
            velocity: vec3(0.0, 0.0, 0.0),
        },
    ));
}

const SIGMA: f64 = 5.670_374_419e-8;

fn luminosity(radius_m: f64, temperature_k: f64) -> f64 {
    4.0 * std::f64::consts::PI * radius_m.powi(2) * SIGMA * temperature_k.powi(4)
}

fn star_color(temperature: f64) -> Vec3 {
    if temperature < 3000.0 {
        return vec3(255.0 / 255.0, 180.0 / 255.0, 107.0 / 255.0);
    } else if temperature < 4500.0 {
        return vec3(255.0 / 255.0, 219.0 / 255.0, 186.0 / 255.0);
    } else if temperature < 5772.0 {
        return vec3(255.0 / 255.0, 243.0 / 255.0, 239.0 / 255.0);
    } else if temperature < 8000.0 {
        return vec3(221.0 / 255.0, 229.0 / 255.0, 255.0 / 255.0);
    } else if temperature < 12000.0 {
        return vec3(191.0 / 255.0, 211.0 / 255.0, 255.0 / 255.0);
    } else if temperature < 30000.0 {
        return vec3(159.0 / 255.0, 190.0 / 255.0, 255.0 / 255.0);
    }
    vec3(0.0, 0.0, 0.0)
}

fn create_star_render_object(
    asset_server: Res<AssetServer>,
    default_meshes: Res<DefaultMeshes>,
    star_entity: Entity,
    commands: &mut Commands,
) {
    let star_material =
        Material::for_pass("shaders/star.wgsl".into(), material_passes::FORWARD_OPAQUE)
            .with_vertex_layouts(vec![ModelVertex::layout(), TransformInstance::layout()])
            .with_cull(CullMode::Front);
    let material_uuid = star_material.uuid;
    asset_server.add(star_material);
    let mesh = default_meshes.sphere;
    commands.entity(star_entity).insert(MeshRendererComponent {
        material: material_uuid,
        mesh,
    });
}
