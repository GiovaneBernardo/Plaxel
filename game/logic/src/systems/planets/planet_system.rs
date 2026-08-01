use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    thread,
};

use engine::{
    core::components::core::TransformComponent,
    ecs::{commands::Commands, query::Query, system::SystemContext},
    game_info,
    multithreading::job_system::JobPriorityHandle,
    renderer::{AtmospherePassNode, RenderData, RenderFlags, RenderObject, RenderObjectId},
};
use engine::{
    ecs::entity::Entity,
    math::{Quat, Vec3, vec3},
};
use game_types::{
    octree::{
        FaceNeighbor, GeneratedMesh, GeneratedReplacement, NodeKey, NodeState, OctreeChanges,
        OctreeNode, PlanetLodSettings, PlanetMeshRequest,
    },
    planet::{Planet, PlanetTerrainEdits},
    terrain::{
        BiomeConfig, ClimateConfig, FeatureConfig, GeologyConfig, LandformConfig,
        PlanetTerrainConfig,
    },
    universe::StarSystemComponent,
};
use rand::Rng;
use rayon::prelude::*;
use web_time::{Duration, Instant};

use crate::{
    CHUNK_CELL_COUNT, GameCamera, GameState, octree,
    systems::{
        planets::PlanetExt,
        terrain::terrain_sampler::{self, PlanetTerrainSamplerContext, PlanetTerrainSnapshot},
    },
};

use crossbeam_channel::{Receiver, Sender};

type DensityGrid = Vec<Vec<Vec<f32>>>;
const CHUNK_GRID_SAMPLE_COUNT: u32 = CHUNK_CELL_COUNT as u32 + 2;

fn retain_render_data(
    renderer: &mut engine::renderer::Renderer,
    render_data: RenderData,
    shadow_pipeline: engine::renderer::PipelineHandle,
) -> RenderObjectId {
    let material_index = render_data.material.material_index;
    let pipeline = render_data.pipeline;
    renderer.objects().insert(
        RenderObject::new(
            render_data.mesh,
            render_data.material,
            engine::model::TransformInstance {
                model_matrix: engine::math::Mat4::IDENTITY.to_cols_array_2d(),
                material_index,
            },
        )
        .with_flags(
            RenderFlags::VISIBLE_MAIN
                | RenderFlags::DEPTH_PREPASS
                | RenderFlags::CASTS_SHADOWS
                | RenderFlags::RECEIVES_SHADOWS,
        )
        .with_pipeline_override(engine::renderer::material_passes::FORWARD_OPAQUE, pipeline)
        .with_pipeline_override(engine::renderer::material_passes::SHADOW, shadow_pipeline)
        .with_bind_groups(render_data.extra_bind_groups),
    )
}

pub struct MeshJobResults {
    pub sender: Sender<GeneratedMesh>,
    pub receiver: Receiver<GeneratedMesh>,
    pub wanted: HashSet<NodeKey>,
    pub in_flight: HashSet<NodeKey>,
    pub in_flight_counts: HashMap<NodeKey, usize>,
    pub versions: HashMap<NodeKey, u64>,
    pub pending_requests: HashMap<NodeKey, PendingMeshRequest>,
    pub base_grid_cache: Arc<Mutex<HashMap<NodeKey, Arc<DensityGrid>>>>,
    pub ready_meshes: Vec<GeneratedMesh>,
    pub replacement_sender: Sender<GeneratedReplacement>,
    pub replacement_receiver: Receiver<GeneratedReplacement>,
    pub ready_replacements: Vec<GeneratedReplacement>,
    pub next_replacement_id: u64,
    pub prioritized_jobs: Vec<PrioritizedMeshJob>,
}

