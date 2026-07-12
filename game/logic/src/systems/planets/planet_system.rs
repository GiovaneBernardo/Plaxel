use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    thread,
};

use engine::math::{Quat, Vec3, vec3};
use engine::{
    core::components::core::TransformComponent,
    ecs::{commands::Commands, query::Query, system::SystemContext},
    game_info,
    renderer::{AtmospherePassNode, PipelineHandle},
};
use game_types::{
    octree::{NodeKey, OctreeChanges, PlanetMeshRequest},
    planet::{Planet, PlanetTerrainEdits, PlanetVertex, SolarSystemComponent},
};
use rand::Rng;
use web_time::{Duration, Instant};

use crate::{
    CHUNK_SIZE, GameCamera, GameState, octree,
    sdf::{EarthHeightmap, base_sdf_at_center, sample_terrain_edit, sdf_at_center},
    systems::planets::PlanetExt,
};

use crossbeam_channel::{Receiver, Sender};

type DensityGrid = Vec<Vec<Vec<f32>>>;

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
}

pub struct GeneratedMesh {
    pub key: NodeKey,
    pub version: u64,
    pub urgent: bool,
    pub vertices: Vec<PlanetVertex>,
    pub indices: Vec<u32>,
}

pub struct PendingMeshRequest {
    pub request: PlanetMeshRequest,
    pub urgent: bool,
}

const MESH_UPLOAD_BUDGET: Duration = Duration::from_millis(6);
const TERRAIN_EDIT_BRICK_SIZE: f32 = 32.0;
const TERRAIN_EDIT_LEVEL: u32 = 0;

