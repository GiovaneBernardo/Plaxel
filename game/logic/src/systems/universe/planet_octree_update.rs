use std::{collections::HashMap, sync::Arc};

use engine::{
    multithreading::job_system::JobSystem, prelude::*, profile_scope, renderer::DebugPassNode,
};
use game_types::{
    octree::{GeneratedMesh, NodeKey, NodeState, OctreeChanges, PlanetLodSettings},
    planet::{Planet, PlanetTerrainEdits},
    terrain::PlanetTerrainConfig,
};
use plaxel_reflect::Reflect;

use crate::{
    GameCamera, GameState, octree,
    systems::planet_mesher::{generate_planet_node_mesh, remove_mesh, upload_mesh},
};

pub fn planet_octree_update(
    asset_server: Res<AssetServer>,
    default_meshes: Res<DefaultMeshes>,
    mut camera: ResMut<GameCamera>,
    game_state: Res<GameState>,
    lod_settings: Res<PlanetLodSettings>,
    mut transforms: Query<(&mut TransformComponent,)>,
    mut planets_query: Query<(&mut Planet, &PlanetTerrainEdits, &Arc<PlanetTerrainConfig>)>,
    mut generation: ResMut<PlanetMeshGeneration>,
    mut globals: GlobalsMut,
    commands: &mut Commands,
) {
    profile_scope!("terrain.planet_octree.update");
    let camera_entity = camera.entity;

    let update_octree = game_state.update_octree;

    let mut camera_pos = transforms.get(camera_entity).unwrap().0.position;
    //let heightmap = ctx
    //    .world
    //    .get_resource::<Arc<EarthHeightmap>>()
    //    .map(|heightmap| Arc::clone(&heightmap));

    let mut changes = Vec::new();
    let lod_strength = lod_settings.strength;
    let mut atmosphere_planet = None;
    let mut debug_pass = globals
        .renderer
        .render_graph
        .get_node_mut::<DebugPassNode>(engine::renderer::ids::graph_passes::DEBUG);
    if let Some(debug_pass) = debug_pass.as_mut() {
        debug_pass.clear_cubes();
    }
    {
        planets_query.for_each(|entity, (planet, terrain_edits, terrain_config)| {
            let change_start = changes.len();
            if update_octree {
                {
                    profile_scope!("terrain.planet.update_octree");
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
            }

            let planet_changes: Vec<_> = changes.drain(change_start..).collect();
            for mut change in planet_changes {
                match &mut change {
                    OctreeChanges::ReplaceMeshes {
                        planet_entity,
                        transition_key,
                        completed_state,
                        additional_transitions,
                        keys_to_remove,
                        requests,
                    } => {
                        let min = vec3(
                            transition_key.x as f32,
                            transition_key.y as f32,
                            transition_key.z as f32,
                        );

                        let size =
                            planet.octree_root.size / 2.0_f32.powi(i32::from(transition_key.level));

                        submit_replacement(
                            change,
                            &mut generation,
                            &globals.job_system,
                            terrain_config,
                        );
                    }
                    OctreeChanges::AddMesh { request } => {
                        submit_addition(change);
                    }
                    _ => {}
                };
            }

            //if let Some(debug_pass) = debug_pass.as_mut() {
            //    let mut leaves = Vec::new();
            //    octree::collect_leaf_nodes(&planet.octree_root, &mut leaves);
            //    for leaf in leaves {
            //        if !leaf.may_contain_surface {
            //            continue;
            //        }
            //        debug_pass.add_cube(
            //            leaf.min + Vec3::splat(leaf.size * 0.5),
            //            leaf.size,
            //            octree::depth_color(leaf.key.level as u32),
            //        );
            //    }
            //}

            if atmosphere_planet.is_none() {
                atmosphere_planet = Some((planet.clone(), terrain_config.radius));
            }
        });
    }

    // TODO: Reenable this (not sure what it does, might be to sync the sun direction when planet moves)
    //if atmosphere_planet.is_some() {
    //    let (plan, planet_radius) = atmosphere_planet.unwrap();
    //    let planet_position = plan.position;
    //    let sun_position = transforms.get(plan.solar_system).unwrap().0.position;
    //
    //    let settings = &mut ctx
    //        .globals
    //        .renderer
    //        .render_graph
    //        .get_node_mut::<AtmospherePassNode>(engine::renderer::ids::graph_passes::ATMOSPHERE)
    //        .unwrap()
    //        .settings;
    //    settings.set_planet(planet_position.into(), planet_radius);
    //    settings.sun_direction = (vec3(sun_position.x, sun_position.y, sun_position.z)
    //        - vec3(planet_position.x, planet_position.y, planet_position.z))
    //    .normalize()
    //    .into();
    //}
}

fn submit_replacement(
    change: OctreeChanges,
    generation: &mut PlanetMeshGeneration,
    job_system: &JobSystem,
    terrain_config: &Arc<PlanetTerrainConfig>,
) {
    let OctreeChanges::ReplaceMeshes {
        planet_entity,
        transition_key,
        completed_state,
        additional_transitions,
        keys_to_remove,
        requests,
    } = change
    else {
        unreachable!();
    };

    let replacement_id = generation.next_replacement_id;
    generation.next_replacement_id += 1;

    generation.replacements.insert(
        replacement_id,
        PendingReplacement {
            planet_entity,
            transition_key,
            completed_state,
            additional_transitions,
            keys_to_remove,

            expected_meshes: requests.len(),
            meshes: Vec::with_capacity(requests.len()),
        },
    );

    for request in requests {
        let tx = generation.completed_tx.clone();
        let terrain_config = Arc::clone(&terrain_config);

        job_system
            .spawn_prioritized_named("planet.mesh.generate", 100, move || {
                let mesh = generate_planet_node_mesh(&request, terrain_config);

                let _ = tx.send(CompletedMesh {
                    replacement_id,
                    mesh,
                });
            })
            .unwrap();
    }
}

fn submit_addition(replacement: OctreeChanges) {}

pub fn drain_completed_requests(mut generation: ResMut<PlanetMeshGeneration>) {
    while let Ok(completed) = generation.completed_rx.try_recv() {
        let Some(replacement) = generation.replacements.get_mut(&completed.replacement_id) else {
            continue;
        };

        replacement.meshes.push(completed.mesh);
    }

    let completed_replacements: Vec<u64> = generation
        .replacements
        .iter()
        .filter_map(|(&id, replacement)| {
            if replacement.meshes.len() == replacement.expected_meshes {
                Some(id)
            } else {
                None
            }
        })
        .collect();

    for replacement_id in completed_replacements {
        let replacement = generation.replacements.remove(&replacement_id).unwrap();

        //
        // New first.
        //
        for mesh in &replacement.meshes {
            upload_mesh(mesh);
        }

        //
        // Old second.
        //
        for key in &replacement.keys_to_remove {
            remove_mesh(replacement.planet_entity, *key);
        }

        //finish_octree_transition(
        //    replacement.transition_key,
        //    replacement.completed_state,
        //    replacement.additional_transitions,
        //);
    }
}

#[derive(Reflect)]
pub struct PendingReplacement {
    pub planet_entity: Entity,
    pub transition_key: NodeKey,
    pub completed_state: NodeState,
    pub additional_transitions: Vec<(NodeKey, NodeState)>,
    pub keys_to_remove: Vec<NodeKey>,

    pub expected_meshes: usize,
    pub meshes: Vec<GeneratedMesh>,
}

pub struct CompletedMesh {
    pub replacement_id: u64,
    pub mesh: GeneratedMesh,
}

#[derive(Reflect)]
// Ignored channel endpoints cannot be defaulted independently by FromReflect.
#[reflect(from_reflect = false)]
pub struct PlanetMeshGeneration {
    pub next_replacement_id: u64,

    pub replacements: HashMap<u64, PendingReplacement>,

    #[reflect(ignore)]
    pub completed_tx: crossbeam_channel::Sender<CompletedMesh>,
    #[reflect(ignore)]
    pub completed_rx: crossbeam_channel::Receiver<CompletedMesh>,
}

impl Default for PlanetMeshGeneration {
    fn default() -> Self {
        let (completed_tx, completed_rx) = crossbeam_channel::unbounded();

        Self {
            next_replacement_id: 0,
            replacements: HashMap::new(),
            completed_tx,
            completed_rx,
        }
    }
}
