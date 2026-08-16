use std::{
    any::type_name,
    collections::{HashMap, HashSet},
};

use crate::prelude::*;

use crate::{
    assets::{
        manager::{AssetManager, Handle},
        material::Material,
    },
    core::components::{core::TransformComponent, renderer::MeshRendererComponent},
    ecs::{
        change::{ChangeCursor, WorldChangeKind},
        entity::Entity,
        world::World,
    },
    math::{Mat4, Vec3},
    model::{MeshAsset, TransformInstance},
    renderer::{
        BindGroupHandle,
        ids::{MaterialPassId, RenderPhaseId, phases},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderObjectId {
    index: u32,
    generation: u32,
}
impl RenderObjectId {
    pub fn index(self) -> usize {
        self.index as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RenderFlags(pub u32);
impl RenderFlags {
    pub const VISIBLE_MAIN: Self = Self(1 << 0);
    pub const DEPTH_PREPASS: Self = Self(1 << 1);
    pub const CASTS_SHADOWS: Self = Self(1 << 2);
    pub const RECEIVES_SHADOWS: Self = Self(1 << 3);
    pub const STATIC: Self = Self(1 << 4);

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub const fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }
}
impl std::ops::BitOr for RenderFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}
impl std::ops::BitOrAssign for RenderFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipelineOverride {
    pub material_pass: MaterialPassId,
    pub pipeline: PipelineHandle,
}

#[derive(Clone)]
pub struct RenderObject {
    pub mesh: Handle<MeshAsset>,
    pub material: Material,
    pub transform: TransformInstance,
    /// Ordering bucket for the main surface pass (opaque, water, transparent, ...).
    /// Depth and shadow participation are controlled independently by `flags`.
    pub main_phase: RenderPhaseId,
    pub flags: RenderFlags,
    pub extra_bind_groups: Vec<(u32, BindGroupHandle)>,
    /// Rare pass-specific overrides for custom layouts. Most objects leave this empty.
    pub pipeline_overrides: Vec<PipelineOverride>,
}

impl RenderObject {
    /// Sensible default for an ordinary opaque object. Specialized renderers can change the
    /// phase/flags or bypass retained objects entirely by implementing `RenderProducer`.
    pub fn new(mesh: Handle<MeshAsset>, material: Material, transform: TransformInstance) -> Self {
        Self {
            mesh,
            material,
            transform,
            main_phase: phases::OPAQUE,
            flags: RenderFlags::VISIBLE_MAIN
                | RenderFlags::DEPTH_PREPASS
                | RenderFlags::CASTS_SHADOWS
                | RenderFlags::RECEIVES_SHADOWS,
            extra_bind_groups: Vec::new(),
            pipeline_overrides: Vec::new(),
        }
    }

    pub fn with_phase(mut self, phase: RenderPhaseId) -> Self {
        self.main_phase = phase;
        self
    }

    pub fn with_flags(mut self, flags: RenderFlags) -> Self {
        self.flags = flags;
        self
    }

    pub fn with_bind_group(mut self, group: u32, binding: BindGroupHandle) -> Self {
        self.extra_bind_groups.push((group, binding));
        self
    }

    pub fn with_bind_groups(
        mut self,
        bindings: impl IntoIterator<Item = (u32, BindGroupHandle)>,
    ) -> Self {
        self.extra_bind_groups.extend(bindings);
        self
    }

    pub fn with_pipeline_override(
        mut self,
        material_pass: MaterialPassId,
        pipeline: PipelineHandle,
    ) -> Self {
        self.pipeline_overrides.push(PipelineOverride {
            material_pass,
            pipeline,
        });
        self
    }

    pub fn pipeline_override(&self, pass: MaterialPassId) -> Option<PipelineHandle> {
        self.pipeline_overrides
            .iter()
            .find_map(|item| (item.material_pass == pass).then_some(item.pipeline))
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub struct RenderDatabaseStats {
    pub inserted: u64,
    pub updated: u64,
    pub removed: u64,
    pub dirty_slots: usize,
    pub structural_revision: u64,
}

struct ObjectSlot {
    generation: u32,
    object: Option<RenderObject>,
}

pub struct RenderDatabase {
    slots: Vec<ObjectSlot>,
    free: Vec<u32>,
    entity_objects: HashMap<Entity, RenderObjectId>,
    dirty: Vec<bool>,
    structural_revision: u64,
    phase_cache_revision: u64,
    phase_cache: HashMap<RenderPhaseId, Vec<RenderObjectId>>,
    ecs_cursor: ChangeCursor,
    stats: RenderDatabaseStats,
}

impl RenderDatabase {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            entity_objects: HashMap::new(),
            dirty: Vec::new(),
            structural_revision: 1,
            phase_cache_revision: 0,
            phase_cache: HashMap::new(),
            ecs_cursor: ChangeCursor::default(),
            stats: RenderDatabaseStats::default(),
        }
    }

    pub fn insert(&mut self, object: RenderObject) -> RenderObjectId {
        let index = self.free.pop().unwrap_or(self.slots.len() as u32);
        if index as usize == self.slots.len() {
            self.slots.push(ObjectSlot {
                generation: 1,
                object: Some(object),
            });
            self.dirty.push(true);
        } else {
            self.slots[index as usize].object = Some(object);
            self.dirty[index as usize] = true;
        }
        let id = RenderObjectId {
            index,
            generation: self.slots[index as usize].generation,
        };
        self.structural_revision = self.structural_revision.wrapping_add(1);
        self.stats.inserted += 1;
        id
    }

    pub fn get(&self, id: RenderObjectId) -> Option<&RenderObject> {
        let slot = self.slots.get(id.index())?;
        (slot.generation == id.generation)
            .then_some(slot.object.as_ref())
            .flatten()
    }

    pub fn update(&mut self, id: RenderObjectId, object: RenderObject) -> bool {
        let Some(slot) = self.slots.get_mut(id.index()) else {
            return false;
        };
        if slot.generation != id.generation || slot.object.is_none() {
            return false;
        }
        let previous = slot.object.as_ref().unwrap();
        let structural = previous.mesh != object.mesh
            || previous.material.uuid != object.material.uuid
            || previous.main_phase != object.main_phase
            || previous.flags != object.flags
            || previous.extra_bind_groups != object.extra_bind_groups
            || previous.pipeline_overrides != object.pipeline_overrides;
        slot.object = Some(object);
        self.dirty[id.index()] = true;
        if structural {
            self.structural_revision = self.structural_revision.wrapping_add(1);
        }
        self.stats.updated += 1;
        true
    }

    pub fn update_transform(&mut self, id: RenderObjectId, transform: TransformInstance) -> bool {
        let Some(slot) = self.slots.get_mut(id.index()) else {
            return false;
        };
        if slot.generation != id.generation {
            return false;
        }
        let Some(object) = slot.object.as_mut() else {
            return false;
        };
        object.transform = transform;
        self.dirty[id.index()] = true;
        self.stats.updated += 1;
        true
    }

    pub fn remove(&mut self, id: RenderObjectId) -> bool {
        let Some(slot) = self.slots.get_mut(id.index()) else {
            return false;
        };
        if slot.generation != id.generation || slot.object.take().is_none() {
            return false;
        }
        slot.generation = slot.generation.wrapping_add(1).max(1);
        self.free.push(id.index);
        self.dirty[id.index()] = true;
        self.structural_revision = self.structural_revision.wrapping_add(1);
        self.stats.removed += 1;
        true
    }

    pub fn object_for_entity(&self, entity: Entity) -> Option<RenderObjectId> {
        self.entity_objects.get(&entity).copied()
    }

    pub fn phase_objects(&mut self, phase: RenderPhaseId) -> &[RenderObjectId] {
        self.rebuild_phase_cache();
        self.phase_cache
            .get(&phase)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn rebuild_phase_cache(&mut self) {
        if self.phase_cache_revision == self.structural_revision {
            return;
        }
        self.phase_cache.clear();
        for (index, slot) in self.slots.iter().enumerate() {
            let Some(object) = &slot.object else {
                continue;
            };
            self.phase_cache
                .entry(object.main_phase)
                .or_default()
                .push(RenderObjectId {
                    index: index as u32,
                    generation: slot.generation,
                });
        }
        for objects in self.phase_cache.values_mut() {
            objects.sort_unstable_by_key(|id| {
                let object = self.slots[id.index()].object.as_ref().unwrap();
                (object.material.uuid, object.mesh.uuid)
            });
        }
        self.phase_cache_revision = self.structural_revision;
    }

    pub fn take_dirty_ranges(&mut self) -> Vec<std::ops::Range<usize>> {
        let mut ranges = Vec::new();
        let mut index = 0;
        while index < self.dirty.len() {
            if !self.dirty[index] {
                index += 1;
                continue;
            }
            let start = index;
            while index < self.dirty.len() && self.dirty[index] {
                self.dirty[index] = false;
                index += 1;
            }
            ranges.push(start..index);
        }
        self.stats.dirty_slots = ranges.iter().map(|range| range.len()).sum();
        ranges
    }

    pub fn gpu_transform_at(&self, index: usize) -> TransformInstance {
        self.slots
            .get(index)
            .and_then(|slot| slot.object.as_ref())
            .map(|object| object.transform)
            .unwrap_or(TransformInstance {
                model_matrix: Mat4::IDENTITY.to_cols_array_2d(),
                material_index: 0,
            })
    }
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }
    pub fn structural_revision(&self) -> u64 {
        self.structural_revision
    }
    pub fn stats(&self) -> RenderDatabaseStats {
        RenderDatabaseStats {
            structural_revision: self.structural_revision,
            ..self.stats
        }
    }

    pub fn sync_ecs(&mut self, world: &mut World, assets: &AssetManager) {
        let last = self.ecs_cursor.tick;
        let now = world.change_tick();
        let changed_entities = {
            crate::profile_scope!("render_database.collect_changed_entities");
            let mut changed_entities = HashSet::new();
            if let Some(mesh_renderers) = world.get_storage::<MeshRendererComponent>() {
                for (entity, _) in mesh_renderers.iter_changed_since(last, now) {
                    changed_entities.insert(entity);
                }
            }
            if let Some(transforms) = world.get_storage::<TransformComponent>() {
                for (entity, _) in transforms.iter_changed_since(last, now) {
                    changed_entities.insert(entity);
                }
            }
            changed_entities
        };
        crate::profile_counter!(
            "render_database.changed_entities",
            changed_entities.len() as f64
        );

        {
            crate::profile_scope!("render_database.apply_changed_entities");
            for entity in changed_entities {
                let Some(mesh_renderer) = world.get::<MeshRendererComponent>(entity) else {
                    if let Some(id) = self.entity_objects.remove(&entity) {
                        self.remove(id);
                    }
                    continue;
                };
                let Some(transform) = world.get::<TransformComponent>(entity) else {
                    if let Some(id) = self.entity_objects.remove(&entity) {
                        self.remove(id);
                    }
                    continue;
                };
                let Some(material) = assets.get_by_uuid::<Material>(mesh_renderer.material) else {
                    if let Some(id) = self.entity_objects.remove(&entity) {
                        self.remove(id);
                    }
                    continue;
                };
                let object = RenderObject::new(
                    mesh_renderer.mesh,
                    material.clone(),
                    transform_instance(&transform, material.material_index),
                );
                if let Some(id) = self.entity_objects.get(&entity).copied() {
                    self.update(id, object);
                } else {
                    let id = self.insert(object);
                    self.entity_objects.insert(entity, id);
                }
            }
        }
        let removals: Vec<_> = {
            crate::profile_scope!("render_database.collect_removals");
            world
                .changes_since(&self.ecs_cursor)
                .filter_map(|change| {
                    let removes_renderer = change.kind == WorldChangeKind::Despawned
                        || (change.kind == WorldChangeKind::Removed
                            && matches!(change.component_type,
                            Some(name) if name == type_name::<MeshRendererComponent>()
                                || name == type_name::<TransformComponent>()));
                    removes_renderer.then_some(change.entity)
                })
                .collect()
        };
        crate::profile_counter!("render_database.removals", removals.len() as f64);
        {
            crate::profile_scope!("render_database.apply_removals");
            for entity in removals {
                if let Some(id) = self.entity_objects.remove(&entity) {
                    self.remove(id);
                }
            }
        }
        {
            crate::profile_scope!("render_database.acknowledge_changes");
            world.acknowledge_changes(&mut self.ecs_cursor);
        }
    }
}
impl Default for RenderDatabase {
    fn default() -> Self {
        Self::new()
    }
}

pub struct RenderObjectWriter<'a> {
    database: &'a mut RenderDatabase,
}
impl<'a> RenderObjectWriter<'a> {
    pub fn new(database: &'a mut RenderDatabase) -> Self {
        Self { database }
    }
    pub fn insert(&mut self, object: RenderObject) -> RenderObjectId {
        self.database.insert(object)
    }
    pub fn update(&mut self, id: RenderObjectId, object: RenderObject) -> bool {
        self.database.update(id, object)
    }
    pub fn remove(&mut self, id: RenderObjectId) -> bool {
        self.database.remove(id)
    }
    pub fn update_transform(&mut self, id: RenderObjectId, transform: TransformInstance) -> bool {
        self.database.update_transform(id, transform)
    }
}

fn transform_instance(transform: &TransformComponent, material_index: u32) -> TransformInstance {
    let model_matrix = Mat4::from_translation(transform.position)
        * Mat4::from_quat(transform.rotation)
        * Mat4::from_scale(Vec3::new(
            transform.scale.x,
            transform.scale.y,
            transform.scale.z,
        ));
    TransformInstance {
        model_matrix: model_matrix.to_cols_array_2d(),
        material_index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::manager::AssetType;
    use uuid::Uuid;

    fn object() -> RenderObject {
        RenderObject {
            mesh: Handle {
                uuid: Uuid::new_v4(),
                asset_type: AssetType::Mesh,
                _marker: std::marker::PhantomData,
            },
            material: Material::new("shaders/test.wgsl".into()),
            transform: TransformInstance {
                model_matrix: Mat4::IDENTITY.to_cols_array_2d(),
                material_index: 0,
            },
            main_phase: phases::OPAQUE,
            flags: RenderFlags::VISIBLE_MAIN,
            extra_bind_groups: Vec::new(),
            pipeline_overrides: Vec::new(),
        }
    }

    #[test]
    fn reused_slots_invalidate_stale_object_ids() {
        let mut database = RenderDatabase::new();
        let stale = database.insert(object());
        assert!(database.remove(stale));
        let replacement = database.insert(object());

        assert_eq!(stale.index(), replacement.index());
        assert!(database.get(stale).is_none());
        assert!(database.get(replacement).is_some());
    }

    #[test]
    fn transform_updates_do_not_force_structural_rebuilds() {
        let mut database = RenderDatabase::new();
        let id = database.insert(object());
        database.take_dirty_ranges();
        let revision = database.structural_revision();
        let mut transform = database.get(id).unwrap().transform;
        transform.model_matrix[3][0] = 42.0;

        assert!(database.update_transform(id, transform));
        assert_eq!(database.structural_revision(), revision);
        assert_eq!(
            database.take_dirty_ranges(),
            vec![id.index()..id.index() + 1]
        );
    }
}
