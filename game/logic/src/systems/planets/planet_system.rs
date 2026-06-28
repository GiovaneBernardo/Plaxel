use cgmath::{EuclideanSpace, InnerSpace, Quaternion, Vector3, point3, vec3};
use engine::{
    core::components::core::TransformComponent,
    ecs::{commands::Commands, query::Query, system::SystemContext},
    renderer::PipelineHandle,
};
use game_types::{
    octree::{NodeKey, OctreeChanges, PlanetMeshRequest},
    planet::Planet,
};
use rand::Rng;

use crate::{
    CHUNK_SIZE, GameCamera, GameState, generate_grid_from_min, octree, systems::planets::PlanetExt,
};

const PLANET_COUNT: usize = 12;
const PLANET_SPAWN_RANGE: f32 = 1_000_000.0 / 16.0;
const MAX_PLANET_SPAWN_ATTEMPTS: usize = 256;

fn random_planet_position(
    rng: &mut impl Rng,
    existing_positions: &[Vector3<f32>],
    min_distance: f32,
) -> Option<Vector3<f32>> {
    let min_distance_sq = min_distance * min_distance;
    let far_enough = |candidate: Vector3<f32>| {
        existing_positions
            .iter()
            .all(|position| (candidate - *position).magnitude2() >= min_distance_sq)
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

    let planet_size = 65536 / 16;
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

    for i in 0..PLANET_COUNT {
        let Some(planet_position) =
            random_planet_position(&mut rng, &planet_positions, min_planet_distance)
        else {
            continue;
        };
        planet_positions.push(planet_position);
        let new_planet = world.spawn();

        world.insert(
            new_planet,
            TransformComponent {
                position: planet_position,
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                scale: vec3(1.0, 1.0, 1.0),
                velocity: vec3(0.0, 0.0, 0.0),
            },
        );

        let octree = Planet::create_octree(
            planet_position,
            planet_size as u32 / 2,
            &point3(camera_pos.x, camera_pos.y, camera_pos.z),
            planet_size,
            chunk_size,
        );

        let planet = Planet {
            id: new_planet.index() as u64,
            name: format!("Planet {}", i + 1),
            position: planet_position,
            octree_root: octree,
        };
        let mut leaf_nodes = Vec::new();
        octree::collect_leaf_nodes(&planet.octree_root, &mut leaf_nodes);

        for leaf in leaf_nodes {
            mesh_requests.push(PlanetMeshRequest {
                planet_position: planet.position,
                planet_size,
                node_min_corner: leaf.min,
                node_size: leaf.size,
            })
        }

        world.insert(new_planet, planet);
    }

    //for request in mesh_requests {
    //    ctx.globals.job_system.spawn(move || {
    //        build_requested_mesh(ctx, &request);
    //    });
    //}
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

    let mut changes = Vec::new();
    {
        let mut query = Query::<(&mut Planet,)>::new(&mut ctx.world);
        query.for_each(|_, (planet,)| {
            let planet_size = (planet.octree_root.size * 2.0) as u32;
            octree::update(
                &mut planet.octree_root,
                camera_pos,
                planet.position,
                planet_size,
                &mut changes,
            );
        });
    }

    for change in changes {
        apply_change(ctx, &change);
    }
}

pub fn build_requested_mesh(ctx: &mut SystemContext, request: &PlanetMeshRequest) {
    let mut game_state = ctx.world.get_resource_mut::<GameState>().unwrap();
    let planet_position = request.planet_position;
    let size = request.node_size;
    let min_corner = point3(
        request.node_min_corner.x,
        request.node_min_corner.y,
        request.node_min_corner.z,
    );

    let resolution = size / CHUNK_SIZE as f32;
    let grid = generate_grid_from_min(
        34,
        34,
        34,
        resolution,
        min_corner.to_vec(),
        planet_position,
        request.planet_size,
    );
    let (vertices, indices) = Planet::dual_contour_grid(
        &grid,
        min_corner,
        resolution,
        planet_position,
        request.planet_size,
    );

    //let collider_mesh = cook_terrain_collider_mesh(&chunk.vertices, &chunk.indices);
    let vertex_bytes: Vec<u8> = bytemuck::cast_slice(&vertices).to_vec();
    let render_data = ctx.globals.renderer.renderer_api.create_render_data(
        &vertex_bytes,
        &indices,
        game_state.solid_material.clone(),
        &PipelineHandle(0),
    );
    game_state.planets_meshes.insert(
        NodeKey {
            x: min_corner.x as i32,
            y: min_corner.y as i32,
            z: min_corner.z as i32,
            size: size as i32,
        },
        render_data,
    );
}

pub fn apply_change(ctx: &mut SystemContext, change: &OctreeChanges) {
    match change {
        OctreeChanges::AddMesh { request } => {
            build_requested_mesh(ctx, request);
        }
        OctreeChanges::RemoveMesh { key } => {
            let mut game_state = ctx.world.get_resource_mut::<GameState>().unwrap();
            game_state.planets_meshes.remove(key);
        }
    }
}
