pub struct Time {
    pub delta_seconds: f32,
    pub elapsed_seconds: f64,
    pub frame: u64,
    pub delta_time: f32,
    pub fixed_delta_time: f32,
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
