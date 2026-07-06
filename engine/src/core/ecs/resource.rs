use std::{
    any::type_name,
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
};

pub trait Resource: 'static + Send + Sync {}

impl<T: 'static + Send + Sync> Resource for T {}

trait ErasedResource {
    fn as_ptr(&self) -> *const ();
    fn as_mut_ptr(&mut self) -> *mut ();
}

impl<T: Resource> ErasedResource for T {
    fn as_ptr(&self) -> *const () {
        self as *const T as *const ()
    }

    fn as_mut_ptr(&mut self) -> *mut () {
        self as *mut T as *mut ()
    }
}

pub struct Resources {
    map: HashMap<&'static str, RefCell<Box<dyn ErasedResource>>>,
}

impl Resources {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn insert<T: Resource>(&mut self, value: T) {
        self.map
            .insert(type_name::<T>(), RefCell::new(Box::new(value)));
    }

    pub fn get<T: Resource>(&self) -> Option<Ref<'_, T>> {
        self.map.get(type_name::<T>()).map(|cell| {
            Ref::map(cell.borrow(), |boxed| unsafe {
                // Hot-reloaded DLL generations get different TypeIds for the same
                // source type. The stable key above is the guard; changing layout
                // still requires restarting the process.
                &*(boxed.as_ptr() as *const T)
            })
        })
    }

    pub fn get_mut<T: Resource>(&self) -> Option<RefMut<'_, T>> {
        self.map.get(type_name::<T>()).map(|cell| {
            RefMut::map(cell.borrow_mut(), |boxed| unsafe {
                // See `get` for the hot-reload TypeId rationale.
                &mut *(boxed.as_mut_ptr() as *mut T)
            })
        })
    }
}
