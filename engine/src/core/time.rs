use web_time::Instant;

use crate::ecs::{plugin::Plugin, resource::ResMut, schedule::CoreSchedule, system::Local};

#[derive(plaxel_reflect::Reflect)]
pub struct Time {
    pub delta_seconds: f32,
    pub elapsed_seconds: f64,
    pub frame: u64,
    pub delta_time: f32,
    pub fixed_delta_time: f32,
}

pub struct TimePlugin;

impl Plugin for TimePlugin {
    fn build(&self, app: &mut crate::App) {
        app.insert_resource(Time::new())
            .add_system(CoreSchedule::First, update_time);
    }
}

fn update_time(mut time: ResMut<Time>, mut previous: Local<Option<Instant>>) {
    let now = Instant::now();
    let delta = previous
        .as_ref()
        .map(|previous| now.duration_since(*previous).as_secs_f32())
        .unwrap_or(0.0);

    *previous = Some(now);
    time.delta_seconds = delta;
    time.delta_time = delta;
    time.elapsed_seconds += f64::from(delta);
    time.frame = time.frame.wrapping_add(1);
}

impl Time {
    pub fn new() -> Self {
        Self {
            delta_seconds: 0.0,
            elapsed_seconds: 0.0,
            frame: 0,
            delta_time: 0.0,
            fixed_delta_time: 0.0,
        }
    }
}
