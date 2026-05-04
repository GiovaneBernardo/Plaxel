use crate::ecs::{schedule::Schedule, world::World};

pub struct Scene {
    world: World,
    init_schedule: Schedule,
    load_schedule: Schedule,
    update_schedule: Schedule,
    fixed_update_schedule: Schedule,
    late_update_schedule: Schedule,
    editor_update_schedule: Schedule,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            world: World::new(),
            init_schedule: Schedule::new(),
            load_schedule: Schedule::new(),
            update_schedule: Schedule::new(),
            fixed_update_schedule: Schedule::new(),
            late_update_schedule: Schedule::new(),
            editor_update_schedule: Schedule::new(),
        }
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    pub fn update_schedule_mut(&mut self) -> &mut Schedule {
        &mut self.update_schedule
    }

    pub fn init(&mut self) {
        self.init_schedule.run(&mut self.world);
    }

    pub fn load(&mut self) {
        self.load_schedule.run(&mut self.world);
    }

    pub fn update(&mut self) {
        self.update_schedule.run(&mut self.world);
    }

    pub fn fixed_update(&mut self) {
        self.fixed_update_schedule.run(&mut self.world);
    }

    pub fn editor_update(&mut self) {
        self.editor_update_schedule.run(&mut self.world);
    }

    pub fn late_update(&mut self) {
        self.late_update_schedule.run(&mut self.world);
    }
}
