use engine::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
};

use engine::{
    core::components::core::TransformComponent,
    ecs::{commands::Commands, query::Query, system::SystemContext},
    game_info,
    model::Vertex,
    multithreading::job_system::JobPriorityHandle,
    profile_scope,
    renderer::{AtmospherePassNode, DebugPassNode},
};
use engine::{
    ecs::entity::Entity,
    math::{Quat, Vec3, vec3},
};
use game_types::{
    clouds::CloudsComponent,
    octree::{
        FaceNeighbor, GeneratedMesh, GeneratedReplacement, NodeKey, NodeState, OctreeChanges,
        OctreeNode, PlanetLodSettings, PlanetMeshRequest,
    },
    planet::{Planet, PlanetTerrainEdits},
    terrain::{
        BiomeConfig, ClimateConfig, FeatureConfig, GeologyConfig, LandformConfig,
        PlanetTerrainConfig,
        terrain_field::{TerrainFieldGraph, TerrainGraphApplyQueue},
    },
};
use rand::Rng;
use rayon::prelude::*;
use web_time::{Duration, Instant};

use crate::{
    CHUNK_CELL_COUNT, GameCamera, GameState, octree,
    render::producers::planet_terrain_producer::{
        PendingChunkMesh, PlanetGenerationCommand, PlanetTerrainRenderQueue,
    },
    systems::{
        terrain::terrain_sampler::{self, PlanetTerrainSamplerContext, PlanetTerrainSnapshot},
        universe::PlanetExt,
    },
};

use crossbeam_channel::{Receiver, Sender};

pub type DensityGrid = Vec<f32>;
const CHUNK_GRID_SAMPLE_COUNT: u32 = CHUNK_CELL_COUNT as u32 + 2;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BaseGridCacheKey {
    planet_entity: Entity,
    generation: u64,
    node_key: NodeKey,
}

const MESH_UPLOAD_BUDGET: Duration = Duration::from_millis(6);
const CAMERA_ALTITUDE_LOG_INTERVAL: Duration = Duration::from_secs(1);
const TERRAIN_EDIT_BRICK_SIZE: f32 = 32.0;
const TERRAIN_EDIT_LEVEL: u32 = 0;

const PLANET_COUNT: usize = 128;
const PLANET_SPAWN_RANGE: f32 = 50_000_000.0;
const MAX_PLANET_SPAWN_ATTEMPTS: usize = 256;
const INITIAL_CAMERA_ALTITUDE: f32 = 32.0;

const INITIAL_CAMERA_DISTANCE_MULTIPLIER: f32 = 4.0;

pub fn default_planet_terrain_config() -> PlanetTerrainConfig {
    PlanetTerrainConfig {
        seed: 1,
        radius: 6_430_000.0,
        sea_level: 10.0,
        rotation_axis: vec3(0.0, 0.3987, 0.9171),
        field_graph: None,
        geology: GeologyConfig {
            definitions: Vec::new(),
            province_scale: 1.0,
            strata_scale: 1.0,
        },
        landforms: LandformConfig {
            continent_height: 50.0,
            continent_scale: 1.0,
            mountain_height: 500.0,
            mountain_width: 300.0,
        },
        climate: ClimateConfig {
            equator_temperature: 20.0,
            pole_temperature: -20.0,
            altitude_cooling: 1.0,
            humidity_scale: 1.0,
        },
        biomes: BiomeConfig {
            definitions: Vec::new(),
        },
        features: FeatureConfig {
            cave_frequency: 0.0,
            cave_size: 0.0,
            overhang_strength: 0.0,
        },
    }
}

pub fn earth_like_planet_terrain_config() -> PlanetTerrainConfig {
    terrain_config_from_graph(TerrainFieldGraph::default())
}

fn startup_planet_terrain_config() -> PlanetTerrainConfig {
    // Embed the startup preset so launching from another directory or on the web
    // uses the same graph before camera placement and initial octree generation.
    let graph = ron::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../planet_refined2.plxterrain"
    )))
    .expect("bundled planet_refined2.plxterrain must be a valid terrain graph");
    terrain_config_from_graph(graph)
}

