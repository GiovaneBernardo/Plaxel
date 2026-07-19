use std::collections::HashMap;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct RenderPhaseId(pub u64);
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct MaterialPassId(pub u64);
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct RenderFeatureId(pub u64);
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct GraphPassId(pub u64);
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct RenderProducerId(pub u64);
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct RenderViewId(pub u64);

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

const fn stable_hash(name: &str) -> u64 {
    let bytes = name.as_bytes();
    let mut hash = FNV_OFFSET;
    let mut index = 0;
    while index < bytes.len() {
        hash ^= bytes[index] as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
        index += 1;
    }
    hash
}

macro_rules! impl_id {
    ($type_name:ident) => {
        impl $type_name {
            pub const fn new(name: &str) -> Self {
                Self(stable_hash(name))
            }
        }
    };
}

impl_id!(RenderPhaseId);
impl_id!(MaterialPassId);
impl_id!(RenderFeatureId);
impl_id!(GraphPassId);
impl_id!(RenderProducerId);
impl_id!(RenderViewId);

macro_rules! impl_display {
    ($($type_name:ident),+ $(,)?) => {$(
        impl std::fmt::Display for $type_name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(formatter, "{:016x}", self.0)
            }
        }
    )+};
}
impl_display!(
    RenderPhaseId,
    MaterialPassId,
    RenderFeatureId,
    GraphPassId,
    RenderProducerId,
    RenderViewId
);

pub mod phases {
    use super::RenderPhaseId;

    pub const OPAQUE: RenderPhaseId = RenderPhaseId::new("engine.opaque");
    pub const WATER: RenderPhaseId = RenderPhaseId::new("engine.water");
    pub const TRANSPARENT: RenderPhaseId = RenderPhaseId::new("engine.transparent");
    pub const DEBUG: RenderPhaseId = RenderPhaseId::new("engine.debug");
    pub const PRESENT: RenderPhaseId = RenderPhaseId::new("engine.present");
}

pub mod material_passes {
    use super::MaterialPassId;

    pub const DEPTH_ONLY: MaterialPassId = MaterialPassId::new("engine.depth_only");
    pub const SHADOW: MaterialPassId = MaterialPassId::new("engine.shadow");
    pub const FORWARD_OPAQUE: MaterialPassId = MaterialPassId::new("engine.forward_opaque");
    pub const FORWARD_TRANSPARENT: MaterialPassId =
        MaterialPassId::new("engine.forward_transparent");
    pub const WATER: MaterialPassId = MaterialPassId::new("engine.water");
    pub const DEBUG: MaterialPassId = MaterialPassId::new("engine.debug");
    pub const FULLSCREEN: MaterialPassId = MaterialPassId::new("engine.fullscreen");
}

pub mod graph_passes {
    use super::GraphPassId;
    pub const GEOMETRY: GraphPassId = GraphPassId::new("geometry_pass");
    pub const DEPTH_PREPASS: GraphPassId = GraphPassId::new("depth_prepass");
    pub const SHADOWS: GraphPassId = GraphPassId::new("shadow_cascades");
    pub const WATER: GraphPassId = GraphPassId::new("water");
    pub const ATMOSPHERE: GraphPassId = GraphPassId::new("atmosphere");
    pub const DEBUG: GraphPassId = GraphPassId::new("debug");
    pub const EGUI: GraphPassId = GraphPassId::new("egui");
}

pub mod producers {
    use super::RenderProducerId;
    pub const STANDARD_MESHES: RenderProducerId = RenderProducerId::new("engine.standard_meshes");
}

pub mod views {
    use super::RenderViewId;
    pub const MAIN: RenderViewId = RenderViewId::new("engine.main_view");
}

// Catch hash collisions
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    IdCollision {
        id: u64,
        existing: String,
        new: String,
    },
    DuplicateName(String),
    UnknownDependency(String),
    Cycle,
}

/// Stable public IDs resolve to dense indices once, outside hot rendering loops.
pub struct StableIdRegistry<I> {
    entries: Vec<(I, String)>,
    names_by_raw: HashMap<u64, String>,
}

impl<I: Copy + Into<u64>> StableIdRegistry<I> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            names_by_raw: HashMap::new(),
        }
    }

    pub fn register(&mut self, id: I, name: impl Into<String>) -> Result<u16, RegistryError> {
        let name = name.into();
        let raw = id.into();
        if let Some(existing) = self.names_by_raw.get(&raw) {
            return if existing == &name {
                Err(RegistryError::DuplicateName(name))
            } else {
                Err(RegistryError::IdCollision {
                    id: raw,
                    existing: existing.clone(),
                    new: name,
                })
            };
        }
        let index =
            u16::try_from(self.entries.len()).expect("stable ID registry exceeded u16::MAX");
        self.names_by_raw.insert(raw, name.clone());
        self.entries.push((id, name));
        Ok(index)
    }

    pub fn dense_index(&self, id: I) -> Option<u16> {
        let raw = id.into();
        self.entries
            .iter()
            .position(|(candidate, _)| (*candidate).into() == raw)
            .map(|i| i as u16)
    }
}

impl<I: Copy + Into<u64>> Default for StableIdRegistry<I> {
    fn default() -> Self {
        Self::new()
    }
}

macro_rules! id_into_u64 {
    ($($name:ident),+ $(,)?) => {$(
        impl From<$name> for u64 { fn from(value: $name) -> Self { value.0 } }
    )+};
}
id_into_u64!(
    RenderPhaseId,
    MaterialPassId,
    RenderFeatureId,
    GraphPassId,
    RenderProducerId,
    RenderViewId
);
