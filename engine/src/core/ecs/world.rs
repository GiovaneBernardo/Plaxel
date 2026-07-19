use std::any::type_name;

use crate::ecs::{
    change::{ChangeCursor, ChangeTick, WorldChange, WorldChangeKind},
    component::Component,
    entity::{Entities, Entity},
    resource::{Resource, Resources},
    storage::{Storage, Storages},
};

pub struct World {
    entities: Entities,
    storages: Storages,
    resources: Resources,
    change_tick: ChangeTick,
    change_log: Vec<WorldChange>,
}

impl World {
    pub fn new() -> Self {
        Self {
            entities: Entities::new(),
            storages: Storages::new(),
            resources: Resources::new(),
            change_tick: ChangeTick(1),
            change_log: Vec::new(),
        }
    }

    pub fn spawn(&mut self) -> Entity {
        self.entities.allocate()
    }

    pub fn insert<T: Component>(&mut self, entity: Entity, component: T) {
        let mut storage = self.storages.ensure_storage::<T>();

        storage.insert(entity, component, self.change_tick);
        self.change_log.push(WorldChange {
            tick: self.change_tick,
            entity,
            component_type: Some(type_name::<T>()),
            kind: WorldChangeKind::Inserted,
        });
    }

    pub fn remove<T: Component>(&mut self, entity: Entity) -> Option<T> {
        let removed = self.storages.remove::<T>(entity);
        if removed.is_some() {
            self.change_log.push(WorldChange {
                tick: self.change_tick,
                entity,
                component_type: Some(type_name::<T>()),
                kind: WorldChangeKind::Removed,
            });
        }
        removed
    }

    pub fn despawn(&mut self, entity: Entity) -> bool {
        if !self.entities.contains(entity) {
            return false;
        }

        self.storages.remove_entity_from_all(entity);
        self.change_log.push(WorldChange {
            tick: self.change_tick,
            entity,
            component_type: None,
            kind: WorldChangeKind::Despawned,
        });
        self.entities.deallocate(entity)
    }

    pub fn insert_resource<T: Resource>(&mut self, resource: T) {
        self.resources.insert(resource);
    }

    pub fn get<T: Component>(&self, entity: Entity) -> Option<std::cell::Ref<'_, T>> {
        let storage = self.storages.get_storage::<T>()?;

        std::cell::Ref::filter_map(storage, |s| s.get(entity)).ok()
    }

    pub fn get_mut<T: Component>(&self, entity: Entity) -> Option<std::cell::RefMut<'_, T>> {
        let storage = self.storages.get_storage_mut::<T>()?;

        std::cell::RefMut::filter_map(storage, |s| s.get_mut(entity, self.change_tick)).ok()
    }

    pub fn get_resource<T: Resource>(&self) -> Option<std::cell::Ref<'_, T>> {
        self.resources.get::<T>()
    }

    pub fn get_resource_mut<T: Resource>(&self) -> Option<std::cell::RefMut<'_, T>> {
        self.resources.get_mut::<T>()
    }

    pub fn entities(&self) -> &Entities {
        &self.entities
    }

    pub fn get_storage<T: Component>(&self) -> Option<std::cell::Ref<'_, Storage<T>>> {
        self.storages.get_storage::<T>()
    }

    pub fn get_storage_mut<T: Component>(&self) -> Option<std::cell::RefMut<'_, Storage<T>>> {
        self.storages.get_storage_mut::<T>()
    }

    pub fn change_tick(&self) -> ChangeTick {
        self.change_tick
    }

    pub fn advance_change_tick(&mut self) -> ChangeTick {
        self.change_tick.0 = self.change_tick.0.wrapping_add(1);
        self.change_tick
    }

    pub fn changes_since<'a>(
        &'a self,
        cursor: &ChangeCursor,
    ) -> impl Iterator<Item = &'a WorldChange> {
        let now = self.change_tick;
        let last = cursor.tick;
        self.change_log
            .iter()
            .filter(move |change| change.tick.is_newer_than(last, now))
    }

    pub fn acknowledge_changes(&mut self, cursor: &mut ChangeCursor) {
        cursor.tick = self.change_tick;
    }

    /// Removes journal entries after every interested consumer has advanced past `tick`.
    /// Individual cursors must never clear the shared journal on their own.
    pub fn compact_changes_through(&mut self, tick: ChangeTick) {
        let now = self.change_tick;
        self.change_log
            .retain(|change| change.tick.is_newer_than(tick, now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct TestComponent(u32);

    #[test]
    fn independent_change_cursors_do_not_consume_each_others_events() {
        let mut world = World::new();
        let entity = world.spawn();
        world.insert(entity, TestComponent(1));

        let mut renderer = ChangeCursor::default();
        let editor = ChangeCursor::default();
        assert_eq!(world.changes_since(&renderer).count(), 1);
        world.acknowledge_changes(&mut renderer);

        assert_eq!(world.changes_since(&renderer).count(), 0);
        assert_eq!(world.changes_since(&editor).count(), 1);
    }

    #[test]
    fn journal_compaction_is_explicit() {
        let mut world = World::new();
        let entity = world.spawn();
        world.insert(entity, TestComponent(1));
        let through = world.change_tick();
        world.compact_changes_through(through);

        assert_eq!(world.changes_since(&ChangeCursor::default()).count(), 0);
    }
}
