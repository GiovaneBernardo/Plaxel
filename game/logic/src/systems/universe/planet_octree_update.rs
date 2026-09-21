use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use engine::{
    game_info, multithreading::job_system::JobSystem, prelude::*, profile_scope,
    renderer::DebugPassNode,
};
use game_types::{
    octree::{
        GeneratedMesh, NodeKey, NodeState, OctreeChanges, PlanetLodSettings, PlanetMeshRequest,
    },
    planet::{Planet, PlanetTerrainEdits},
    terrain::{self, PlanetTerrainConfig},
};
use plaxel_reflect::Reflect;

use crate::{
    GameCamera, GameState, octree,
    render::producers::planet_terrain_producer::{
        PendingTerrainChunk, PlanetTerrainCommand, PlanetTerrainEvent, PlanetTerrainEvents,
        PlanetTerrainRenderQueue,
    },
    systems::planet_mesher::{
        cache::DensityCache, generate_planet_node_mesh, remove_mesh, upload_mesh,
    },
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
    queue: Res<PlanetTerrainRenderQueue>,
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
    let mut active_planets = HashSet::new();
    let lod_strength = lod_settings.strength;
    let mut debug_pass = globals
        .renderer
        .render_graph
        .get_node_mut::<DebugPassNode>(engine::renderer::ids::graph_passes::DEBUG);
    if let Some(debug_pass) = debug_pass.as_mut() {
        debug_pass.clear_cubes();
    }
    {
        planets_query.for_each(|entity, (planet, terrain_edits, terrain_config)| {
            active_planets.insert(entity);
            let change_start = changes.len();
            if update_octree
                && !generation
                    .replacements
                    .values()
                    .any(|r| r.planet_entity == entity)
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

            if changes.len() == change_start {
                return;
            }

            let edits = Arc::new(terrain_edits.clone());
            let planet_changes: Vec<_> = changes.drain(change_start..).collect();
            for mut change in planet_changes {
                if let OctreeChanges::ReplaceMeshes { requests, .. } = &mut change {
                    // Existing neighbors also need new seam ownership after a split/merge.
                    let mut neighbors = Vec::new();
                    for request in requests.iter() {
                        octree::collect_face_neighbor_leaves(
                            &planet.octree_root,
                            request.node_min_corner,
                            request.node_size,
                            &mut neighbors,
                        );
                    }
                    let mut keys: HashSet<_> = requests.iter().map(|r| r.node_key).collect();
                    for node in neighbors {
                        if node.may_contain_surface && keys.insert(node.key) {
                            requests.push(PlanetMeshRequest {
                                planet_entity: entity,
                                node_key: node.key,
                                planet_position: planet.position,
                                node_min_corner: node.min,
                                node_size: node.size,
                                face_neighbors: [game_types::octree::FaceNeighbor::SAME_OR_ABSENT;
                                    6],
                            });
                        }
                    }
                    for request in requests {
                        octree::annotate_mesh_request(&planet.octree_root, request);
                    }
                }

                match &change {
                    OctreeChanges::ReplaceMeshes {
                        planet_entity,
                        transition_key,
                        completed_state,
                        additional_transitions,
                        keys_to_remove,
                        requests,
                    } => {
                        //for key in keys_to_remove {
                        //    generation.debug_nodes.remove(&(*planet_entity, *key));
                        //}
                        //
                        //for request in requests {
                        //    generation
                        //        .debug_nodes
                        //        .insert((request.planet_entity, request.node_key), *request);
                        //}

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
                            &edits,
                        );
                    }
                    OctreeChanges::AddMesh { request } => {
                        generation
                            .debug_nodes
                            .insert((request.planet_entity, request.node_key), *request);
                        submit_addition(change);
                    }
                    OctreeChanges::RemoveMeshes { planet_entity, key } => {
                        generation.debug_nodes.remove(&(*planet_entity, *key));
                        queue
                            .send(PlanetTerrainCommand::ReplaceChunks {
                                planet: *planet_entity,
                                remove_all: false,
                                remove: vec![*key],
                                insert: Vec::new(),
                                replacement_id: None,
                            })
                            .expect("terrain renderer connected");
                    }
                    OctreeChanges::CancelPlanetReplacements { planet_entity } => {
                        //generation
                        //    .debug_nodes
                        //    .retain(|(entity, _), _| entity != planet_entity);
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
        });
    }

    generation
        .density_caches
        .retain(|entity, _| active_planets.contains(entity));

    // Update debug drawing
    if let Some(debug_pass) = globals
        .renderer
        .render_graph
        .get_node_mut::<DebugPassNode>(engine::renderer::ids::graph_passes::DEBUG)
    {
        debug_pass.clear_cubes();
        debug_pass.clear_wire_cubes();

        for request in generation.debug_nodes.values() {
            let center = request.node_min_corner + Vec3::splat(request.node_size * 0.5);

            debug_pass.add_cube(
                center,
                request.node_size,
                octree::depth_color(request.node_key.level as u32),
            );
        }
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
    edits: &Arc<PlanetTerrainEdits>,
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
            submitted: false,

            expected_meshes: requests.len(),
            meshes: Vec::with_capacity(requests.len()),
        },
    );

    // Jobs share a cache for this planet's immutable configuration snapshot.
    // Existing jobs keep their old snapshot alive when the config is replaced.
    let cache = generation
        .density_caches
        .entry(planet_entity)
        .or_insert_with(|| Arc::new(DensityCache::new(Arc::clone(terrain_config))));
    if !Arc::ptr_eq(&cache.config, terrain_config) {
        *cache = Arc::new(DensityCache::new(Arc::clone(terrain_config)));
    }

    for request in requests {
        let tx = generation.completed_tx.clone();
        let terrain_config = Arc::clone(&terrain_config);
        let edits = Arc::clone(&edits);
        let cache = Arc::clone(cache);

        job_system
            .spawn_prioritized_named("planet.mesh.generate", 100, move || {
                let mesh = generate_planet_node_mesh(&request, terrain_config, edits, &cache);

                let _ = tx.send(CompletedMesh {
                    replacement_id,
                    mesh,
                });
            })
            .unwrap();
    }
}

fn submit_addition(replacement: OctreeChanges) {}

pub fn drain_completed_requests(
    mut generation: ResMut<PlanetMeshGeneration>,
    queue: Res<PlanetTerrainRenderQueue>,
    events: Res<PlanetTerrainEvents>,
    mut planets: Query<(&mut Planet,)>,
) {
    for event in events.try_iter() {
        match event {
            PlanetTerrainEvent::ReplacementApplied {
                replacement_id: Some(id),
                rendered_keys,
                ..
            } => {
                if let Some(replacement) = generation.replacements.remove(&id) {
                    if let Some((planet,)) = planets.get(replacement.planet_entity) {
                        finish_transition(
                            &mut planet.octree_root,
                            replacement.transition_key,
                            replacement.completed_state,
                        );
                        for (key, state) in replacement.additional_transitions {
                            finish_transition(&mut planet.octree_root, key, state);
                        }
                        let rendered: HashSet<_> = rendered_keys.into_iter().collect();
                        if let Some((ancestor, descendant)) =
                            octree::rendered_overlap(&planet.octree_root, &rendered)
                        {
                            engine::game_error!(
                                "Overlapping terrain LODs: {ancestor:?} and {descendant:?}"
                            );
                        }
                        let mut leaves = Vec::new();
                        octree::collect_leaf_nodes(&planet.octree_root, &mut leaves);
                        let valid: HashSet<_> = leaves.iter().map(|node| node.key).collect();
                        if let Some(stale) = rendered.difference(&valid).next() {
                            engine::game_error!(
                                "Rendered terrain chunk is not a current leaf: {stale:?}"
                            );
                        }
                    }
                }
            }
            PlanetTerrainEvent::ReplacementFailed { reason, .. } => {
                engine::game_error!("Terrain replacement failed: {reason}");
            }
            _ => {}
        }
    }

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
            if !replacement.submitted && replacement.meshes.len() == replacement.expected_meshes {
                Some(id)
            } else {
                None
            }
        })
        .take(1) // One small atomic GPU upload batch per frame.
        .collect();

    for replacement_id in completed_replacements {
        let replacement = generation.replacements.get_mut(&replacement_id).unwrap();
        replacement.submitted = true;

        ////
        //// New first.
        ////
        //for mesh in &replacement.meshes {
        //    upload_mesh(mesh);
        //}
        //
        ////
        //// Old second.
        ////
        //for key in &replacement.keys_to_remove {
        //    remove_mesh(replacement.planet_entity, *key);
        //}

        let insert = std::mem::take(&mut replacement.meshes)
            .into_iter()
            .map(|mesh| PendingTerrainChunk {
                key: mesh.key,
                node_origin_planet: mesh.node_origin_planet,
                vertices: mesh.vertices,
                indices: mesh.indices,
            })
            .collect();

        queue
            .send(PlanetTerrainCommand::ReplaceChunks {
                planet: replacement.planet_entity,
                replacement_id: Some(replacement_id),
                remove_all: false,
                remove: replacement.keys_to_remove.clone(), //remove: replacement.keys_to_remove,
                insert,
            })
            .expect("terrain renderer must remain connected");

        //finish_octree_transition(
        //    replacement.transition_key,
        //    replacement.completed_state,
        //    replacement.additional_transitions,
        //);
    }
}

