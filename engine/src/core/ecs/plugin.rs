use crate::App;

pub trait Plugin: Send + Sync + 'static {
    fn build(&self, app: &mut App);
}