fn terrain_config_from_graph(graph: TerrainFieldGraph) -> PlanetTerrainConfig {
    let mut config = default_planet_terrain_config();
    config.seed = graph.seed;
    config.radius = graph.radius as f32;
    config.sea_level = graph.sea_level as f32;
    config.field_graph = Some(graph);
    config
}

#[derive(plaxel_reflect::Reflect)]
struct CameraAltitudeLogState {
    #[reflect(ignore)]
    last_log: Option<Instant>,
    logs_emitted: u64,
}

fn random_planet_position(
    rng: &mut impl Rng,
    existing_positions: &[Vec3],
    min_distance: f32,
) -> Option<Vec3> {
    let min_distance_sq = min_distance * min_distance;
    let far_enough = |candidate: Vec3| {
        existing_positions
            .iter()
            .all(|position| (candidate - *position).length_squared() >= min_distance_sq)
    };

    for _ in 0..MAX_PLANET_SPAWN_ATTEMPTS {
        let candidate = vec3(
            rng.gen_range(-PLANET_SPAWN_RANGE..=PLANET_SPAWN_RANGE),
            rng.gen_range(-PLANET_SPAWN_RANGE..=PLANET_SPAWN_RANGE),
            rng.gen_range(-PLANET_SPAWN_RANGE..=PLANET_SPAWN_RANGE),
        );

        if far_enough(candidate) {
            return Some(candidate);
        }
    }

    const GRID_STEPS: i32 = 20;
    let step = (PLANET_SPAWN_RANGE * 2.0) / GRID_STEPS as f32;
    for x in 0..=GRID_STEPS {
        for y in 0..=GRID_STEPS {
            for z in 0..=GRID_STEPS {
                let candidate = vec3(
                    -PLANET_SPAWN_RANGE + x as f32 * step,
                    -PLANET_SPAWN_RANGE + y as f32 * step,
                    -PLANET_SPAWN_RANGE + z as f32 * step,
                );

                if far_enough(candidate) {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

pub fn planet_system_init(ctx: &mut SystemContext, _commands: &mut Commands) {
    profile_scope!("terrain.planet.init");
    let world = &mut ctx.world;
    let camera_entity = {
        let Some(camera) = world.get_resource::<GameCamera>() else {
            return;
        };
        camera.entity
    };

    let camera_pos = world
        .get::<TransformComponent>(camera_entity)
        .unwrap()
        .position;

    world.insert_resource(CameraAltitudeLogState {
        last_log: None,
        logs_emitted: 0,
    });
}

fn log_camera_altitude(ctx: &mut SystemContext, camera_pos: Vec3) {
    let now = Instant::now();
    let should_log = {
        let Some(mut state) = ctx.world.get_resource_mut::<CameraAltitudeLogState>() else {
            return;
        };
        if state
            .last_log
            .is_some_and(|last_log| now.duration_since(last_log) < CAMERA_ALTITUDE_LOG_INTERVAL)
        {
            false
        } else {
            state.last_log = Some(now);
            state.logs_emitted += 1;
            true
        }
    };
    if !should_log {
        return;
    }

    let mut nearest = None;
    let mut query =
        Query::<(&Planet, &PlanetTerrainEdits, &Arc<PlanetTerrainConfig>)>::new(&mut ctx.world);
    query.for_each(|_, (planet, terrain_edits, terrain_config)| {
        let altitude_above_sea = (camera_pos - planet.position).length() - terrain_config.radius;
        let distance_to_sea = altitude_above_sea.abs();
        if nearest
            .as_ref()
            .is_some_and(|(nearest_distance, _, _, _)| distance_to_sea >= *nearest_distance)
        {
            return;
        }

        let terrain = PlanetTerrainSamplerContext {
            config: terrain_config.as_ref(),
            edits: terrain_edits,
            planet_position: planet.position,
        };
        let altitude_above_terrain = terrain_sampler::sample_final_density(&terrain, camera_pos);
        let terrain_elevation = altitude_above_sea - altitude_above_terrain;
        nearest = Some((
            distance_to_sea,
            altitude_above_terrain,
            altitude_above_sea,
            terrain_elevation,
        ));
    });

    if let Some((_, altitude_above_terrain, altitude_above_sea, terrain_elevation)) = nearest {
        game_info!(
            "Camera altitude | ground: {:.1} m AGL | ocean: {:.1} m MSL | terrain elevation: {:+.1} m",
            altitude_above_terrain,
            altitude_above_sea,
            terrain_elevation,
        );
    }
}

pub fn create_planet(
    camera: &mut GameCamera,
    game_state: &GameState,
    lod_settings: &PlanetLodSettings,
    occupied_planet_positions: &mut Vec<Vec3>,
    camera_transforms: &mut Query<(&mut TransformComponent,)>,
    commands: &mut Commands,
    solar_system: Entity,
    forced_position: Option<Vec3>,
    planet_index: usize,
) -> Option<Entity> {
    profile_scope!("terrain.planet.create");
    let camera_entity = camera.entity;
    let mut camera_pos = camera_transforms.get(camera_entity)?.0.position;

    let terrain_config = Arc::new(if game_state.start_with_earth_like_terrain {
        startup_planet_terrain_config()
    } else {
        default_planet_terrain_config()
    });
    let chunk_size = 32;
    let min_planet_distance = terrain_config.radius * 2.1;
    let mut rng = rand::thread_rng();

    let Some(mut planet_position) =
        random_planet_position(&mut rng, occupied_planet_positions, min_planet_distance)
    else {
        return None;
    };

    if forced_position.is_some() {
        planet_position = forced_position.unwrap();
    }
    occupied_planet_positions.push(planet_position);
    let new_planet = commands.spawn_empty().id();

    let terrain_edits = PlanetTerrainEdits {
        modified_chunks: HashMap::new(),
        modified_ranges: HashMap::new(),
    };

    // Update camera position to follow planet
    if forced_position.is_some() {
        let spawn_direction = Vec3::Y;
        let terrain = PlanetTerrainSamplerContext {
            config: terrain_config.as_ref(),
            edits: &terrain_edits,
            planet_position,
        };
        let point_at_base_radius = planet_position + spawn_direction * terrain_config.radius;
        let surface_radius = terrain_config.radius
            - terrain_sampler::sample_original_density(&terrain, point_at_base_radius);
        camera_pos = planet_position
            + spawn_direction
                * (surface_radius * INITIAL_CAMERA_DISTANCE_MULTIPLIER + INITIAL_CAMERA_ALTITUDE);

        camera_pos = vec3(1287700.88, 6242394.00, 136645.75);
        let spawn_orientation = engine::camera::Camera::look_at(
            vec3(0.01, -1.0, 0.0).normalize(),
            vec3(0.0, 0.0, -1.0),
        );

        let (camera_transform,) = camera_transforms.get(camera_entity)?;
        camera_transform.position = camera_pos;
        camera_transform.rotation = spawn_orientation;

        camera.camera.position = camera_pos;
        camera.world_position = camera_pos.as_dvec3();
        camera.previous_world_position = camera.world_position;
        camera.camera.orientation = spawn_orientation;
        camera.velocity_sample_pos = camera_pos;
    }
    let lod_strength = lod_settings.strength;

    let surface_octree = {
        profile_scope!("terrain.planet.initial_octree");
        Planet::create_octree(
            planet_position,
            &vec3(camera_pos.x, camera_pos.y, camera_pos.z),
            terrain_config.as_ref(),
            chunk_size,
            lod_strength,
            &terrain_edits,
        )
    };

    let ocean_octree = {
        profile_scope!("terrain.planet.initial_ocean_octree");
        Planet::create_octree(
            planet_position,
            &vec3(camera_pos.x, camera_pos.y, camera_pos.z),
            terrain_config.as_ref(),
            chunk_size,
            lod_strength,
            &terrain_edits,
        )
    };

    let planet = Planet {
        id: new_planet.index() as u64,
        name: format!("Planet {planet_index}"),
        position: planet_position,
        surface_octree_root: surface_octree,
        ocean_octree_root: ocean_octree,
        solar_system,
    };

    let clouds = CloudsComponent {
        min_height: 500.0,
        max_height: 50000.0,
    };

    commands.entity(new_planet).insert_bundle((
        TransformComponent {
            position: planet_position,
            rotation: Quat::IDENTITY,
            scale: vec3(1.0, 1.0, 1.0),
            velocity: vec3(0.0, 0.0, 0.0),
        },
        planet,
        terrain_edits,
        terrain_config,
        clouds,
    ));

    //pending_mesh_requests.requests.extend(mesh_requests);
    Some(new_planet)
}

pub fn planet_system_update(
    asset_server: Res<AssetServer>,
    default_meshes: Res<DefaultMeshes>,
    mut camera: ResMut<GameCamera>,
    game_state: Res<GameState>,
    lod_settings: Res<PlanetLodSettings>,
    mut transforms: Query<(&mut TransformComponent,)>,
    mut planets_query: Query<(&mut Planet, &PlanetTerrainEdits, &Arc<PlanetTerrainConfig>)>,
    mut globals: GlobalsMut,
    commands: &mut Commands,
) {
    // profile_scope!("terrain.planet.update");
    // let camera_entity = camera.entity;

    // let update_octree = game_state.update_octree;

    // let mut camera_pos = transforms.get(camera_entity).unwrap().0.position;
    // //let heightmap = ctx
    // //    .world
    // //    .get_resource::<Arc<EarthHeightmap>>()
    // //    .map(|heightmap| Arc::clone(&heightmap));

    // let mut changes = Vec::new();
    // let lod_strength = lod_settings.strength;
    // let mut atmosphere_planet = None;
    // let mut debug_pass = globals
    //     .renderer
    //     .render_graph
    //     .get_node_mut::<DebugPassNode>(engine::renderer::ids::graph_passes::DEBUG);
    // if let Some(debug_pass) = debug_pass.as_mut() {
    //     debug_pass.clear_cubes();
    // }
    // {
    //     planets_query.for_each(|entity, (planet, terrain_edits, terrain_config)| {
    //         let change_start = changes.len();
    //         if update_octree {
    //             {
    //                 profile_scope!("terrain.planet.update_octree");
    //                 octree::update(
    //                     &mut planet.octree_root,
    //                     camera_pos,
    //                     entity,
    //                     planet.position,
    //                     terrain_config.as_ref(),
    //                     lod_strength,
    //                     &mut changes,
    //                     terrain_edits,
    //                 );
    //             }
    //         }

    //         //if let Some(debug_pass) = debug_pass.as_mut() {
    //         //    let mut leaves = Vec::new();
    //         //    octree::collect_leaf_nodes(&planet.octree_root, &mut leaves);
    //         //    for leaf in leaves {
    //         //        if !leaf.may_contain_surface {
    //         //            continue;
    //         //        }
    //         //        debug_pass.add_cube(
    //         //            leaf.min + Vec3::splat(leaf.size * 0.5),
    //         //            leaf.size,
    //         //            octree::depth_color(leaf.key.level as u32),
    //         //        );
    //         //    }
    //         //}

    //         if atmosphere_planet.is_none() {
    //             atmosphere_planet = Some((planet.clone(), terrain_config.radius));
    //         }
    //     });
    // }

    // // TODO: Reenable this (not sure what it does, might be to sync the sun direction when planet moves)
    // //if atmosphere_planet.is_some() {
    // //    let (plan, planet_radius) = atmosphere_planet.unwrap();
    // //    let planet_position = plan.position;
    // //    let sun_position = transforms.get(plan.solar_system).unwrap().0.position;
    // //
    // //    let settings = &mut ctx
    // //        .globals
    // //        .renderer
    // //        .render_graph
    // //        .get_node_mut::<AtmospherePassNode>(engine::renderer::ids::graph_passes::ATMOSPHERE)
    // //        .unwrap()
    // //        .settings;
    // //    settings.set_planet(planet_position.into(), planet_radius);
    // //    settings.sun_direction = (vec3(sun_position.x, sun_position.y, sun_position.z)
    // //        - vec3(planet_position.x, planet_position.y, planet_position.z))
    // //    .normalize()
    // //    .into();
    // //}
}

fn set_octree_node_state(node: &mut OctreeNode, key: NodeKey, state: NodeState) -> bool {
    if node.key == key {
        node.state = state;
        return true;
    }
    let Some(children) = node.children.as_mut() else {
        return false;
    };
    children
        .iter_mut()
        .any(|child| set_octree_node_state(child, key, state))
}

fn get_or_build_base_grid(
    key: BaseGridCacheKey,
    nx: u32,
    ny: u32,
    nz: u32,
    resolution: f32,
    min: Vec3,
    terrain: &PlanetTerrainSamplerContext<'_>,
    base_grid_cache: &Arc<Mutex<HashMap<BaseGridCacheKey, Arc<DensityGrid>>>>,
    cancelled: Option<&AtomicBool>,
) -> Option<Arc<DensityGrid>> {
    profile_scope!("terrain.mesh.base_grid_cache");
    {
        profile_scope!("terrain.mesh.lock_return");
        if let Some(grid) = base_grid_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&key).cloned())
        {
            return Some(grid);
        }
    }

    {
        profile_scope!("terrain.mesh.generate_base_grid_from_min");
        let node_size = nx.saturating_sub(2) as f32 * resolution;

        let generated_grid =
            generate_base_grid_from_min(nx, ny, nz, resolution, min, terrain, cancelled)?;
        let grid = Arc::new(generated_grid);

        {
            profile_scope!("terrain.mesh.cancelled");
            if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
                return None;
            }
        }

        {
            profile_scope!("terrain.mesh.base_grid_cache_lock");

            if let Ok(mut cache) = base_grid_cache.lock() {
                Some(
                    cache
                        .entry(key)
                        .or_insert_with(|| Arc::clone(&grid))
                        .clone(),
                )
            } else {
                Some(grid)
            }
        }
    }
}

fn generate_base_grid_from_min(
    nx: u32,
    ny: u32,
    nz: u32,
    resolution: f32,
    min: Vec3,
    terrain: &PlanetTerrainSamplerContext<'_>,
    cancelled: Option<&AtomicBool>,
) -> Option<DensityGrid> {
    profile_scope!("terrain.mesh.sample_base_density");
    let local_min = min.as_dvec3() - terrain.planet_position.as_dvec3();
    let resolution = f64::from(resolution);
    let mut grid = Vec::with_capacity((nx * ny * nz) as usize);
    for xi in 0..nx {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return None;
        }
        for yi in 0..ny {
            for zi in 0..nz {
                let position = local_min
                    + engine::math::dvec3(
                        f64::from(xi) * resolution,
                        f64::from(yi) * resolution,
                        f64::from(zi) * resolution,
                    );
                grid.push(terrain_sampler::sample_original_density_planet_local(
                    terrain, position,
                ));
            }
        }
    }
    Some(grid)
}

fn generate_grid_from_base(
    base_grid: &DensityGrid,
    resolution: f32,
    min: Vec3,
    terrain: &PlanetTerrainSamplerContext<'_>,
) -> DensityGrid {
    profile_scope!("terrain.mesh.sample_edit_density");
    let size = CHUNK_GRID_SAMPLE_COUNT as usize;
    debug_assert_eq!(base_grid.len(), size * size * size);
    let mut grid = Vec::with_capacity(base_grid.len());
    let local_min = min.as_dvec3() - terrain.planet_position.as_dvec3();
    let resolution = f64::from(resolution);

    for xi in 0..size {
        for yi in 0..size {
            for zi in 0..size {
                let position = local_min
                    + engine::math::dvec3(
                        xi as f64 * resolution,
                        yi as f64 * resolution,
                        zi as f64 * resolution,
                    );
                let index = (xi * size + yi) * size + zi;
                grid.push(
                    base_grid[index]
                        + terrain_sampler::sample_terrain_edits_density_planet_local(
                            terrain, position,
                        ),
                );
            }
        }
    }

    grid
}
