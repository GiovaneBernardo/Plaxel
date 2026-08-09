#[derive(Clone, Copy, PartialEq, Eq, Debug, plaxel_reflect::Reflect)]
pub enum GameMode {
    Walking,
    PilotingShip,
    Menu,
    Editor,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, plaxel_reflect::Reflect)]
pub struct GameModeState {
    pub mode: GameMode,
}
