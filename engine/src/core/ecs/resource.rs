use std::{
    any::{Any, TypeId},
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
};

pub trait Resource: 'static + Send + Sync {}

impl<T: 'static + Send + Sync> Resource for T {}

pub struct Resources {
    map: HashMap<TypeId, RefCell<Box<dyn Any>>>,
}

impl Resources {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn insert<T: Resource>(&mut self, value: T) {
        self.map
            .insert(TypeId::of::<T>(), RefCell::new(Box::new(value)));
    }

    pub fn get<T: Resource>(&self) -> Option<Ref<'_, T>> {
        self.map.get(&TypeId::of::<T>()).map(|cell| {
            Ref::map(cell.borrow(), |boxed| {
                boxed.downcast_ref::<T>().expect("TypeId mismatch")
            })
        })
    }

    pub fn get_mut<T: Resource>(&self) -> Option<RefMut<'_, T>> {
        self.map.get(&TypeId::of::<T>()).map(|cell| {
            RefMut::map(cell.borrow_mut(), |boxed| {
                boxed.downcast_mut::<T>().expect("TypeId mismatch")
            })
        })
    }
}
