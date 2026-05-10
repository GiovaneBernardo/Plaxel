use std::collections::HashSet;

pub type KeyCode = winit::keyboard::KeyCode;

pub struct InputState {
    pub pressed: HashSet<KeyCode>,
    pub just_pressed: HashSet<KeyCode>,
    pub just_released: HashSet<KeyCode>,
    pub mouse_delta: (f32, f32),
    pub scroll: f32,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            pressed: HashSet::new(),
            just_pressed: HashSet::new(),
            just_released: HashSet::new(),
            mouse_delta: (0.0, 0.0),
            scroll: 0.0,
        }
    }
}
