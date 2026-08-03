use std::{
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
};

pub struct GpuHandle<T> {
    index: u32,
    generation: u32,
    marker: PhantomData<fn() -> T>,
}

impl<T> GpuHandle<T> {
    pub(crate) const fn new(index: u32, generation: u32) -> Self {
        Self {
            index,
            generation,
            marker: PhantomData,
        }
    }

    pub const fn index(self) -> u32 {
        self.index
    }

    pub const fn generation(self) -> u32 {
        self.generation
    }
}

impl<T> Copy for GpuHandle<T> {}

impl<T> Clone for GpuHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> fmt::Debug for GpuHandle<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GpuHandle")
            .field("index", &self.index)
            .field("generation", &self.generation)
            .finish()
    }
}

impl<T> PartialEq for GpuHandle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<T> Eq for GpuHandle<T> {}

impl<T> Hash for GpuHandle<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        self.generation.hash(state);
    }
}

struct GpuSlot<T> {
    generation: u32,
    value: Option<T>,
}

pub(crate) struct GpuArena<T> {
    slots: Vec<GpuSlot<T>>,
    free: Vec<u32>,
}

impl<T> GpuArena<T> {
    pub(crate) fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }

    pub(crate) fn insert(&mut self, value: T) -> GpuHandle<T> {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            debug_assert!(slot.value.is_none());
            slot.value = Some(value);
            return GpuHandle::new(index, slot.generation);
        }

        let index = u32::try_from(self.slots.len()).expect("GPU arena exhausted its handle space");
        self.slots.push(GpuSlot {
            generation: 1,
            value: Some(value),
        });
        GpuHandle::new(index, 1)
    }

    pub(crate) fn get(&self, handle: GpuHandle<T>) -> Option<&T> {
        let slot = self.slots.get(handle.index as usize)?;
        (slot.generation == handle.generation)
            .then_some(slot.value.as_ref())
            .flatten()
    }

    pub(crate) fn remove(&mut self, handle: GpuHandle<T>) -> Option<T> {
        let slot = self.slots.get_mut(handle.index as usize)?;
        if slot.generation != handle.generation {
            return None;
        }

        let value = slot.value.take()?;
        slot.generation = slot.generation.wrapping_add(1).max(1);
        self.free.push(handle.index);
        Some(value)
    }
}

impl<T> Default for GpuArena<T> {
    fn default() -> Self {
        Self::new()
    }
}
