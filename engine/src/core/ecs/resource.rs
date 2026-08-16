use std::{
    any::type_name,
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use crate::reflect::{PartialReflect, Reflect};

pub trait Resource: 'static + Send + Sync {}

impl<T: 'static + Send + Sync> Resource for T {}

pub struct Res<'w, T: Resource> {
    value: Ref<'w, T>,
}

impl<'w, T: Resource> Res<'w, T> {
    pub(crate) fn new(value: Ref<'w, T>) -> Self {
        Self { value }
    }

    pub fn into_inner(self) -> Ref<'w, T> {
        self.value
    }
}

impl<T: Resource> Deref for Res<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

pub struct ResMut<'w, T: Resource> {
    value: RefMut<'w, T>,
}

impl<'w, T: Resource> ResMut<'w, T> {
    pub(crate) fn new(value: RefMut<'w, T>) -> Self {
        Self { value }
    }

    pub fn into_inner(self) -> RefMut<'w, T> {
        self.value
    }
}

impl<T: Resource> Deref for ResMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T: Resource> DerefMut for ResMut<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

trait ErasedResource {
    fn as_ptr(&self) -> *const ();
    fn as_mut_ptr(&mut self) -> *mut ();
    fn reflect_mut(&mut self) -> Option<&mut dyn PartialReflect>;
}

struct ReflectedResource<T>(T);

impl<T: Resource + Reflect> ErasedResource for ReflectedResource<T> {
    fn as_ptr(&self) -> *const () {
        &self.0 as *const T as *const ()
    }

    fn as_mut_ptr(&mut self) -> *mut () {
        &mut self.0 as *mut T as *mut ()
    }

    fn reflect_mut(&mut self) -> Option<&mut dyn PartialReflect> {
        Some(&mut self.0)
    }
}

struct OpaqueResource<T>(T);

impl<T: Resource> ErasedResource for OpaqueResource<T> {
    fn as_ptr(&self) -> *const () {
        &self.0 as *const T as *const ()
    }

    fn as_mut_ptr(&mut self) -> *mut () {
        &mut self.0 as *mut T as *mut ()
    }

    fn reflect_mut(&mut self) -> Option<&mut dyn PartialReflect> {
        None
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

    pub fn insert<T: Resource + Reflect>(&mut self, value: T) {
        self.map.insert(
            type_name::<T>(),
            RefCell::new(Box::new(ReflectedResource(value))),
        );
    }

    pub fn insert_opaque<T: Resource>(&mut self, value: T) {
        self.map.insert(
            type_name::<T>(),
            RefCell::new(Box::new(OpaqueResource(value))),
        );
    }

    pub fn contains<T: Resource>(&self) -> bool {
        self.map.contains_key(type_name::<T>())
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

    pub fn for_each_reflected_mut(
        &self,
        mut visit: impl FnMut(&'static str, &mut dyn PartialReflect),
    ) {
        self.for_each_mut(|name, value| {
            if let Some(value) = value {
                visit(name, value);
            }
        });
    }

    pub fn for_each_mut(
        &self,
        mut visit: impl FnMut(&'static str, Option<&mut dyn PartialReflect>),
    ) {
        let mut names = self.map.keys().copied().collect::<Vec<_>>();
        names.sort_unstable();
        for name in names {
            let mut resource = self.map[name].borrow_mut();
            visit(name, resource.reflect_mut());
        }
    }
}
