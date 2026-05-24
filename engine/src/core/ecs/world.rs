use crate::ecs::{
    component::Component,
    entity::{Entities, Entity},
    resource::{Resource, Resources},
    storage::{Storage, Storages},
};

pub struct World {
    entities: Entities,
    storages: Storages,
    resources: Resources,
}

impl World {
    pub fn new() -> Self {
        Self {
            entities: Entities::new(),
            storages: Storages::new(),
            resources: Resources::new(),
        }
    }

    pub fn spawn(&mut self) -> Entity {
        self.entities.allocate()
    }

    pub fn insert<T: Component>(&mut self, entity: Entity, component: T) {
        let mut storage = self.storages.ensure_storage::<T>();
        storage.insert(entity, component);
    }

    pub fn remove<T: Component>(&mut self, entity: Entity) -> Option<T> {
        self.storages.remove::<T>(entity)
    }

    pub fn despawn(&mut self, entity: Entity) -> bool {
        if !self.entities.contains(entity) {
            return false;
        }

        self.storages.remove_entity_from_all(entity);
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

        std::cell::RefMut::filter_map(storage, |s| s.get_mut(entity)).ok()
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
}
