//! Minimal Thread execution, model/tool contracts, immutable context and optional cold storage.
//! Product configuration, provider protocols and concrete tools are owned by embedding crates.
pub mod context;
mod error_record;
pub mod model;
#[cfg(feature = "sqlite")]
pub mod persistence;
pub mod storage;
pub mod thread;
mod time;
pub mod tool;