fn finish_transition(node: &mut game_types::octree::OctreeNode, key: NodeKey, state: NodeState) {
    if node.key == key {
        node.state = state;
        return;
    }
    if let Some(children) = &mut node.children {
        for child in children {
            finish_transition(child, key, state);
        }
    }
}

#[derive(Reflect)]
pub struct PendingReplacement {
    pub planet_entity: Entity,
    pub transition_key: NodeKey,
    pub completed_state: NodeState,
    pub additional_transitions: Vec<(NodeKey, NodeState)>,
    pub keys_to_remove: Vec<NodeKey>,
    pub submitted: bool,

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

    #[reflect(ignore)]
    density_caches: HashMap<Entity, Arc<DensityCache>>,

    pub replacements: HashMap<u64, PendingReplacement>,

    #[reflect(ignore)]
    pub completed_tx: crossbeam_channel::Sender<CompletedMesh>,
    #[reflect(ignore)]
    pub completed_rx: crossbeam_channel::Receiver<CompletedMesh>,

    pub debug_nodes: HashMap<(Entity, NodeKey), PlanetMeshRequest>,
}

impl Default for PlanetMeshGeneration {
    fn default() -> Self {
        let (completed_tx, completed_rx) = crossbeam_channel::unbounded();

        Self {
            next_replacement_id: 0,
            density_caches: HashMap::new(),
            replacements: HashMap::new(),
            completed_tx,
            completed_rx,
            debug_nodes: HashMap::new(),
        }
    }
}
