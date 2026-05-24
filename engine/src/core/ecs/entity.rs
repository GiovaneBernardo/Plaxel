use std::num::NonZeroU32;

#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
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

pub struct Entities {
    slots: Vec<Slot>,
    free_head: Option<u32>,
    alive_count: u32,
}

impl Entities {
    pub fn new() -> Self {
        Entities {
            slots: Vec::new(),
            free_head: None,
            alive_count: 0,
        }
    }
    pub fn allocate(&mut self) -> Entity {
        match self.free_head {
            Some(index) => {
                let slot = &mut self.slots[index as usize];
                let next = match slot.state {
                    SlotState::Free { next } => next,
                    SlotState::Alive => unreachable!("free_head pointed to an alive slot"),
                };
                slot.state = SlotState::Alive;
                self.free_head = next;
                self.alive_count += 1;
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
                    state: SlotState::Alive,
                });
                self.alive_count += 1;
                Entity { index, generation }
            }
        }
    }
    // false on stale handle
    pub fn deallocate(&mut self, entity: Entity) -> bool {
        let Some(slot) = self.slots.get_mut(entity.index as usize) else {
            return false;
        };

        if !matches!(slot.state, SlotState::Alive) {
            return false;
        }
        if slot.generation != entity.generation {
            return false;
        }

        slot.state = SlotState::Free {
            next: self.free_head,
        };
        slot.generation =
            NonZeroU32::new(slot.generation.get().checked_add(1).unwrap_or(1)).unwrap();
        self.free_head = Some(entity.index);
        self.alive_count -= 1;

        true
    }

    pub fn contains(&self, entity: Entity) -> bool {
        let Some(slot) = self.slots.get(entity.index as usize) else {
            return false;
        };

        if matches!(slot.state, SlotState::Alive) && slot.generation == entity.generation {
            return true;
        }
        false
    }

    pub fn alive_count(&self) -> u32 {
        self.alive_count
    }

    pub fn iter_alive(&self) -> impl Iterator<Item = Entity> + '_ {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            if matches!(slot.state, SlotState::Alive) {
                Some(Entity {
                    index: index as u32,
                    generation: slot.generation,
                })
            } else {
                None
            }
        })
    }
}
struct Slot {
    generation: NonZeroU32,
    state: SlotState,
}

enum SlotState {
    Alive,
    Free { next: Option<u32> },
}
