//! Project Skills catalog 子系统：核心类型、只读查询与显式发现流程的目录页。

mod discovery;
mod query;
mod remote;
mod system;
mod types;

pub(super) use discovery::skills_fingerprint;
pub use types::{SkillCatalogRuntime, SkillSearchResult, SkillsStateData, SkillsStateSnapshot};
