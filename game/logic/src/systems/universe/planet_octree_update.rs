use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use engine::{
    game_info,
    multithreading::job_system::JobSystem,
    prelude::*,
    profile_scope,
    renderer::{AtmospherePassNode, DebugPassNode},
};
use game_types::{
    octree::{
        FaceNeighbor, GeneratedMesh, NodeKey, NodeState, OctreeChanges, OctreeNode,
        PlanetLodSettings, PlanetMeshRequest,
    },
    planet::{Planet, PlanetTerrainEdits},
    terrain::{self, PlanetTerrainConfig, terrain_field::TerrainGraphApplyQueue},
};
use plaxel_reflect::Reflect;

use crate::{
    GameCamera, GameState, octree,
    render::producers::planet_terrain_producer::{
        PendingChunkMesh, PlanetGenerationCommand, PlanetTerrainEvent, PlanetTerrainEvents,
        PlanetTerrainRenderQueue,
    },
    systems::planet_mesher::{
        cache::DensityCache, generate_terrain_node_mesh, remove_mesh, upload_mesh,
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
    let mut atmosphere_planet = None;
    let mut atmosphere_distance = f64::INFINITY;
    let lod_strength = lod_settings.strength;
    let mut debug_pass = globals
        .renderer
        .render_graph
        .get_node_mut::<DebugPassNode>(engine::renderer::ids::graph_passes::DEBUG);
    if let Some(debug_pass) = debug_pass.as_mut() {
        debug_pass.clear_cubes();
    }

    planets_query.for_each(|entity, (planet, terrain_edits, terrain_config)| {
        // Lighting also updates while LOD is unchanged or octree updates are paused.
        let distance = camera
            .world_position
            .distance_squared(planet.position.as_dvec3());
        if distance < atmosphere_distance {
            atmosphere_distance = distance;
            atmosphere_planet = Some((planet.position, terrain_config.radius, planet.solar_system));
        }
        active_planets.insert(entity);
        let change_start = changes.len();
        let pending = generation
            .replacements
            .values()
            .filter(|r| r.planet_entity == entity)
            .count();
        // Let old LOD/edit snapshots finish before selecting the current leaves.
        // While an edit is queued, do not schedule more LOD work ahead of it.
        if generation.dirty_terrain.contains_key(&entity) {
            if pending == 0 {
                let bounds = generation.dirty_terrain.remove(&entity).unwrap();
                octree::refresh_density_ranges_in_bounds(
                    &mut planet.surface_octree_root,
                    bounds.min,
                    bounds.max,
                    planet.position,
                    terrain_config,
                    terrain_edits,
                );
                let requests = terrain_edit_requests(planet, entity, bounds);
                let change = OctreeChanges::ReplaceMeshes {
                    planet_entity: entity,
                    transition_key: planet.surface_octree_root.key,
                    completed_state: planet.surface_octree_root.state,
                    additional_transitions: Vec::new(),
                    // Empty generated meshes must also remove the old geometry.
                    keys_to_remove: requests.iter().map(|r| r.node_key).collect(),
                    requests,
                };
                submit_replacement(
                    change,
                    vec![octree::node_bounds(&planet.surface_octree_root)],
                    &mut generation,
                    &globals.job_system,
                    terrain_config,
                    &Arc::new(terrain_edits.clone()),
                );
            }
            return;
        }
        let mut reserved: Vec<_> = generation
            .replacements
            .values()
            .filter(|r| r.planet_entity == entity)
            .flat_map(|r| r.reserved_regions.iter().copied())
            .collect();
        if update_octree {
            for _ in pending..4 {
                let start = changes.len();
                octree::update_reserved(
                    &mut planet.surface_octree_root,
                    camera_pos,
                    entity,
                    planet.position,
                    terrain_config.as_ref(),
                    lod_strength,
                    &mut changes,
                    terrain_edits,
                    &reserved,
                    !generation.balanced_planets.contains(&entity),
                );
                if changes.len() == start {
                    // No active locks: the fallback inspected the whole tree, not
                    // merely the currently available regions.
                    if pending == 0 && start == change_start {
                        generation.balanced_planets.insert(entity);
                    }
                    break;
                }
                for change in &changes[start..] {
                    reserved.extend(octree::replacement_footprint(
                        &planet.surface_octree_root,
                        change,
                    ));
                }
            }
        }

        if changes.len() == change_start {
            return;
        }

        let edits = Arc::new(terrain_edits.clone());
        let planet_changes: Vec<_> = changes.drain(change_start..).collect();
        for mut change in planet_changes {
            if let OctreeChanges::ReplaceMeshes {
                requests,
                transition_key,
                additional_transitions,
                ..
            } = &mut change
            {
                // Existing neighbors also need new seam ownership after a split/merge.
                let mut neighbors = Vec::new();
                // Include transitions with no surface requests: their neighbors still change ownership.
                for key in std::iter::once(&*transition_key)
                    .chain(additional_transitions.iter().map(|(key, _)| key))
                {
                    let min = vec3(key.x as f32, key.y as f32, key.z as f32);
                    let size = planet.surface_octree_root.size
                        / 2.0_f32.powi(i32::from(key.level - planet.surface_octree_root.key.level));
                    octree::collect_face_neighbor_leaves(
                        &planet.surface_octree_root,
                        min,
                        size,
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
                            face_neighbors: [game_types::octree::FaceNeighbor::SAME_OR_ABSENT; 6],
                        });
                    }
                }
                for request in requests {
                    octree::annotate_mesh_request(&planet.surface_octree_root, request);
                }
            }

            match &change {
                OctreeChanges::ReplaceMeshes { transition_key, .. } => {
                    //for key in keys_to_remove {
                    //    generation.debug_nodes.remove(&(*planet_entity, *key));
                    //}
                    //
                    //for request in requests {
                    //    generation
                    //        .debug_nodes
                    //        .insert((request.planet_entity, request.node_key), *request);
                    //}

                    let reserved_regions =
                        octree::replacement_footprint(&planet.surface_octree_root, &change);
                    submit_replacement(
                        change,
                        reserved_regions,
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
                        .send(PlanetGenerationCommand::ReplaceChunks {
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

    generation
        .dirty_terrain
        .retain(|entity, _| active_planets.contains(entity));
    generation
        .balanced_planets
        .retain(|entity| active_planets.contains(entity));
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

    if let Some((planet_position, planet_radius, solar_system)) = atmosphere_planet {
        if let Some(atmosphere) = globals
            .renderer
            .render_graph
            .get_node_mut::<AtmospherePassNode>(engine::renderer::ids::graph_passes::ATMOSPHERE)
        {
            atmosphere
                .settings
                .set_planet(planet_position.to_array(), planet_radius);
            if let Some((sun_transform,)) = transforms.get(solar_system) {
                // Preserve the last valid direction when the star and planet coincide.
                if let Some(direction) =
                    (sun_transform.position.as_dvec3() - planet_position.as_dvec3()).try_normalize()
                {
                    atmosphere.settings.sun_direction = direction.as_vec3().to_array();
                }
            }
        }
    }
}

pub fn apply_terrain_graph_changes(
    mut generation: ResMut<PlanetMeshGeneration>,
    mut apply_queue: ResMut<TerrainGraphApplyQueue>,
    game_state: Res<GameState>,
    camera: Res<GameCamera>,
    lod_settings: Res<PlanetLodSettings>,
    mut transforms: Query<(&TransformComponent,)>,
    globals: Globals,
    mut planets: Query<(
        &mut Planet,
        &PlanetTerrainEdits,
        &mut Arc<PlanetTerrainConfig>,
    )>,
) {
    // Do not clear visible terrain while rebuilding is paused.
    if !game_state.update_octree {
        return;
    }
    let Some((camera_transform,)) = transforms.get(camera.entity) else {
        return;
    };
    let camera_pos = camera_transform.position;
    let mut latest = HashMap::new();
    for request in std::mem::take(&mut apply_queue.requests) {
        latest.insert(request.target, request);
    }
    for request in latest.into_values() {
        let Some((planet, edits, config)) = planets.get(request.target) else {
            continue;
        };
        if generation
            .replacements
            .values()
            .any(|r| r.planet_entity == request.target)
        {
            apply_queue.requests.push(request);
            continue;
        }
        if let Some(error) = request.graph.validate().first() {
            engine::game_error!("Apply terrain graph failed: {}", error.message);
            continue;
        }
        let mut updated = (**config).clone();
        updated.seed = request.graph.seed;
        updated.radius = request.graph.radius as f32;
        updated.sea_level = request.graph.sea_level as f32;
        updated.field_graph = Some(request.graph);
        let Some(mut root) = fresh_terrain_root(planet.position, &updated, edits) else {
            engine::game_error!("Apply terrain graph failed: planet bounds exceed supported size");
            continue;
        };
        let requests = octree::prepare_terrain_replacement(
            &mut root,
            camera_pos,
            lod_settings.strength,
            request.target,
            planet.position,
            &updated,
            edits,
        );
        let completed_state = root.state;
        let change = OctreeChanges::ReplaceMeshes {
            planet_entity: request.target,
            transition_key: root.key,
            completed_state,
            additional_transitions: Vec::new(),
            keys_to_remove: Vec::new(),
            requests,
        };
        let reserved = vec![octree::node_bounds(&root)];
        root.state = NodeState::Splitting;
        *config = Arc::new(updated);
        planet.surface_octree_root = root;
        generation.density_caches.remove(&request.target);
        generation.balanced_planets.remove(&request.target);
        generation
            .debug_nodes
            .retain(|(entity, _), _| *entity != request.target);
        let replacement_id = generation.next_replacement_id;
        submit_replacement(
            change,
            reserved,
            &mut generation,
            &globals.job_system,
            config,
            &Arc::new(edits.clone()),
        );
        // Preserve all old GPU chunks until the final leaf meshes are ready,
        // including chunks outside the new root when the graph shrinks bounds.
        generation
            .replacements
            .get_mut(&replacement_id)
            .unwrap()
            .remove_all = true;
    }
}

pub fn fresh_terrain_root(
    position: Vec3,
    config: &PlanetTerrainConfig,
    edits: &PlanetTerrainEdits,
) -> Option<game_types::octree::OctreeNode> {
    let (_, max_height) = crate::sdf::terrain_height_bounds(config, None);
    let diameter = (config.radius + max_height) * 2.0;
    if !diameter.is_finite() || diameter <= 0.0 || !config.sea_level.is_finite() {
        return None;
    }
    // A root split should produce cells at least as large as the 32 m minimum.
    let size = (diameter.ceil() as u32)
        .max(64)
        .checked_next_power_of_two()? as f32;
    let min = position - Vec3::splat(size * 0.5);
    let density_range = octree::node_density_range(min, size, position, config, edits);
    Some(game_types::octree::OctreeNode {
        key: NodeKey {
            level: 0,
            x: min.x as i32,
            y: min.y as i32,
            z: min.z as i32,
        },
        min,
        size,
        children: None,
        vertex: None,
        density_range,
        may_contain_surface: density_range.contains_zero(),
        state: NodeState::Leaf,
    })
}

fn node_overlaps_bounds(node: &OctreeNode, bounds_min: Vec3, bounds_max: Vec3) -> bool {
    let node_max = node.min + vec3(node.size, node.size, node.size);

    node.min.x <= bounds_max.x
        && node_max.x >= bounds_min.x
        && node.min.y <= bounds_max.y
        && node_max.y >= bounds_min.y
        && node.min.z <= bounds_max.z
        && node_max.z >= bounds_min.z
}

fn collect_dirty_mesh_requests(
    node: &OctreeNode,
    planet_entity: Entity,
    planet_position: Vec3,
    bounds_min: Vec3,
    bounds_max: Vec3,
    requests: &mut Vec<PlanetMeshRequest>,
) {
    if !node_overlaps_bounds(node, bounds_min, bounds_max) {
        return;
    }

    if let Some(children) = node.children.as_ref() {
        for child in children {
            collect_dirty_mesh_requests(
                child,
                planet_entity,
                planet_position,
                bounds_min,
                bounds_max,
                requests,
            );
        }
        return;
    }

    requests.push(PlanetMeshRequest {
        planet_entity,
        node_key: node.key,
        planet_position,
        node_min_corner: node.min,
        node_size: node.size,
        face_neighbors: [FaceNeighbor::SAME_OR_ABSENT; 6],
    });
}

fn terrain_edit_requests(
    planet: &Planet,
    entity: Entity,
    bounds: octree::Aabb,
) -> Vec<PlanetMeshRequest> {
    let mut requests = Vec::new();
    collect_dirty_mesh_requests(
        &planet.surface_octree_root,
        entity,
        planet.position,
        bounds.min,
        bounds.max,
        &mut requests,
    );
    let mut keys: HashSet<_> = requests.iter().map(|r| r.node_key).collect();
    for dirty in requests.clone() {
        let mut neighbors = Vec::new();
        octree::collect_face_neighbor_leaves(
            &planet.surface_octree_root,
            dirty.node_min_corner,
            dirty.node_size,
            &mut neighbors,
        );
        for neighbor in neighbors {
            if neighbor.size != dirty.node_size && keys.insert(neighbor.key) {
                requests.push(PlanetMeshRequest {
                    planet_entity: entity,
                    node_key: neighbor.key,
                    planet_position: planet.position,
                    node_min_corner: neighbor.min,
                    node_size: neighbor.size,
                    face_neighbors: [FaceNeighbor::SAME_OR_ABSENT; 6],
                });
            }
        }
    }
    for request in &mut requests {
        octree::annotate_mesh_request(&planet.surface_octree_root, request);
    }
    requests.sort_unstable_by(|a, b| {
        let center = bounds.center();
        (a.node_min_corner + Vec3::splat(a.node_size * 0.5) - center)
            .length_squared()
            .total_cmp(
                &(b.node_min_corner + Vec3::splat(b.node_size * 0.5) - center).length_squared(),
            )
    });
    requests
}

fn submit_replacement(
    change: OctreeChanges,
    reserved_regions: Vec<octree::Aabb>,
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
            remove_all: false,
            reserved_regions,
            uniform_proofs: Vec::new(),

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
            .spawn_prioritized_named(
                "planet.mesh.generate",
                100_u32.saturating_sub((request.node_size / 32.0) as u32), // TODO: SEE IF IT MAKES ANY DIFFERENCE
                move || {
                    let (mesh, uniform) =
                        generate_terrain_node_mesh(&request, terrain_config, edits, &cache);

                    let _ = tx.send(CompletedMesh {
                        replacement_id,
                        mesh,
                        uniform,
                    });
                },
            )
            .unwrap();
    }
}

pub fn submit_addition(replacement: OctreeChanges) {}

pub fn drain_completed_requests(
    mut generation: ResMut<PlanetMeshGeneration>,
    queue: Res<PlanetTerrainRenderQueue>,
    events: Res<PlanetTerrainEvents>,
    mut planets: Query<(&mut Planet, &PlanetTerrainEdits)>,
) {
    for event in events.try_iter() {
        match event {
            PlanetTerrainEvent::ReplacementApplied {
                replacement_id: Some(id),
                rendered_keys,
                ..
            } => {
                if let Some(replacement) = generation.replacements.remove(&id) {
                    if let Some((planet, edits)) = planets.get(replacement.planet_entity) {
                        finish_transition(
                            &mut planet.surface_octree_root,
                            replacement.transition_key,
                            replacement.completed_state,
                        );
                        for (key, state) in replacement.additional_transitions {
                            finish_transition(&mut planet.surface_octree_root, key, state);
                        }
                        octree::apply_uniform_proofs(
                            &mut planet.surface_octree_root,
                            &replacement.uniform_proofs,
                            edits,
                        );
                        let rendered: HashSet<_> = rendered_keys.into_iter().collect();
                        if let Some((ancestor, descendant)) =
                            octree::rendered_overlap(&planet.surface_octree_root, &rendered)
                        {
                            engine::game_error!(
                                "Overlapping terrain LODs: {ancestor:?} and {descendant:?}"
                            );
                        }
                        let mut leaves = Vec::new();
                        octree::collect_leaf_nodes(&planet.surface_octree_root, &mut leaves);
                        let mut valid: HashSet<_> = leaves.iter().map(|node| node.key).collect();
                        for pending in generation
                            .replacements
                            .values()
                            .filter(|r| r.planet_entity == replacement.planet_entity)
                        {
                            valid.extend(pending.keys_to_remove.iter().copied());
                        }
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

        if completed.uniform.is_some() {
            replacement.uniform_proofs.push(completed.mesh.key);
        }
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
        .take(4) // Bound command submission; each replacement remains atomic.
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
            .map(|mesh| PendingChunkMesh {
                key: mesh.key,
                node_origin_planet: mesh.node_origin_planet,
                vertices: mesh.vertices,
                indices: mesh.indices,
            })
            .collect();

        queue
            .send(PlanetGenerationCommand::ReplaceChunks {
                planet: replacement.planet_entity,
                replacement_id: Some(replacement_id),
                remove_all: replacement.remove_all,
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
struct PendingReplacement {
    pub planet_entity: Entity,
    pub transition_key: NodeKey,
    pub completed_state: NodeState,
    pub additional_transitions: Vec<(NodeKey, NodeState)>,
    pub keys_to_remove: Vec<NodeKey>,
    pub submitted: bool,
    pub remove_all: bool,
    #[reflect(ignore)]
    pub reserved_regions: Vec<octree::Aabb>,
    pub uniform_proofs: Vec<NodeKey>,

    pub expected_meshes: usize,
    pub meshes: Vec<GeneratedMesh>,
}

struct CompletedMesh {
    pub uniform: Option<super::planet_mesher::cache::UniformRegion>,
    pub replacement_id: u64,
    pub mesh: GeneratedMesh,
}

#[derive(Reflect)]
// Ignored channel endpoints cannot be defaulted independently by FromReflect.
#[reflect(from_reflect = false)]
pub struct PlanetMeshGeneration {
    pub next_replacement_id: u64,

    #[reflect(ignore)]
    dirty_terrain: HashMap<Entity, octree::Aabb>,

    #[reflect(ignore)]
    pub density_caches: HashMap<Entity, Arc<DensityCache>>,

    #[reflect(ignore)]
    pub balanced_planets: HashSet<Entity>,

    pub replacements: HashMap<u64, PendingReplacement>,

    #[reflect(ignore)]
    pub completed_tx: crossbeam_channel::Sender<CompletedMesh>,
    #[reflect(ignore)]
    pub completed_rx: crossbeam_channel::Receiver<CompletedMesh>,

    pub debug_nodes: HashMap<(Entity, NodeKey), PlanetMeshRequest>,
}

impl PlanetMeshGeneration {
    pub fn queue_terrain_edit(&mut self, planet: Entity, min: Vec3, max: Vec3) {
        self.dirty_terrain
            .entry(planet)
            .and_modify(|bounds| {
                bounds.min = bounds.min.min(min);
                bounds.max = bounds.max.max(max);
            })
            .or_insert(octree::Aabb { min, max });
        self.balanced_planets.remove(&planet);
    }
}

impl Default for PlanetMeshGeneration {
    fn default() -> Self {
        let (completed_tx, completed_rx) = crossbeam_channel::unbounded();

        Self {
            next_replacement_id: 0,
            dirty_terrain: HashMap::new(),
            density_caches: HashMap::new(),
            balanced_planets: HashSet::new(),
            replacements: HashMap::new(),
            completed_tx,
            completed_rx,
            debug_nodes: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod apply_tests {
    use super::*;

    #[test]
    fn changed_graph_resets_bounds_and_starts_mesh_generation() {
        let mut config =
            crate::systems::universe::planet_system::earth_like_planet_terrain_config();
        let graph = config.field_graph.as_mut().unwrap();
        graph.layers.clear();
        graph.radius = 1000.0;
        config.radius = 1000.0;
        let edits = PlanetTerrainEdits {
            modified_chunks: HashMap::new(),
            modified_ranges: HashMap::new(),
        };
        let position = vec3(10000.0, -5000.0, 2000.0);
        let old = fresh_terrain_root(position, &config, &edits).unwrap();
        config.radius = 4000.0;
        config.field_graph.as_mut().unwrap().radius = 4000.0;
        let mut root = fresh_terrain_root(position, &config, &edits).unwrap();
        assert!(root.size > old.size);
        assert!(root.children.is_none() && matches!(root.state, NodeState::Leaf));
        assert!(root.may_contain_surface);
        assert!(
            (position - Vec3::splat(config.radius))
                .cmpge(root.min)
                .all()
        );
        assert!(
            (position + Vec3::splat(config.radius))
                .cmple(root.min + Vec3::splat(root.size))
                .all()
        );
        let camera = position + Vec3::X * 4010.0;
        let requests = octree::prepare_terrain_replacement(
            &mut root,
            camera,
            1.0,
            Entity::PLACEHOLDER,
            position,
            &config,
            &edits,
        );
        assert!(!requests.is_empty());
        assert!(root.children.is_some() && matches!(root.state, NodeState::Internal));
        let mut leaves = Vec::new();
        octree::collect_leaf_nodes(&root, &mut leaves);
        assert_eq!(
            requests.len(),
            leaves.iter().filter(|n| n.may_contain_surface).count()
        );
        assert!(requests.iter().any(|r| r.node_size == 32.0));
        for leaf in leaves {
            assert!(!octree::should_split(leaf, camera, 32.0, 1.0));
            let mut neighbors = Vec::new();
            octree::collect_face_neighbor_leaves(&root, leaf.min, leaf.size, &mut neighbors);
            assert!(neighbors.iter().all(|n| n.size <= leaf.size * 2.0));
        }
        for request in requests {
            let node = octree::find_node_mut(&mut root, request.node_key).unwrap();
            assert!(node.children.is_none() && node.may_contain_surface);
        }
    }

    #[test]
    fn edit_remesh_includes_empty_leaves_on_shared_boundaries() {
        let config = crate::systems::universe::planet_system::default_planet_terrain_config();
        let edits = PlanetTerrainEdits {
            modified_chunks: HashMap::new(),
            modified_ranges: HashMap::new(),
        };
        let mut root = fresh_terrain_root(Vec3::ZERO, &config, &edits).unwrap();
        root.min = Vec3::splat(-32.0);
        root.size = 64.0;
        root.children = Some(std::array::from_fn(|i| {
            let min = root.min
                + vec3((i & 1) as f32, ((i >> 1) & 1) as f32, ((i >> 2) & 1) as f32) * 32.0;
            Box::new(OctreeNode {
                key: NodeKey {
                    level: 1,
                    x: min.x as i32,
                    y: min.y as i32,
                    z: min.z as i32,
                },
                min,
                size: 32.0,
                children: None,
                vertex: None,
                density_range: game_types::octree::DensityRange::new(1.0, 2.0),
                may_contain_surface: false,
                state: NodeState::Leaf,
            })
        }));
        let mut requests = Vec::new();
        // Adding terrain can create a surface in a previously empty leaf.
        // An edit at a shared corner must also rebuild all touching chunks.
        collect_dirty_mesh_requests(
            &root,
            Entity::PLACEHOLDER,
            Vec3::ZERO,
            Vec3::ZERO,
            Vec3::ONE,
            &mut requests,
        );
        assert_eq!(requests.len(), 8);
        assert_eq!(
            requests
                .iter()
                .map(|r| r.node_key)
                .collect::<HashSet<_>>()
                .len(),
            8
        );
    }

    #[test]
    fn unsupported_radius_does_not_create_a_root() {
        let mut config = crate::systems::universe::planet_system::default_planet_terrain_config();
        let edits = PlanetTerrainEdits {
            modified_chunks: HashMap::new(),
            modified_ranges: HashMap::new(),
        };
        for radius in [f32::INFINITY, f32::MAX, 3_000_000_000.0] {
            config.radius = radius;
            assert!(fresh_terrain_root(Vec3::ZERO, &config, &edits).is_none());
        }
    }
}
