mod completion;
mod conflict_types;
mod coordinator;
mod design;
mod git;
mod merge;
mod owned_path;
mod recovery;
pub(crate) mod review;
mod spawn;
mod types;
mod work_completion;

pub(crate) use conflict_types::*;
pub(crate) use coordinator::*;
pub(crate) use spawn::{
    StudioSpawnIntent, StudioTaskSpawnPreparation, StudioTaskSpawnRequest, owned_paths_overlap,
};
pub(crate) use types::*;

#[cfg(test)]
mod tests;
