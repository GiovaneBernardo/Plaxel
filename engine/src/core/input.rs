use std::collections::HashSet;

pub type KeyCode = winit::keyboard::KeyCode;
pub type MouseButton = winit::event::MouseButton;

#[derive(plaxel_reflect::Reflect)]
#[reflect(from_reflect = false)]
pub struct InputState {
    #[reflect(ignore)]
    pub pressed: HashSet<KeyCode>,
    #[reflect(ignore)]
    pub just_pressed: HashSet<KeyCode>,
    #[reflect(ignore)]
    pub just_released: HashSet<KeyCode>,
    #[reflect(ignore)]
    pub mouse_pressed: HashSet<MouseButton>,
    #[reflect(ignore)]
    pub mouse_just_pressed: HashSet<MouseButton>,
    #[reflect(ignore)]
    pub mouse_just_released: HashSet<MouseButton>,
    pub mouse_position: Option<(f32, f32)>,
    pub mouse_delta: (f32, f32),
    pub scroll: f32,

    // Mouse UI
    pub is_mouse_over_game_view: bool,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            pressed: HashSet::new(),
            just_pressed: HashSet::new(),
            just_released: HashSet::new(),
            mouse_just_pressed: HashSet::new(),
            mouse_just_released: HashSet::new(),
            mouse_pressed: HashSet::new(),
            mouse_position: None,
            mouse_delta: (0.0, 0.0),
            scroll: 0.0,
            is_mouse_over_game_view: true,
        }
    }
}