const PLANET_COUNT: usize = 128;
const PLANET_RADIUS_MULTIPLIER: f32 = 1.0; //0.1;
const PLANET_SPAWN_RANGE: f32 = 1_000_000.0 * PLANET_RADIUS_MULTIPLIER;
const MAX_PLANET_SPAWN_ATTEMPTS: usize = 256;

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

    let planet_size = 65536.0 * PLANET_RADIUS_MULTIPLIER;
    let chunk_size = 32;
    let min_planet_distance = planet_size as f32;
    let mut rng = rand::thread_rng();

    let mut planet_positions = Vec::new();
    {
        let mut query = Query::<(&Planet,)>::new(world);
        query.for_each(|_, (planet,)| {
            planet_positions.push(planet.position);
        });
    }

    let mut mesh_requests = Vec::new();

    let (mesh_tx, mesh_rx) = crossbeam_channel::unbounded();
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
    });

    // Create solar system
    let solar_system = world.spawn();
    world.insert(
        solar_system,
        TransformComponent {
            position: random_planet_position(&mut rng, &planet_positions, min_planet_distance)
                .unwrap(),
            rotation: Quat::IDENTITY,
            scale: vec3(1.0, 1.0, 1.0),
            velocity: vec3(0.0, 0.0, 0.0),
        },
    );

    world.insert(
        solar_system,
        SolarSystemComponent {
            planets: Vec::new(),
        },
    );

    // Create planets
    let mut created_planets = Vec::new();
    for i in 0..PLANET_COUNT {
        let Some(mut planet_position) =
            random_planet_position(&mut rng, &planet_positions, min_planet_distance)
        else {
            continue;
        };
        if i == 0 {
            planet_position = vec3(0.0, 0.0, 0.0);
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
        };

        let octree = Planet::create_octree(
            planet_position,
            planet_size as u32 / 2,
            &vec3(camera_pos.x, camera_pos.y, camera_pos.z),
            planet_size as u32,
            chunk_size,
            &terrain_edits,
        );

        let planet = Planet {
            id: new_planet.index() as u64,
            name: format!("Planet {}", i + 1),
            position: planet_position,
            octree_root: octree,
            solar_system,
        };
        created_planets.push(new_planet);
        let mut leaf_nodes = Vec::new();
        octree::collect_leaf_nodes(&planet.octree_root, &mut leaf_nodes);

        for leaf in leaf_nodes {
            mesh_requests.push(PlanetMeshRequest {
                planet_entity: new_planet,
                planet_position: planet.position,
                planet_size: planet_size as u32,
                node_min_corner: leaf.min,
                node_size: leaf.size,
            })
        }

        world.insert(new_planet, planet);
        world.insert(new_planet, terrain_edits);
    }

    let mut solar_system_component = world.get_mut::<SolarSystemComponent>(solar_system).unwrap();
    solar_system_component.planets = created_planets;
    drop(solar_system_component);

    for request in mesh_requests {
        submit_requested_mesh(ctx, request);
    }
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
    //let heightmap = ctx
    //    .world
    //    .get_resource::<Arc<EarthHeightmap>>()
    //    .map(|heightmap| Arc::clone(&heightmap));

    let mut changes = Vec::new();
    let mut atmosphere_planet = None;
    {
        let mut query = Query::<(&mut Planet, &PlanetTerrainEdits)>::new(&mut ctx.world);
        query.for_each(|entity, (planet, terrain_edits)| {
            let planet_size = (planet.octree_root.size * 2.0) as u32;
            octree::update(
                &mut planet.octree_root,
                camera_pos,
                entity,
                planet.position,
                planet_size,
                &mut changes,
                None, //heightmap.as_deref(),
                terrain_edits,
            );

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
            .get_node_mut::<AtmospherePassNode>(1)
            .unwrap()
            .settings
            .sun_direction = (vec3(sun_position.x, sun_position.y, sun_position.z)
            - vec3(planet_position.x, planet_position.y, planet_position.z))
        .normalize()
        .into();
    }

    for change in changes {
        apply_change(ctx, &change);
    }

    drain_generated_meshes(ctx);
}

pub fn build_requested_mesh(
    request: PlanetMeshRequest,
    version: u64,
    urgent: bool,
    heightmap: Option<Arc<EarthHeightmap>>,
    terrain_edits: &PlanetTerrainEdits,
    base_grid_cache: Arc<Mutex<HashMap<NodeKey, Arc<DensityGrid>>>>,
) -> GeneratedMesh {
    let planet_position = request.planet_position;
    let size = request.node_size;
    let min_corner = vec3(
        request.node_min_corner.x,
        request.node_min_corner.y,
        request.node_min_corner.z,
    );

    let resolution = size / CHUNK_SIZE as f32;
    let key = NodeKey {
        x: min_corner.x as i32,
        y: min_corner.y as i32,
        z: min_corner.z as i32,
        size: size as i32,
    };
    let base_grid = get_or_build_base_grid(
        key,
        34,
        34,
        34,
        resolution,
        min_corner,
        planet_position,
        request.planet_size,
        heightmap.as_deref(),
        &base_grid_cache,
    );
    let grid;
    let grid_ref = if terrain_edits.modified_chunks.is_empty() {
        base_grid.as_ref()
    } else {
        grid = generate_grid_from_base(
            base_grid.as_ref(),
            resolution,
            min_corner,
            planet_position,
            terrain_edits,
        );
        &grid
    };
    let (vertices, indices) = Planet::dual_contour_grid(
        grid_ref,
        min_corner,
        resolution,
        planet_position,
        request.planet_size,
        heightmap.as_deref(),
        terrain_edits,
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
        };
    }

    let mesh_sample_spacing = request.node_size / CHUNK_SIZE as f32;
    let margin = mesh_sample_spacing * 2.0 + TERRAIN_EDIT_BRICK_SIZE;
    let local_min =
        request.node_min_corner - request.planet_position - vec3(margin, margin, margin);
    let local_max = request.node_min_corner - request.planet_position
        + vec3(request.node_size, request.node_size, request.node_size)
        + vec3(margin, margin, margin);
    let mut relevant_chunks = HashMap::new();

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
        }
    }

    PlanetTerrainEdits {
        modified_chunks: relevant_chunks,
    }
}

fn mesh_request_key(request: &PlanetMeshRequest) -> NodeKey {
    NodeKey {
        x: request.node_min_corner.x as i32,
        y: request.node_min_corner.y as i32,
        z: request.node_min_corner.z as i32,
        size: request.node_size as i32,
    }
}

pub(crate) fn submit_requested_mesh(ctx: &mut SystemContext, request: PlanetMeshRequest) {
    submit_requested_mesh_internal(ctx, request, false);
}

