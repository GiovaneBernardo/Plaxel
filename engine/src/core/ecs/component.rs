pub trait Component: 'static + Send + Sync {}

impl<T: 'static + Send + Sync> Component for T {}
