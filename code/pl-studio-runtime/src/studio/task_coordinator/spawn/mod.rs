mod lifecycle;
mod metadata;
mod tool;

pub(crate) use lifecycle::{
    StudioTaskSpawnPreparation, StudioTaskSpawnRequest, normalize_scope_hints,
};
pub(crate) use metadata::StudioSpawnIntent;
