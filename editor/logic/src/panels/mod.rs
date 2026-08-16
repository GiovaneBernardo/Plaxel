//! One module per dock panel. Panels own their view state and never keep engine data
//! alive across frames, apart from the caches they explicitly document.

pub mod assets;
pub mod console;
pub mod cpu_sampling;
pub mod fields;
pub mod hierarchy;
pub mod icons;
pub mod inspector;
pub mod profiler;
pub mod reflect;
pub mod render_graph;
pub mod resources;
pub mod viewport;
