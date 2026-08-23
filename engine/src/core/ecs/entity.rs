use std::{num::NonZeroU32, sync::Arc};

use parking_lot::Mutex;

#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, plaxel_reflect::Reflect)]
pub struct Entity {
    index: u32,
    generation: NonZeroU32,
}

impl Entity {
    pub const PLACEHOLDER: Entity = Entity {
        index: u32::MAX,
        generation: NonZeroU32::new(u32::MAX).unwrap(),
    };

    pub fn index(self) -> u32 {
        self.index
    }

    pub fn generation(self) -> u32 {
        self.generation.get()
    }
}

/// A cloneable handle used by deferred commands to reserve stable entity IDs.
/// Reserved entities do not become visible to the world until they are activated.
#[derive(Clone)]
pub(crate) struct EntityAllocator {
    inner: Arc<Mutex<EntityAllocatorInner>>,
}

pub struct Entities {
    allocator: EntityAllocator,
}

struct EntityAllocatorInner {
    slots: Vec<Slot>,
    free_head: Option<u32>,
    alive_count: u32,
}

impl Entities {
    pub fn new() -> Self {
        Self {
            allocator: EntityAllocator {
                inner: Arc::new(Mutex::new(EntityAllocatorInner {
                    slots: Vec::new(),
                    free_head: None,
                    alive_count: 0,
                })),
            },
        }
    }

    pub fn allocate(&mut self) -> Entity {
        self.allocator.allocate()
    }

    pub(crate) fn allocator(&self) -> EntityAllocator {
        self.allocator.clone()
    }

    pub(crate) fn activate_reserved(&mut self, entity: Entity) -> bool {
        self.allocator.activate_reserved(entity)
    }

    // false on stale handle
    pub fn deallocate(&mut self, entity: Entity) -> bool {
        self.allocator.deallocate(entity)
    }

    pub fn contains(&self, entity: Entity) -> bool {
        self.allocator.contains(entity)
    }

    pub fn alive_count(&self) -> u32 {
        self.allocator.inner.lock().alive_count
    }

    pub fn iter_alive(&self) -> impl Iterator<Item = Entity> + '_ {
        let entities: Vec<_> = self
            .allocator
            .inner
            .lock()
            .slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                matches!(slot.state, SlotState::Alive).then_some(Entity {
                    index: index as u32,
                    generation: slot.generation,
                })
            })
            .collect();
        entities.into_iter()
    }
}

impl Default for Entities {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityAllocator {
    pub(crate) fn reserve(&self) -> Entity {
        self.inner.lock().reserve()
    }

    fn allocate(&self) -> Entity {
        let mut inner = self.inner.lock();
        let entity = inner.reserve();
        let activated = inner.activate_reserved(entity);
        debug_assert!(activated, "a newly reserved entity must activate");
        entity
    }

    fn activate_reserved(&self, entity: Entity) -> bool {
        self.inner.lock().activate_reserved(entity)
    }

    fn deallocate(&self, entity: Entity) -> bool {
        let mut inner = self.inner.lock();
        let free_head = inner.free_head;
        let Some(slot) = inner.slots.get_mut(entity.index as usize) else {
            return false;
        };

        if !matches!(slot.state, SlotState::Alive) || slot.generation != entity.generation {
            return false;
        }

        slot.state = SlotState::Free { next: free_head };
        slot.generation =
            NonZeroU32::new(slot.generation.get().checked_add(1).unwrap_or(1)).unwrap();
        inner.free_head = Some(entity.index);
        inner.alive_count -= 1;
        true
    }

    fn contains(&self, entity: Entity) -> bool {
        let inner = self.inner.lock();
        let Some(slot) = inner.slots.get(entity.index as usize) else {
            return false;
        };
        matches!(slot.state, SlotState::Alive) && slot.generation == entity.generation
    }
}

impl EntityAllocatorInner {
    fn reserve(&mut self) -> Entity {
        match self.free_head {
            Some(index) => {
                let slot = &mut self.slots[index as usize];
                let next = match slot.state {
                    SlotState::Free { next } => next,
                    SlotState::Reserved | SlotState::Alive => {
                        unreachable!("free_head pointed to a non-free slot")
                    }
                };
                slot.state = SlotState::Reserved;
                self.free_head = next;
                Entity {
                    index,
                    generation: slot.generation,
                }
            }
            None => {
                let index: u32 = self
                    .slots
                    .len()
                    .try_into()
                    .expect("entity slot count exceeded u32::MAX");
                let generation = NonZeroU32::new(1).expect("1 is non-zero");
                self.slots.push(Slot {
                    generation,
                    state: SlotState::Reserved,
                });
                Entity { index, generation }
            }
        }
    }

    fn activate_reserved(&mut self, entity: Entity) -> bool {
        let Some(slot) = self.slots.get_mut(entity.index as usize) else {
            return false;
        };

        if !matches!(slot.state, SlotState::Reserved) || slot.generation != entity.generation {
            return false;
        }

        slot.state = SlotState::Alive;
        self.alive_count += 1;
        true
    }
}

struct Slot {
    generation: NonZeroU32,
    state: SlotState,
}

enum SlotState {
    Reserved,
    Alive,
    Free { next: Option<u32> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_entities_are_hidden_until_activated() {
        let mut entities = Entities::new();
        let reserved = entities.allocator().reserve();

        assert!(!entities.contains(reserved));
        assert_eq!(entities.alive_count(), 0);
        assert!(entities.activate_reserved(reserved));
        assert!(entities.contains(reserved));
        assert_eq!(entities.alive_count(), 1);
    }

    #[test]
    fn reservation_reuses_free_slots_with_the_new_generation() {
        let mut entities = Entities::new();
        let original = entities.allocate();
        assert!(entities.deallocate(original));

        let reserved = entities.allocator().reserve();
        assert_eq!(reserved.index(), original.index());
        assert_ne!(reserved.generation(), original.generation());
        assert!(entities.activate_reserved(reserved));
    }
}