pub struct PrioritizedMeshJob {
    pub handle: JobPriorityHandle,
    pub target: MeshPriorityTarget,
    pub requests: Vec<PlanetMeshRequest>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MeshPriorityTarget {
    Single { key: NodeKey, version: u64 },
    Replacement(u64),
}

pub struct PendingMeshRequest {
    pub request: PlanetMeshRequest,
    pub urgent: bool,
}

const MESH_UPLOAD_BUDGET: Duration = Duration::from_millis(6);
const CAMERA_ALTITUDE_LOG_INTERVAL: Duration = Duration::from_secs(1);
const TERRAIN_EDIT_BRICK_SIZE: f32 = 32.0;
const TERRAIN_EDIT_LEVEL: u32 = 0;

const PLANET_COUNT: usize = 128;
const PLANET_SPAWN_RANGE: f32 = 50_000_000.0;
const MAX_PLANET_SPAWN_ATTEMPTS: usize = 256;
const INITIAL_CAMERA_ALTITUDE: f32 = 256.0;

pub fn default_planet_terrain_config() -> PlanetTerrainConfig {
    PlanetTerrainConfig {
        seed: 1,
        radius: 6_430_000.0,
        sea_level: 10.0,
        rotation_axis: vec3(0.0, 0.3987, 0.9171),
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

struct CameraAltitudeLogState {
    last_log: Option<Instant>,
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

    let (mesh_tx, mesh_rx) = crossbeam_channel::unbounded();
    let (replacement_tx, replacement_rx) = crossbeam_channel::unbounded();
    let base_grid_cache = Arc::new(Mutex::new(HashMap::new()));
    world.insert_resource(MeshJobResults {
        sender: mesh_tx,
        receiver: mesh_rx,
        wanted: HashSet::new(),
        in_flight: HashSet::new(),
        in_flight_counts: HashMap::new(),
        versions: HashMap::new(),
        pending_requests: HashMap::new(),
        base_grid_cache,
        ready_meshes: Vec::new(),
        replacement_sender: replacement_tx,
        replacement_receiver: replacement_rx,
        ready_replacements: Vec::new(),
        next_replacement_id: 0,
        prioritized_jobs: Vec::new(),
    });
    world.insert_resource(CameraAltitudeLogState { last_log: None });
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
    ctx: &mut SystemContext,
    _commands: &mut Commands,
    solar_system: Entity,
    forced_position: Option<Vec3>,
) -> Option<Entity> {
    let world = &mut ctx.world;

    let camera_entity = {
        let Some(camera) = world.get_resource::<GameCamera>() else {
            return None;
        };
        camera.entity
    };

    let mut camera_pos = world
        .get::<TransformComponent>(camera_entity)
        .unwrap()
        .position;

    let terrain_config = Arc::new(default_planet_terrain_config());
    let chunk_size = 32;
    let min_planet_distance = terrain_config.radius * 2.1;
    let mut rng = rand::thread_rng();

    let mut planet_positions = Vec::new();
    {
        let mut query = Query::<(&Planet,)>::new(world);
        query.for_each(|_, (planet,)| {
            planet_positions.push(planet.position);
        });
    }

    let Some(mut planet_position) =
        random_planet_position(&mut rng, &planet_positions, min_planet_distance)
    else {
        return None;
    };

    if forced_position.is_some() {
        planet_position = forced_position.unwrap();
    }
    planet_positions.push(planet_position);
    let new_planet = world.spawn();

    world.insert(
        new_planet,
        TransformComponent {
            position: planet_position,
            rotation: Quat::IDENTITY,
            scale: vec3(1.0, 1.0, 1.0),
            velocity: vec3(0.0, 0.0, 0.0),
        },
    );

    let terrain_edits = PlanetTerrainEdits {
        modified_chunks: HashMap::new(),
        modified_ranges: HashMap::new(),
    };

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
        camera_pos = planet_position + spawn_direction * (surface_radius + INITIAL_CAMERA_ALTITUDE);
        let spawn_orientation = engine::camera::Camera::look_at(
            vec3(0.01, -1.0, 0.0).normalize(),
            vec3(0.0, 0.0, -1.0),
        );

        let mut camera_transform = world.get_mut::<TransformComponent>(camera_entity).unwrap();
        camera_transform.position = camera_pos;
        camera_transform.rotation = spawn_orientation;
        drop(camera_transform);
        if let Some(mut camera) = world.get_resource_mut::<GameCamera>() {
            camera.camera.position = camera_pos;
            camera.camera.orientation = spawn_orientation;
            camera.velocity_sample_pos = camera_pos;
        }
    }
    let lod_strength = world
        .get_resource::<PlanetLodSettings>()
        .map_or(1.0, |settings| settings.strength);

    let octree = Planet::create_octree(
        planet_position,
        &vec3(camera_pos.x, camera_pos.y, camera_pos.z),
        terrain_config.as_ref(),
        chunk_size,
        lod_strength,
        &terrain_edits,
    );

    let planet = Planet {
        id: new_planet.index() as u64,
        name: format!(
            "Planet {}",
            world
                .get::<StarSystemComponent>(solar_system)
                .unwrap()
                .planets
                .len()
        ),
        position: planet_position,
        octree_root: octree,
        solar_system,
    };
    let mut leaf_nodes = Vec::new();
    octree::collect_leaf_nodes(&planet.octree_root, &mut leaf_nodes);

    let mut mesh_requests = Vec::new();

    for leaf in leaf_nodes {
        let mut request = PlanetMeshRequest {
            planet_entity: new_planet,
            node_key: leaf.key,
            planet_position: planet.position,
            node_min_corner: leaf.min,
            node_size: leaf.size,
            face_neighbors: [FaceNeighbor::SAME_OR_ABSENT; 6],
        };
        octree::annotate_mesh_request(&planet.octree_root, &mut request);
        mesh_requests.push(request);
    }

    ctx.world.insert(new_planet, planet);
    ctx.world.insert(new_planet, terrain_edits);
    ctx.world.insert(new_planet, terrain_config);

    for request in mesh_requests {
        submit_requested_mesh(ctx, request);
    }
    Some(new_planet)
}

pub fn planet_system_update(ctx: &mut SystemContext, _commands: &mut Commands) {
    let camera_entity = {
        let Some(camera) = ctx.world.get_resource::<GameCamera>() else {
            return;
        };
        camera.entity
    };

    let camera_pos = ctx
        .world
        .get::<TransformComponent>(camera_entity)
        .unwrap()
        .position;
    log_camera_altitude(ctx, camera_pos);
    //let heightmap = ctx
    //    .world
    //    .get_resource::<Arc<EarthHeightmap>>()
    //    .map(|heightmap| Arc::clone(&heightmap));

    let mut changes = Vec::new();
    let lod_strength = ctx
        .world
        .get_resource::<PlanetLodSettings>()
        .map_or(1.0, |settings| settings.strength);
    let mut atmosphere_planet = None;
    {
        let mut query = Query::<(&mut Planet, &PlanetTerrainEdits, &Arc<PlanetTerrainConfig>)>::new(
            &mut ctx.world,
        );
        query.for_each(|entity, (planet, terrain_edits, terrain_config)| {
            let change_start = changes.len();
            // Keep one atomic topology generation in flight per planet. A
            // second generation could otherwise invalidate a shared boundary
            // mesh while leaving the first generation's nodes transitional.
            if !octree::has_pending_transition(&planet.octree_root) {
                octree::update(
                    &mut planet.octree_root,
                    camera_pos,
                    entity,
                    planet.position,
                    terrain_config.as_ref(),
                    lod_strength,
                    &mut changes,
                    terrain_edits,
                );
            }

            let planet_changes: Vec<_> = changes.drain(change_start..).collect();
            let mut transitions = Vec::new();
            let mut keys_to_remove = Vec::new();
            let mut requests = Vec::new();
            let mut passthrough_changes = Vec::new();
            for change in planet_changes {
                match change {
                    OctreeChanges::ReplaceMeshes {
                        transition_key,
                        completed_state,
                        additional_transitions,
                        keys_to_remove: removed,
                        requests: replacement_requests,
                        ..
                    } => {
                        transitions.push((transition_key, completed_state));
                        transitions.extend(additional_transitions);
                        keys_to_remove.extend(removed);
                        requests.extend(replacement_requests);
                    }
                    other => passthrough_changes.push(other),
                }
            }

            let mut scheduled_keys: HashSet<NodeKey> =
                requests.iter().map(mesh_request_key).collect();
            for &(key, _) in &transitions {
                let min = vec3(key.x as f32, key.y as f32, key.z as f32);
                let size = planet.octree_root.size / 2.0_f32.powi(i32::from(key.level));
                let mut neighbors = Vec::new();
                octree::collect_face_neighbor_leaves(
                    &planet.octree_root,
                    min,
                    size,
                    &mut neighbors,
                );
                for neighbor in neighbors {
                    if !neighbor.has_surface || !scheduled_keys.insert(neighbor.key) {
                        continue;
                    }
                    let mut request = PlanetMeshRequest {
                        planet_entity: entity,
                        node_key: neighbor.key,
                        planet_position: planet.position,
                        node_min_corner: neighbor.min,
                        node_size: neighbor.size,
                        face_neighbors: [FaceNeighbor::SAME_OR_ABSENT; 6],
                    };
                    octree::annotate_mesh_request(&planet.octree_root, &mut request);
                    requests.push(request);
                }
            }

            for request in &mut requests {
                octree::annotate_mesh_request(&planet.octree_root, request);
            }
            keys_to_remove.sort_unstable();
            keys_to_remove.dedup();
            if let Some((transition_key, completed_state)) = transitions.first().copied() {
                changes.push(OctreeChanges::ReplaceMeshes {
                    planet_entity: entity,
                    transition_key,
                    completed_state,
                    additional_transitions: transitions[1..].to_vec(),
                    keys_to_remove,
                    requests,
                });
            }
            for mut change in passthrough_changes {
                if let OctreeChanges::AddMesh { request } = &mut change {
                    octree::annotate_mesh_request(&planet.octree_root, request);
                }
                changes.push(change);
            }

            if atmosphere_planet.is_none() {
                atmosphere_planet = Some(planet.clone());
            }
        });
    }

    if atmosphere_planet.is_some() {
        let plan = atmosphere_planet.unwrap();
        let planet_position = plan.position;
        let sun_position = ctx
            .world
            .get::<TransformComponent>(plan.solar_system)
            .unwrap()
            .position;

        ctx.globals
            .renderer
            .render_graph
            .get_node_mut::<AtmospherePassNode>(engine::renderer::ids::graph_passes::ATMOSPHERE)
            .unwrap()
            .settings
            .sun_direction = (vec3(sun_position.x, sun_position.y, sun_position.z)
            - vec3(planet_position.x, planet_position.y, planet_position.z))
        .normalize()
        .into();
    }

    changes.sort_by(|a, b| {
        let size_a = change_node_size(a);
        let size_b = change_node_size(b);

        // Smaller chunks—deeper octree levels—first.
        size_a.total_cmp(&size_b)
    });

    for change in changes {
        apply_change(ctx, &change);
    }

    drain_generated_meshes(ctx);
    reprioritize_mesh_jobs(ctx, camera_pos);
}

fn change_node_size(change: &OctreeChanges) -> f32 {
    match change {
        OctreeChanges::ReplaceMeshes { requests, .. } => requests
            .first()
            .map_or(f32::MAX, |request| request.node_size),

        OctreeChanges::AddMesh { request } => request.node_size,

        OctreeChanges::RemoveMeshes { .. } => f32::MAX,
    }
}

pub fn build_requested_mesh(
    request: PlanetMeshRequest,
    version: u64,
    urgent: bool,
    terrain: &PlanetTerrainSamplerContext<'_>,
    base_grid_cache: Arc<Mutex<HashMap<NodeKey, Arc<DensityGrid>>>>,
) -> GeneratedMesh {
    let size = request.node_size;
    let min_corner = vec3(
        request.node_min_corner.x,
        request.node_min_corner.y,
        request.node_min_corner.z,
    );

    let resolution = size / CHUNK_CELL_COUNT as f32;
    let key = NodeKey {
        x: min_corner.x as i32,
        y: min_corner.y as i32,
        z: min_corner.z as i32,
        level: request.node_key.level,
    };
    let base_grid = get_or_build_base_grid(
        key,
        CHUNK_GRID_SAMPLE_COUNT,
        CHUNK_GRID_SAMPLE_COUNT,
        CHUNK_GRID_SAMPLE_COUNT,
        resolution,
        min_corner,
        terrain,
        &base_grid_cache,
    );
    let grid;
    let grid_ref = if terrain_sampler::is_terrain_edits_empty(terrain) {
        base_grid.as_ref()
    } else {
        grid = generate_grid_from_base(base_grid.as_ref(), resolution, min_corner, terrain);
        &grid
    };
    let (vertices, indices) = Planet::dual_contour_grid(
        grid_ref,
        min_corner,
        resolution,
        terrain,
        &request.face_neighbors,
    );

    GeneratedMesh {
        key,
        version,
        urgent,
        vertices,
        indices,
    }
}

fn edits_for_mesh_request(
    terrain_edits: &PlanetTerrainEdits,
    request: &PlanetMeshRequest,
) -> PlanetTerrainEdits {
    if terrain_edits.modified_chunks.is_empty() {
        return PlanetTerrainEdits {
            modified_chunks: HashMap::new(),
            modified_ranges: HashMap::new(),
        };
    }

    let mesh_sample_spacing = request
        .face_neighbors
        .iter()
        .filter(|neighbor| neighbor.size > request.node_size)
        .fold(request.node_size, |size, neighbor| size.max(neighbor.size))
        / CHUNK_CELL_COUNT as f32;
    let margin = mesh_sample_spacing * 2.0 + TERRAIN_EDIT_BRICK_SIZE;
    let local_min =
        request.node_min_corner - request.planet_position - vec3(margin, margin, margin);
    let local_max = request.node_min_corner - request.planet_position
        + vec3(request.node_size, request.node_size, request.node_size)
        + vec3(margin, margin, margin);
    let mut relevant_chunks = HashMap::new();
    let mut relevant_ranges = HashMap::new();

    for (key, brick) in &terrain_edits.modified_chunks {
        if key.level != TERRAIN_EDIT_LEVEL {
            continue;
        }

        let brick_min = vec3(
            key.x as f32 * TERRAIN_EDIT_BRICK_SIZE,
            key.y as f32 * TERRAIN_EDIT_BRICK_SIZE,
            key.z as f32 * TERRAIN_EDIT_BRICK_SIZE,
        );
        let brick_max = brick_min
            + vec3(
                TERRAIN_EDIT_BRICK_SIZE,
                TERRAIN_EDIT_BRICK_SIZE,
                TERRAIN_EDIT_BRICK_SIZE,
            );

        let overlaps = brick_min.x <= local_max.x
            && brick_max.x >= local_min.x
            && brick_min.y <= local_max.y
            && brick_max.y >= local_min.y
            && brick_min.z <= local_max.z
            && brick_max.z >= local_min.z;

        if overlaps {
            relevant_chunks.insert(*key, Arc::clone(brick));
            if let Some(range) = terrain_edits.modified_ranges.get(key) {
                relevant_ranges.insert(*key, *range);
            }
        }
    }

    PlanetTerrainEdits {
        modified_chunks: relevant_chunks,
        modified_ranges: relevant_ranges,
    }
}

fn mesh_request_key(request: &PlanetMeshRequest) -> NodeKey {
    NodeKey {
        x: request.node_min_corner.x as i32,
        y: request.node_min_corner.y as i32,
        z: request.node_min_corner.z as i32,
        level: request.node_key.level,
    }
}

pub(crate) fn submit_requested_mesh(ctx: &mut SystemContext, request: PlanetMeshRequest) {
    submit_requested_mesh_internal(ctx, request, false);
}

pub(crate) fn submit_requested_mesh_urgent(ctx: &mut SystemContext, request: PlanetMeshRequest) {
    submit_requested_mesh_internal(ctx, request, true);
}

fn replacement_locks_key(mesh_jobs: &MeshJobResults, key: NodeKey) -> bool {
    mesh_jobs.prioritized_jobs.iter().any(|job| {
        matches!(job.target, MeshPriorityTarget::Replacement(_))
            && job
                .requests
                .iter()
                .any(|request| mesh_request_key(request) == key)
    }) || mesh_jobs
        .ready_replacements
        .iter()
        .any(|replacement| replacement.meshes.iter().any(|mesh| mesh.key == key))
}

fn submit_requested_mesh_internal(
    ctx: &mut SystemContext,
    request: PlanetMeshRequest,
    urgent: bool,
) {
    let key = mesh_request_key(&request);

    let (sender, version, base_grid_cache) = {
        let Some(mut mesh_jobs) = ctx.world.get_resource_mut::<MeshJobResults>() else {
            return;
        };

        mesh_jobs.wanted.insert(key);
        if replacement_locks_key(&mesh_jobs, key) {
            mesh_jobs
                .pending_requests
                .entry(key)
                .and_modify(|pending| {
                    pending.request = request;
                    pending.urgent |= urgent;
                })
                .or_insert(PendingMeshRequest { request, urgent });
            return;
        }

        let version = mesh_jobs.versions.entry(key).or_insert(0);
        *version += 1;
        let version = *version;

        if mesh_jobs.in_flight.contains(&key) && !urgent {
            mesh_jobs
                .pending_requests
                .entry(key)
                .and_modify(|pending| {
                    pending.request = request;
                    pending.urgent |= urgent;
                })
                .or_insert(PendingMeshRequest { request, urgent });
            return;
        }

        mesh_jobs.in_flight.insert(key);
        *mesh_jobs.in_flight_counts.entry(key).or_insert(0) += 1;

        (
            mesh_jobs.sender.clone(),
            version,
            Arc::clone(&mesh_jobs.base_grid_cache),
        )
    };

    start_mesh_job(ctx, sender, request, version, urgent, base_grid_cache);
}

fn start_mesh_job(
    ctx: &mut SystemContext,
    sender: Sender<GeneratedMesh>,
    request: PlanetMeshRequest,
    version: u64,
    urgent: bool,
    base_grid_cache: Arc<Mutex<HashMap<NodeKey, Arc<DensityGrid>>>>,
) {
    //let heightmap = ctx
    //    .world
    //    .get_resource::<Arc<EarthHeightmap>>()
    //    .map(|heightmap| Arc::clone(&heightmap));

    let terrain_edits = {
        let terrain_edits = ctx
            .world
            .get::<PlanetTerrainEdits>(request.planet_entity)
            .unwrap();
        edits_for_mesh_request(&terrain_edits, &request)
    };

    let terrain_config = {
        let terrain_config = ctx
            .world
            .get::<Arc<PlanetTerrainConfig>>(request.planet_entity)
            .unwrap();
        Arc::clone(&terrain_config)
    };

    let terrain = PlanetTerrainSnapshot {
        config: terrain_config,
        edits: terrain_edits,
        planet_position: request.planet_position,
    };

    let job = move || {
        let sampler = terrain.sampler_context();
        let mesh = build_requested_mesh(request, version, urgent, &sampler, base_grid_cache);
        let _ = sender.send(mesh);
    };

    if urgent {
        let _ = thread::Builder::new()
            .name("urgent-terrain-mesh".to_string())
            .spawn(job);
    } else {
        let priority = mesh_job_priority(ctx, &[request]);
        if let Ok(handle) = ctx.globals.job_system.spawn_prioritized(priority, job) {
            let mut mesh_jobs = ctx.world.get_resource_mut::<MeshJobResults>().unwrap();
            mesh_jobs.prioritized_jobs.push(PrioritizedMeshJob {
                handle,
                target: MeshPriorityTarget::Single {
                    key: mesh_request_key(&request),
                    version,
                },
                requests: vec![request],
            });
        }
    }
}

pub fn drain_generated_meshes(ctx: &mut SystemContext) {
    loop {
        let replacement = {
            let Some(mesh_jobs) = ctx.world.get_resource::<MeshJobResults>() else {
                return;
            };
            match mesh_jobs.replacement_receiver.try_recv() {
                Ok(replacement) => replacement,
                Err(_) => break,
            }
        };

        let mut mesh_jobs = ctx.world.get_resource_mut::<MeshJobResults>().unwrap();
        mesh_jobs.prioritized_jobs.retain(|job| {
            job.target != MeshPriorityTarget::Replacement(replacement.replacement_id)
        });
        for mesh in &replacement.meshes {
            let remaining = mesh_jobs
                .in_flight_counts
                .get_mut(&mesh.key)
                .map(|count| {
                    *count = count.saturating_sub(1);
                    *count
                })
                .unwrap_or(0);
            if remaining == 0 {
                mesh_jobs.in_flight_counts.remove(&mesh.key);
                mesh_jobs.in_flight.remove(&mesh.key);
            }
        }

        let still_current = replacement.meshes.iter().all(|mesh| {
            mesh_jobs.wanted.contains(&mesh.key)
                && mesh_jobs
                    .versions
                    .get(&mesh.key)
                    .is_some_and(|version| *version == mesh.version)
        });
        if !still_current {
            let retry = (
                replacement.planet_entity,
                replacement.transition_key,
                replacement.completed_state,
                replacement.additional_transitions.clone(),
                replacement.keys_to_remove.clone(),
                replacement.requests.clone(),
            );
            drop(mesh_jobs);
            submit_replacement(ctx, retry.0, retry.1, retry.2, retry.3, retry.4, retry.5);
            continue;
        }
        mesh_jobs.ready_replacements.push(replacement);
    }

    loop {
        let mesh = {
            let Some(mesh_jobs) = ctx.world.get_resource::<MeshJobResults>() else {
                return;
            };

            match mesh_jobs.receiver.try_recv() {
                Ok(mesh) => mesh,
                Err(_) => break,
            }
        };

        let (still_wanted, next_request) = {
            let Some(mut mesh_jobs) = ctx.world.get_resource_mut::<MeshJobResults>() else {
                return;
            };

            mesh_jobs.prioritized_jobs.retain(|job| {
                job.target
                    != MeshPriorityTarget::Single {
                        key: mesh.key,
                        version: mesh.version,
                    }
            });

            let remaining_in_flight =
                if let Some(count) = mesh_jobs.in_flight_counts.get_mut(&mesh.key) {
                    *count = count.saturating_sub(1);
                    *count
                } else {
                    0
                };
            if remaining_in_flight == 0 {
                mesh_jobs.in_flight_counts.remove(&mesh.key);
                mesh_jobs.in_flight.remove(&mesh.key);
            }

            let still_wanted = mesh_jobs.wanted.contains(&mesh.key)
                && mesh_jobs
                    .versions
                    .get(&mesh.key)
                    .is_some_and(|version| *version == mesh.version);

            let next_request = if remaining_in_flight == 0 {
                mesh_jobs
                    .pending_requests
                    .remove(&mesh.key)
                    .and_then(|pending| {
                        if !mesh_jobs.wanted.contains(&mesh.key) {
                            return None;
                        }

                        mesh_jobs.in_flight.insert(mesh.key);
                        *mesh_jobs.in_flight_counts.entry(mesh.key).or_insert(0) += 1;
                        let version = *mesh_jobs.versions.get(&mesh.key)?;
                        Some((
                            mesh_jobs.sender.clone(),
                            pending.request,
                            version,
                            pending.urgent,
                            Arc::clone(&mesh_jobs.base_grid_cache),
                        ))
                    })
            } else {
                None
            };

            (still_wanted, next_request)
        };

        if let Some((sender, request, version, urgent, base_grid_cache)) = next_request {
            start_mesh_job(ctx, sender, request, version, urgent, base_grid_cache);
        }

        if still_wanted {
            let Some(mut mesh_jobs) = ctx.world.get_resource_mut::<MeshJobResults>() else {
                return;
            };
            mesh_jobs.ready_meshes.push(mesh);
        }
    }

    let start = Instant::now();

    loop {
        if start.elapsed() >= MESH_UPLOAD_BUDGET {
            break;
        }

        let replacement = {
            let Some(mut mesh_jobs) = ctx.world.get_resource_mut::<MeshJobResults>() else {
                return;
            };
            if mesh_jobs.ready_replacements.is_empty() {
                break;
            }
            mesh_jobs.ready_replacements.remove(0)
        };

        let still_current = {
            let mesh_jobs = ctx.world.get_resource::<MeshJobResults>().unwrap();
            replacement.meshes.iter().all(|mesh| {
                mesh_jobs.wanted.contains(&mesh.key)
                    && mesh_jobs
                        .versions
                        .get(&mesh.key)
                        .is_some_and(|version| *version == mesh.version)
            })
        };
        if !still_current {
            submit_replacement(
                ctx,
                replacement.planet_entity,
                replacement.transition_key,
                replacement.completed_state,
                replacement.additional_transitions.clone(),
                replacement.keys_to_remove.clone(),
                replacement.requests.clone(),
            );
            continue;
        }

        let planet_entity = replacement.planet_entity;
        let transition_key = replacement.transition_key;
        let completed_state = replacement.completed_state;
        let additional_transitions = replacement.additional_transitions.clone();
        let replacement_mesh_keys: Vec<NodeKey> =
            replacement.meshes.iter().map(|mesh| mesh.key).collect();
        let mut keys_to_clear = replacement.keys_to_remove.clone();
        keys_to_clear.extend(replacement_mesh_keys.iter().copied());
        keys_to_clear.sort_unstable();
        keys_to_clear.dedup();

        // Create every new GPU mesh before changing the visible mesh map.
        let mut uploaded = Vec::new();
        for mesh in replacement.meshes {
            if mesh.indices.is_empty() {
                continue;
            }
            let (
                solid_material,
                solid_pipeline,
                solid_shadow_pipeline,
                terrain_materials_bind_group,
            ) = {
                let game_state = ctx.world.get_resource::<GameState>().unwrap();
                (
                    game_state.solid_material.clone(),
                    game_state.solid_pipeline,
                    game_state.solid_shadow_pipeline,
                    game_state.terrain_materials_bind_group,
                )
            };
            let vertex_bytes: Vec<u8> = bytemuck::cast_slice(&mesh.vertices).to_vec();
            let mut render_data = ctx.globals.renderer.renderer_api.create_render_data(
                &vertex_bytes,
                &mesh.indices,
                solid_material,
                &solid_pipeline,
            );
            render_data
                .extra_bind_groups
                .push((2, terrain_materials_bind_group));
            let object = retain_render_data(
                &mut ctx.globals.renderer,
                render_data,
                solid_shadow_pipeline,
            );
            uploaded.push((mesh.key, object));
        }

        let mut game_state = ctx.world.get_resource_mut::<GameState>().unwrap();
        let mut removed = Vec::new();
        // A requested chunk that contours to nothing must remove any previous
        // render object for that key instead of silently preserving it.
        for key in keys_to_clear {
            if let Some(object) = game_state.planets_meshes.remove(&key) {
                removed.push(object);
            }
        }
        for (key, object) in uploaded {
            if let Some(previous) = game_state.planets_meshes.insert(key, object) {
                removed.push(previous);
            }
        }
        drop(game_state);
        let mut objects = ctx.globals.renderer.objects();
        for object in removed {
            objects.remove(object);
        }
        complete_replacement_transition(ctx, planet_entity, transition_key, completed_state);
        for (transition_key, completed_state) in additional_transitions {
            complete_replacement_transition(ctx, planet_entity, transition_key, completed_state);
        }
        let pending: Vec<PendingMeshRequest> = {
            let mut mesh_jobs = ctx.world.get_resource_mut::<MeshJobResults>().unwrap();
            replacement_mesh_keys
                .iter()
                .filter_map(|key| mesh_jobs.pending_requests.remove(key))
                .collect()
        };
        for pending in pending {
            submit_requested_mesh_internal(ctx, pending.request, pending.urgent);
        }
    }

    loop {
        if start.elapsed() >= MESH_UPLOAD_BUDGET {
            break;
        }

        let mesh = {
            let Some(mut mesh_jobs) = ctx.world.get_resource_mut::<MeshJobResults>() else {
                return;
            };

            if mesh_jobs.ready_meshes.is_empty() {
                break;
            }

            // Urgent meshes first, preserving FIFO order within each priority.
            let next_index = mesh_jobs
                .ready_meshes
                .iter()
                .position(|mesh| mesh.urgent)
                .unwrap_or(0);
            mesh_jobs.ready_meshes.remove(next_index)
        };
        if mesh.indices.is_empty() {
            let previous = ctx
                .world
                .get_resource_mut::<GameState>()
                .unwrap()
                .planets_meshes
                .remove(&mesh.key);
            if let Some(previous) = previous {
                ctx.globals.renderer.objects().remove(previous);
            }
            continue;
        }
        let (solid_material, solid_pipeline, solid_shadow_pipeline, terrain_materials_bind_group) = {
            let game_state = ctx.world.get_resource::<GameState>().unwrap();
            (
                game_state.solid_material.clone(),
                game_state.solid_pipeline,
                game_state.solid_shadow_pipeline,
                game_state.terrain_materials_bind_group,
            )
        };
        let vertex_bytes: Vec<u8> = bytemuck::cast_slice(&mesh.vertices).to_vec();
        let mut render_data = ctx.globals.renderer.renderer_api.create_render_data(
            &vertex_bytes,
            &mesh.indices,
            solid_material,
            &solid_pipeline,
        );
        render_data
            .extra_bind_groups
            .push((2, terrain_materials_bind_group));
        let object = retain_render_data(
            &mut ctx.globals.renderer,
            render_data,
            solid_shadow_pipeline,
        );
        let previous = ctx
            .world
            .get_resource_mut::<GameState>()
            .unwrap()
            .planets_meshes
            .insert(mesh.key, object);
        if let Some(previous) = previous {
            ctx.globals.renderer.objects().remove(previous);
        }
    }
}

pub fn apply_change(ctx: &mut SystemContext, change: &OctreeChanges) {
    match change {
        OctreeChanges::ReplaceMeshes {
            planet_entity,
            transition_key,
            completed_state,
            additional_transitions,
            keys_to_remove,
            requests,
        } => {
            submit_replacement(
                ctx,
                *planet_entity,
                *transition_key,
                *completed_state,
                additional_transitions.clone(),
                keys_to_remove.clone(),
                requests.clone(),
            );
        }
        OctreeChanges::AddMesh { request } => {
            submit_requested_mesh(ctx, *request);
        }
        OctreeChanges::RemoveMeshes { key } => {
            if let Some(mut mesh_jobs) = ctx.world.get_resource_mut::<MeshJobResults>() {
                mesh_jobs.wanted.remove(key);
                mesh_jobs.pending_requests.remove(key);
            }

            let removed = ctx
                .world
                .get_resource_mut::<GameState>()
                .unwrap()
                .planets_meshes
                .remove(key);
            if let Some(object) = removed {
                ctx.globals.renderer.objects().remove(object);
            }
        }
    }
}

fn submit_replacement(
    ctx: &mut SystemContext,
    planet_entity: engine::ecs::entity::Entity,
    transition_key: NodeKey,
    completed_state: NodeState,
    additional_transitions: Vec<(NodeKey, NodeState)>,
    keys_to_remove: Vec<NodeKey>,
    requests: Vec<PlanetMeshRequest>,
) {
    if requests.is_empty() {
        for key in keys_to_remove {
            apply_change(ctx, &OctreeChanges::RemoveMeshes { key });
        }
        complete_replacement_transition(ctx, planet_entity, transition_key, completed_state);
        for (transition_key, completed_state) in additional_transitions {
            complete_replacement_transition(ctx, planet_entity, transition_key, completed_state);
        }
        return;
    }

    let mut mesh_jobs = ctx.world.get_resource_mut::<MeshJobResults>().unwrap();
    // Invalidate old CPU jobs now, but keep their already uploaded meshes visible.
    for key in &keys_to_remove {
        mesh_jobs.wanted.remove(key);
        mesh_jobs.pending_requests.remove(key);
    }
    drop(mesh_jobs);

    // Prepare ECS-owned data on the main thread. The worker only receives owned data.
    let terrain_config = {
        let terrain_config = ctx
            .world
            .get::<Arc<PlanetTerrainConfig>>(planet_entity)
            .unwrap();
        Arc::clone(&terrain_config)
    };
    let prepared: Vec<_> = requests
        .iter()
        .map(|request| {
            let terrain_edits = ctx
                .world
                .get::<PlanetTerrainEdits>(request.planet_entity)
                .unwrap();
            (
                *request,
                PlanetTerrainSnapshot {
                    config: Arc::clone(&terrain_config),
                    edits: edits_for_mesh_request(&terrain_edits, request),
                    planet_position: request.planet_position,
                },
            )
        })
        .collect();

    let (replacement_id, sender, base_grid_cache, versions) = {
        let mut mesh_jobs = ctx.world.get_resource_mut::<MeshJobResults>().unwrap();

        mesh_jobs.next_replacement_id += 1;
        let replacement_id = mesh_jobs.next_replacement_id;
        let mut versions = Vec::with_capacity(requests.len());
        for request in &requests {
            let key = mesh_request_key(request);
            mesh_jobs.wanted.insert(key);
            let version = mesh_jobs.versions.entry(key).or_insert(0);
            *version += 1;
            versions.push(*version);
            mesh_jobs.in_flight.insert(key);
            *mesh_jobs.in_flight_counts.entry(key).or_insert(0) += 1;
        }
        (
            replacement_id,
            mesh_jobs.replacement_sender.clone(),
            Arc::clone(&mesh_jobs.base_grid_cache),
            versions,
        )
    };

    let priority = mesh_job_priority(ctx, &requests);
    let priority_requests = requests.clone();
    let retry_requests = requests.clone();
    let job = move || {
        let meshes = prepared
            .into_iter()
            .zip(versions)
            .collect::<Vec<_>>()
            .into_par_iter()
            .map(|((request, terrain), version)| {
                let sampler = terrain.sampler_context();
                build_requested_mesh(
                    request,
                    version,
                    false,
                    &sampler,
                    Arc::clone(&base_grid_cache),
                )
            })
            .collect();

        let _ = sender.send(GeneratedReplacement {
            replacement_id,
            planet_entity,
            transition_key,
            completed_state,
            additional_transitions,
            keys_to_remove,
            requests: retry_requests,
            meshes,
        });
    };
    if let Ok(handle) = ctx.globals.job_system.spawn_prioritized(priority, job) {
        let mut mesh_jobs = ctx.world.get_resource_mut::<MeshJobResults>().unwrap();
        mesh_jobs.prioritized_jobs.push(PrioritizedMeshJob {
            handle,
            target: MeshPriorityTarget::Replacement(replacement_id),
            requests: priority_requests,
        });
    }
}

fn mesh_job_priority(ctx: &SystemContext, requests: &[PlanetMeshRequest]) -> u32 {
    let camera_pos = ctx
        .world
        .get_resource::<GameCamera>()
        .and_then(|camera| ctx.world.get::<TransformComponent>(camera.entity))
        .map(|transform| transform.position)
        .unwrap_or(Vec3::ZERO);
    priority_for_requests(requests, camera_pos)
}

fn reprioritize_mesh_jobs(ctx: &mut SystemContext, camera_pos: Vec3) {
    let Some(mesh_jobs) = ctx.world.get_resource::<MeshJobResults>() else {
        return;
    };
    for job in &mesh_jobs.prioritized_jobs {
        job.handle
            .set(priority_for_requests(&job.requests, camera_pos));
    }
}

fn priority_for_requests(requests: &[PlanetMeshRequest], camera_pos: Vec3) -> u32 {
    let distance = requests
        .iter()
        .map(|request| {
            let half = request.node_size * 0.5;
            let center = request.node_min_corner + vec3(half, half, half);
            center.distance(camera_pos)
        })
        .min_by(f32::total_cmp)
        .unwrap_or(f32::MAX);
    let node_size = requests
        .first()
        .map_or(f32::MAX, |request| request.node_size);

    // Distance is primary. The low byte breaks ties in favor of deeper LODs.
    let distance_bucket = (distance * 4.0).clamp(0.0, 0x00ff_ffff as f32) as u32;
    let depth_tie_breaker = 255_u32.saturating_sub(node_size.max(1.0).log2() as u32);
    ((0x00ff_ffff - distance_bucket) << 8) | depth_tie_breaker
}

fn complete_replacement_transition(
    ctx: &mut SystemContext,
    planet_entity: engine::ecs::entity::Entity,
    transition_key: NodeKey,
    completed_state: NodeState,
) {
    let Some(mut planet) = ctx.world.get_mut::<Planet>(planet_entity) else {
        return;
    };
    set_octree_node_state(&mut planet.octree_root, transition_key, completed_state);
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
    key: NodeKey,
    nx: u32,
    ny: u32,
    nz: u32,
    resolution: f32,
    min: Vec3,
    terrain: &PlanetTerrainSamplerContext<'_>,
    base_grid_cache: &Arc<Mutex<HashMap<NodeKey, Arc<DensityGrid>>>>,
) -> Arc<DensityGrid> {
    if let Some(grid) = base_grid_cache
        .lock()
        .ok()
        .and_then(|cache| cache.get(&key).cloned())
    {
        return grid;
    }

    let grid = Arc::new(generate_base_grid_from_min(
        nx, ny, nz, resolution, min, terrain,
    ));

    if let Ok(mut cache) = base_grid_cache.lock() {
        cache
            .entry(key)
            .or_insert_with(|| Arc::clone(&grid))
            .clone()
    } else {
        grid
    }
}

fn generate_base_grid_from_min(
    nx: u32,
    ny: u32,
    nz: u32,
    resolution: f32,
    min: Vec3,
    terrain: &PlanetTerrainSamplerContext<'_>,
) -> DensityGrid {
    let mut grid = Vec::with_capacity(nx as usize);
    for xi in 0..nx {
        let mut plane = Vec::with_capacity(ny as usize);
        for yi in 0..ny {
            let mut row = Vec::with_capacity(nz as usize);
            for zi in 0..nz {
                let position = vec3(
                    min.x + xi as f32 * resolution,
                    min.y + yi as f32 * resolution,
                    min.z + zi as f32 * resolution,
                );
                row.push(terrain_sampler::sample_original_density(terrain, position));
            }
            plane.push(row);
        }
        grid.push(plane);
    }
    grid
}

fn generate_grid_from_base(
    base_grid: &DensityGrid,
    resolution: f32,
    min: Vec3,
    terrain: &PlanetTerrainSamplerContext<'_>,
) -> DensityGrid {
    let nx = base_grid.len();
    let ny = base_grid.first().map_or(0, Vec::len);
    let nz = base_grid
        .first()
        .and_then(|plane| plane.first())
        .map_or(0, Vec::len);
    let mut grid = Vec::with_capacity(nx);

    for xi in 0..nx {
        let mut plane = Vec::with_capacity(ny);
        for yi in 0..ny {
            let mut row = Vec::with_capacity(nz);
            for zi in 0..nz {
                let position = vec3(
                    min.x + xi as f32 * resolution,
                    min.y + yi as f32 * resolution,
                    min.z + zi as f32 * resolution,
                );
                row.push(
                    base_grid[xi][yi][zi]
                        + terrain_sampler::sample_terrain_edits_density(terrain, position),
                );
            }
            plane.push(row);
        }
        grid.push(plane);
    }

    grid
}
