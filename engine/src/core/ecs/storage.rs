use crate::ecs::change::ChangeTick;
use crate::ecs::component::Component;
use crate::ecs::entity::Entity;
use crate::reflect::{PartialReflect, Reflect};
use std::any::type_name;
use std::cell::RefCell;
use std::collections::HashMap;

trait ErasedStorage {
    fn as_ptr(&self) -> *const ();
    fn as_mut_ptr(&mut self) -> *mut ();
    fn remove_entity(&mut self, entity: Entity);
    fn reflected_component_mut(
        &mut self,
        entity: Entity,
        tick: ChangeTick,
    ) -> Option<&mut dyn PartialReflect>;
}

struct ReflectedStorage<T>(Storage<T>);

impl<T: Component + Reflect> ErasedStorage for ReflectedStorage<T> {
    fn as_ptr(&self) -> *const () {
        &self.0 as *const Storage<T> as *const ()
    }

    fn as_mut_ptr(&mut self) -> *mut () {
        &mut self.0 as *mut Storage<T> as *mut ()
    }

    fn remove_entity(&mut self, entity: Entity) {
        self.0.remove(entity);
    }

    fn reflected_component_mut(
        &mut self,
        entity: Entity,
        tick: ChangeTick,
    ) -> Option<&mut dyn PartialReflect> {
        self.0
            .get_mut(entity, tick)
            .map(|value| value as &mut dyn PartialReflect)
    }
}

struct OpaqueStorage<T>(Storage<T>);

impl<T: Component> ErasedStorage for OpaqueStorage<T> {
    fn as_ptr(&self) -> *const () {
        &self.0 as *const Storage<T> as *const ()
    }

    fn as_mut_ptr(&mut self) -> *mut () {
        &mut self.0 as *mut Storage<T> as *mut ()
    }

    fn remove_entity(&mut self, entity: Entity) {
        self.0.remove(entity);
    }

    fn reflected_component_mut(
        &mut self,
        _entity: Entity,
        _tick: ChangeTick,
    ) -> Option<&mut dyn PartialReflect> {
        None
    }
}

pub struct Storage<T> {
    sparse: Vec<u32>,
    dense: Vec<T>,
    entities: Vec<Entity>,
    added_ticks: Vec<ChangeTick>,
    changed_ticks: Vec<ChangeTick>,
}

impl<T> Storage<T> {
    pub fn new() -> Self {
        Self {
            sparse: Vec::new(),
            dense: Vec::new(),
            entities: Vec::new(),
            added_ticks: Vec::new(),
            changed_ticks: Vec::new(),
        }
    }

    pub fn insert(&mut self, entity: Entity, component: T, tick: ChangeTick) {
        let idx = entity.index() as usize;

        // ensure sparse is large enough
        if self.sparse.len() <= idx {
            self.sparse.resize(idx + 1, u32::MAX);
        }

        let dense_idx = self.sparse[idx];

        // Case 1: entity already has component → overwrite
        if dense_idx != u32::MAX {
            self.dense[dense_idx as usize] = component;
            self.changed_ticks[dense_idx as usize] = tick;
            return;
        }

        // Case 2: new insert
        let new_dense_idx = self.dense.len() as u32;

        self.dense.push(component);
        self.entities.push(entity);
        self.added_ticks.push(tick);
        self.changed_ticks.push(tick);
        self.sparse[idx] = new_dense_idx;
    }

    pub fn get(&self, entity: Entity) -> Option<&T> {
        let idx = entity.index() as usize;

        let dense_idx = *self.sparse.get(idx)?;

        if dense_idx == u32::MAX {
            return None;
        }

        if self.entities.get(dense_idx as usize).copied()? != entity {
            return None;
        }

        self.dense.get(dense_idx as usize)
    }

    pub fn get_mut(&mut self, entity: Entity, tick: ChangeTick) -> Option<&mut T> {
        let idx = entity.index() as usize;

        let dense_idx = *self.sparse.get(idx)?;

        if dense_idx == u32::MAX {
            return None;
        }

        if self.entities.get(dense_idx as usize).copied()? != entity {
            return None;
        }

        self.changed_ticks[dense_idx as usize] = tick;
        self.dense.get_mut(dense_idx as usize)
    }

    pub fn remove(&mut self, entity: Entity) -> Option<T> {
        let sparse_idx = entity.index() as usize;
        let dense_idx = *self.sparse.get(sparse_idx)?;

        if dense_idx == u32::MAX {
            return None;
        }

        let dense_idx = dense_idx as usize;

        if self.entities.get(dense_idx).copied()? != entity {
            return None;
        }

        self.sparse[sparse_idx] = u32::MAX;

        let removed_component = self.dense.swap_remove(dense_idx);
        self.entities.swap_remove(dense_idx);
        self.added_ticks.swap_remove(dense_idx);
        self.changed_ticks.swap_remove(dense_idx);

        if dense_idx < self.entities.len() {
            let moved_entity = self.entities[dense_idx];
            self.sparse[moved_entity.index() as usize] = dense_idx as u32;
        }

        Some(removed_component)
    }

