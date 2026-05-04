pub trait Component: 'static + Send + Sync {}

impl<T: 'static + Send + Sync> Component for T {}

pub struct Position {
    x: f32,
    y: f32,
    z: f32,
}
