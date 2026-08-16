/// Stable engine-facing facade for the reflection implementation used by tools.
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

pub use plaxel_reflect::*;

/// A shared runtime counter that can be surfaced by the editor without
/// exposing the synchronization primitive used to update it.
#[derive(Clone, plaxel_reflect::Reflect)]
#[reflect(opaque)]
pub struct RuntimeCounter(Arc<AtomicU64>);

impl RuntimeCounter {
    pub fn new(value: u64) -> Self {
        Self(Arc::new(AtomicU64::new(value)))
    }

    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    pub fn increment(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add(&self, value: usize) {
        self.0.fetch_add(value as u64, Ordering::Relaxed);
    }
}

impl Default for RuntimeCounter {
    fn default() -> Self {
        Self::new(0)
    }
}
