use crate::ecs::entity::Entity;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChangeTick(pub u64);

impl ChangeTick {
    pub fn is_newer_than(self, last_run: Self, this_run: Self) -> bool {
        let age = this_run.0.wrapping_sub(self.0);
        let system_age = this_run.0.wrapping_sub(last_run.0);
        age < system_age
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldChangeKind {
    Inserted,
    Removed,
    Despawned,
}

#[derive(Debug, Clone)]
pub struct WorldChange {
    pub tick: ChangeTick,
    pub entity: Entity,
    pub component_type: Option<&'static str>,
    pub kind: WorldChangeKind,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ChangeCursor {
    pub tick: ChangeTick,
}