    pub fn dense(&self) -> &Vec<T> {
        &self.dense
    }

    pub fn dense_mut(&mut self, tick: ChangeTick) -> &mut Vec<T> {
        self.changed_ticks.fill(tick);
        &mut self.dense
    }

    pub fn entities(&self) -> &Vec<Entity> {
        &self.entities
    }

    pub fn entities_mut(&mut self) -> &mut Vec<Entity> {
        &mut self.entities
    }

    pub fn iter(&self) -> impl Iterator<Item = (Entity, &T)> {
        self.entities.iter().copied().zip(self.dense.iter())
    }

    pub fn iter_mut(&mut self, tick: ChangeTick) -> impl Iterator<Item = (Entity, &mut T)> {
        self.changed_ticks.fill(tick);
        self.entities.iter().copied().zip(self.dense.iter_mut())
    }

    pub fn iter_added_since(
        &self,
        last_run: ChangeTick,
        this_run: ChangeTick,
    ) -> impl Iterator<Item = (Entity, &T)> {
        self.iter_with_ticks(&self.added_ticks, last_run, this_run)
    }

    pub fn iter_changed_since(
        &self,
        last_run: ChangeTick,
        this_run: ChangeTick,
    ) -> impl Iterator<Item = (Entity, &T)> {
        self.iter_with_ticks(&self.changed_ticks, last_run, this_run)
    }

    fn iter_with_ticks<'a>(
        &'a self,
        ticks: &'a [ChangeTick],
        last_run: ChangeTick,
        this_run: ChangeTick,
    ) -> impl Iterator<Item = (Entity, &'a T)> + 'a {
        self.entities
            .iter()
            .copied()
            .zip(self.dense.iter())
            .zip(ticks.iter().copied())
            .filter_map(move |((entity, value), tick)| {
                tick.is_newer_than(last_run, this_run)
                    .then_some((entity, value))
            })
    }
}

pub struct Storages {
    map: HashMap<&'static str, RefCell<Box<dyn ErasedStorage>>>,
}

impl Storages {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn get_storage<T: Component>(&self) -> Option<std::cell::Ref<'_, Storage<T>>> {
        self.map.get(type_name::<T>()).map(|cell| {
            std::cell::Ref::map(cell.borrow(), |boxed| unsafe {
                // Hot-reloaded DLL generations get different TypeIds for the same
                // source component. The stable key above is the guard; changing
                // component layout still requires a restart.
                &*(boxed.as_ptr() as *const Storage<T>)
            })
        })
    }

    pub fn get_storage_mut<T: Component>(&self) -> Option<std::cell::RefMut<'_, Storage<T>>> {
        self.map.get(type_name::<T>()).map(|cell| {
            std::cell::RefMut::map(cell.borrow_mut(), |boxed| unsafe {
                // See `get_storage` for the hot-reload TypeId rationale.
                &mut *(boxed.as_mut_ptr() as *mut Storage<T>)
            })
        })
    }

    pub fn remove<T: Component>(&mut self, entity: Entity) -> Option<T> {
        let cell = self.map.get_mut(type_name::<T>())?;
        let mut storage = cell.borrow_mut();

        unsafe { &mut *(storage.as_mut_ptr() as *mut Storage<T>) }.remove(entity)
    }

    pub fn remove_entity_from_all(&mut self, entity: Entity) {
        for cell in self.map.values_mut() {
            cell.borrow_mut().remove_entity(entity);
        }
    }

    pub fn ensure_storage<T: Component + Reflect>(&mut self) -> std::cell::RefMut<'_, Storage<T>> {
        let type_name = type_name::<T>();

        let cell = self
            .map
            .entry(type_name)
            .or_insert_with(|| RefCell::new(Box::new(ReflectedStorage(Storage::<T>::new()))));

        std::cell::RefMut::map(cell.borrow_mut(), |boxed| unsafe {
            // See `get_storage` for the hot-reload TypeId rationale.
            &mut *(boxed.as_mut_ptr() as *mut Storage<T>)
        })
    }

    pub fn ensure_opaque_storage<T: Component>(&mut self) -> std::cell::RefMut<'_, Storage<T>> {
        let type_name = type_name::<T>();
        let cell = self
            .map
            .entry(type_name)
            .or_insert_with(|| RefCell::new(Box::new(OpaqueStorage(Storage::<T>::new()))));
        std::cell::RefMut::map(cell.borrow_mut(), |boxed| unsafe {
            &mut *(boxed.as_mut_ptr() as *mut Storage<T>)
        })
    }

    pub fn for_each_reflected_component_mut(
        &self,
        entity: Entity,
        tick: ChangeTick,
        mut visit: impl FnMut(&'static str, &mut dyn PartialReflect),
    ) {
        let mut names = self.map.keys().copied().collect::<Vec<_>>();
        names.sort_unstable();
        for name in names {
            let mut storage = self.map[name].borrow_mut();
            if let Some(value) = storage.reflected_component_mut(entity, tick) {
                visit(name, value);
            }
        }
    }
}
