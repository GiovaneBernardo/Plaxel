#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameMode {
    Walking,
    PilotingShip,
    Menu,
    Editor,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GameModeState {
    pub mode: GameMode,
}
