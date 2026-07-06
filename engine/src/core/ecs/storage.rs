use crate::ecs::component::Component;
use crate::ecs::entity::Entity;
use std::any::type_name;
use std::cell::RefCell;
use std::collections::HashMap;

trait ErasedStorage {
    fn as_ptr(&self) -> *const ();
    fn as_mut_ptr(&mut self) -> *mut ();
    fn remove_entity(&mut self, entity: Entity);
}

impl<T: 'static> ErasedStorage for Storage<T> {
    fn as_ptr(&self) -> *const () {
        self as *const Storage<T> as *const ()
    }

    fn as_mut_ptr(&mut self) -> *mut () {
        self as *mut Storage<T> as *mut ()
    }

    fn remove_entity(&mut self, entity: Entity) {
        self.remove(entity);
    }
}

pub struct Storage<T> {
    sparse: Vec<u32>,
    dense: Vec<T>,
    entities: Vec<Entity>,
}

impl<T> Storage<T> {
    pub fn new() -> Self {
        Self {
            sparse: Vec::new(),
            dense: Vec::new(),
            entities: Vec::new(),
        }
    }

    pub fn insert(&mut self, entity: Entity, component: T) {
        let idx = entity.index() as usize;

        // ensure sparse is large enough
        if self.sparse.len() <= idx {
            self.sparse.resize(idx + 1, u32::MAX);
        }

        let dense_idx = self.sparse[idx];

        // Case 1: entity already has component → overwrite
        if dense_idx != u32::MAX {
            self.dense[dense_idx as usize] = component;
            return;
        }

        // Case 2: new insert
        let new_dense_idx = self.dense.len() as u32;

        self.dense.push(component);
        self.entities.push(entity);
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

    pub fn get_mut(&mut self, entity: Entity) -> Option<&mut T> {
        let idx = entity.index() as usize;

        let dense_idx = *self.sparse.get(idx)?;

        if dense_idx == u32::MAX {
            return None;
        }

        if self.entities.get(dense_idx as usize).copied()? != entity {
            return None;
        }

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

        if dense_idx < self.entities.len() {
            let moved_entity = self.entities[dense_idx];
            self.sparse[moved_entity.index() as usize] = dense_idx as u32;
        }

        Some(removed_component)
    }

    pub fn dense(&self) -> &Vec<T> {
        &self.dense
    }

    pub fn dense_mut(&mut self) -> &mut Vec<T> {
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

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Entity, &mut T)> {
        self.entities.iter().copied().zip(self.dense.iter_mut())
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

    pub fn ensure_storage<T: Component>(&mut self) -> std::cell::RefMut<'_, Storage<T>> {
        let type_name = type_name::<T>();

        let cell = self
            .map
            .entry(type_name)
            .or_insert_with(|| RefCell::new(Box::new(Storage::<T>::new())));

        std::cell::RefMut::map(cell.borrow_mut(), |boxed| unsafe {
            // See `get_storage` for the hot-reload TypeId rationale.
            &mut *(boxed.as_mut_ptr() as *mut Storage<T>)
        })
    }
}
