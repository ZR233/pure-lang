//! Studio-owned durable timeline projection index and bounded cold readers.
//!
//! The canonical Thread journal, owned by `pl-core`, stays the single source of history. This
//! module is the Studio-derived, fully rebuildable display index that lives **inside the same
//! per-Thread session database** (`sessions/<thread-id>.sqlite`) as `session_entries`; it never
//! creates a second history database and never reinterprets the core journal format.
//!
//! The durable boundary mirrors [`crate::studio::thread_projection`]:
//!
//! - A small `timeline_head` row holds only the watermark, next ordinal and rebuild generation.
//! - [`write::TimelineWriter`] consumes one already-applied [`ProjectionDelta`] together with the
//!   owning [`ProjectionState`], and persists the dirty fact rows, changed slots, panel summary and
//!   head **in one transaction** whose watermark is validated against a core commit that is already
//!   durable. It never replays a journal prefix and never trusts a caller-supplied watermark.
//! - [`read::TimelineReader`] opens the session database **read-only** (never creating it, never
//!   rebuilding it, never loading the core journal), then serves keyset timeline pages, bounded
//!   content reads and requirement-driven fact loads.
//!
//! Oversized display items are split at write time into a bounded preview plus a versioned content
//! reference backed by fixed-size chunk rows, so a page never decodes a whole large item and
//! [`read::TimelineReader::read_content`] fetches only the chunks an offset window needs.
//!
//! This module is intentionally staged: it depends on the projector's `requirements`/`delta`
//! contract and does not itself own connection routing, the write-behind worker or the protocol
//! wire. Loading requirement rows back into a *live* [`ProjectionState`] still needs a projector
//! hook (see `read::TimelineReader::load_requirements`); the cold activation path uses
//! [`read::TimelineReader::restore_source`] with the projector's own `restore`.

mod build;
mod content;
mod facts;
mod page;
mod read;
mod schema;
mod turns;
mod write;

#[cfg(test)]
mod fixture;

pub(crate) use build::{
    IndexBuildOutcome, IndexVerification, build_index, rebuild_index, verify_index,
};
pub(crate) use content::{ContentChunk, ContentRef};
pub(crate) use facts::{LoadedFacts, RestoreSource, TimelineWorkingSet};
pub(crate) use page::{
    TimelineBudget, TimelineCursor, TimelineEntry, TimelinePageQuery, TimelinePreview,
    TimelineTurnMeta, TimelineWindow,
};
pub(crate) use read::TimelineReader;
pub(crate) use schema::TIMELINE_SCHEMA_VERSION;
pub(crate) use turns::{TimelineTurnQuery, TurnWindow};
pub(crate) use write::TimelineWriter;

use std::path::PathBuf;

/// Default and maximum number of display items one page may return.
pub(crate) const DEFAULT_PAGE_ITEMS: usize = 100;
pub(crate) const MAX_PAGE_ITEMS: usize = 100;
/// Default and maximum payload bytes one page may return.
pub(crate) const DEFAULT_PAGE_BYTES: usize = 256 * 1024;
pub(crate) const MAX_PAGE_BYTES: usize = 256 * 1024;

/// Typed failure for the Studio timeline index; every variant keeps its underlying source.
#[derive(Debug, thiserror::Error)]
pub(crate) enum TimelineStoreError {
    #[error("timeline index database operation failed: {0}")]
    Database(#[from] sea_orm::DbErr),
    #[error("timeline index filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("timeline index payload codec failed: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error("timeline index session database is missing: {path}")]
    MissingDatabase { path: PathBuf },
    #[error("timeline index is not initialised in {path}; missing index is never treated as empty")]
    IndexNotInitialized { path: PathBuf },
    #[error("unsupported timeline index schema {found}; expected {supported}")]
    UnsupportedSchema { found: i64, supported: i64 },
    #[error("Thread {thread_id} has no durable timeline index")]
    ThreadNotIndexed { thread_id: String },
    #[error("invalid timeline request: {0}")]
    InvalidRequest(String),
    #[error("invalid timeline cursor: {0}")]
    InvalidCursor(String),
    #[error("stale timeline cursor: {0}")]
    StaleCursor(String),
    #[error("timeline item {item_id} does not belong to this Thread at the requested watermark")]
    UnknownItem { item_id: String },
    #[error("timeline Turn {turn_id} is unknown for this Thread")]
    UnknownTurn { turn_id: String },
    #[error("timeline content reference {ref_id} is unknown")]
    UnknownContentRef { ref_id: String },
    #[error("timeline content offset {offset} is outside the item's {total} bytes")]
    InvalidOffset { offset: u64, total: u64 },
    #[error("timeline content offset {offset} does not fall on a UTF-8 character boundary")]
    NotUtf8Boundary { offset: u64 },
    #[error("Thread {thread_id} commit {sequence} is not durable in the core journal")]
    WatermarkNotDurable { thread_id: String, sequence: u64 },
    #[error("timeline index watermark is out of sequence: expected {expected}, found {found}")]
    SequenceGap { expected: u64, found: u64 },
    #[error("timeline index watermark {index} leads the durable journal end {journal}")]
    IndexAhead { index: u64, journal: u64 },
    #[error("timeline index watermark {found} does not match the durable journal end {expected}")]
    IndexMismatch { expected: u64, found: u64 },
    #[error("Thread journal is not contiguous: expected commit {expected}, found {found}")]
    JournalGap { expected: u64, found: u64 },
    #[error("saved projection facts were rejected: {0}")]
    Projection(#[from] crate::studio::thread_projection::ProjectionError),
    #[error("timeline index row is corrupt: {0}")]
    Corrupt(String),
}

impl TimelineStoreError {
    /// True when the failure is a corrupted or incompatible index rather than a transient IO error.
    pub(crate) fn is_index_fault(&self) -> bool {
        matches!(
            self,
            Self::MissingDatabase { .. }
                | Self::IndexNotInitialized { .. }
                | Self::UnsupportedSchema { .. }
                | Self::Corrupt(_)
        )
    }
}