pub(crate) fn submit_requested_mesh_urgent(ctx: &mut SystemContext, request: PlanetMeshRequest) {
    submit_requested_mesh_internal(ctx, request, true);
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

    let job = move || {
        let mesh = build_requested_mesh(
            request,
            version,
            urgent,
            None, //heightmap,
            &terrain_edits,
            base_grid_cache,
        );
        let _ = sender.send(mesh);
    };

    if urgent {
        let _ = thread::Builder::new()
            .name("urgent-terrain-mesh".to_string())
            .spawn(job);
    } else {
        let _ = ctx.globals.job_system.spawn(job);
    }
}

pub fn drain_generated_meshes(ctx: &mut SystemContext) {
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

        if still_wanted && !mesh.vertices.is_empty() {
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
        let mut game_state = ctx.world.get_resource_mut::<GameState>().unwrap();
        let vertex_bytes: Vec<u8> = bytemuck::cast_slice(&mesh.vertices).to_vec();
        let mut render_data = ctx.globals.renderer.renderer_api.create_render_data(
            &vertex_bytes,
            &mesh.indices,
            game_state.solid_material.clone(),
            &PipelineHandle(0),
        );
        render_data
            .extra_bind_groups
            .push((2, game_state.terrain_materials_bind_group));
        game_state.planets_meshes.insert(mesh.key, render_data);
    }
}

pub fn apply_change(ctx: &mut SystemContext, change: &OctreeChanges) {
    match change {
        OctreeChanges::ReplaceMesh {
            keys_to_remove,
            requests,
        } => {
            for key in keys_to_remove {
                apply_change(ctx, &OctreeChanges::RemoveMeshes { key: *key });
            }
            for request in requests {
                submit_requested_mesh(ctx, *request);
            }
        }
        OctreeChanges::AddMesh { request } => {
            submit_requested_mesh(ctx, *request);
        }
        OctreeChanges::RemoveMeshes { key } => {
            if let Some(mut mesh_jobs) = ctx.world.get_resource_mut::<MeshJobResults>() {
                mesh_jobs.wanted.remove(key);
                mesh_jobs.pending_requests.remove(key);
            }

            let mut game_state = ctx.world.get_resource_mut::<GameState>().unwrap();
            game_state.planets_meshes.remove(key);
        }
    }
}

pub fn generate_grid_from_min(
    nx: u32,
    ny: u32,
    nz: u32,
    resolution: f32,
    min: Vec3,
    planet_position: Vec3,
    planet_size: u32,
    heightmap: Option<&EarthHeightmap>,
    terrain_edits: &PlanetTerrainEdits,
) -> Vec<Vec<Vec<f32>>> {
    let mut grid = Vec::new();
    for xi in 0..nx {
        let mut plane = Vec::new();
        for yi in 0..ny {
            let mut row = Vec::new();
            for zi in 0..nz {
                let position = vec3(
                    min.x + xi as f32 * resolution,
                    min.y + yi as f32 * resolution,
                    min.z + zi as f32 * resolution,
                );
                row.push(sdf_at_center(
                    position,
                    planet_position,
                    planet_size,
                    heightmap,
                    terrain_edits,
                ));
            }
            plane.push(row);
        }
        grid.push(plane);
    }
    grid
}

fn get_or_build_base_grid(
    key: NodeKey,
    nx: u32,
    ny: u32,
    nz: u32,
    resolution: f32,
    min: Vec3,
    planet_position: Vec3,
    planet_size: u32,
    heightmap: Option<&EarthHeightmap>,
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
        nx,
        ny,
        nz,
        resolution,
        min,
        planet_position,
        planet_size,
        heightmap,
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
    planet_position: Vec3,
    planet_size: u32,
    heightmap: Option<&EarthHeightmap>,
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
                row.push(base_sdf_at_center(
                    position,
                    planet_position,
                    planet_size,
                    heightmap,
                ));
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
    planet_position: Vec3,
    terrain_edits: &PlanetTerrainEdits,
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
                        + sample_terrain_edit(position - planet_position, terrain_edits),
                );
            }
            plane.push(row);
        }
        grid.push(plane);
    }

    grid
}
