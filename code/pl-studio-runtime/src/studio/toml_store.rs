//! Shared atomic write boundary for the three revisioned Studio TOML documents.

use std::path::Path;
use std::sync::{Mutex, PoisonError};

use anyhow::{Context, Result};
use serde::Serialize;

pub(super) trait RevisionedDocument: Clone + Serialize {
    const KIND: &'static str;

    fn revision_mut(&mut self) -> &mut u64;
}

pub(super) fn persist_if_absent<D: RevisionedDocument>(
    inner: &Mutex<D>,
    path: &Path,
) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let document = inner.lock().unwrap_or_else(PoisonError::into_inner);
    let contents = toml::to_string_pretty(&*document)
        .with_context(|| format!("failed to serialize {}", D::KIND))?;
    pl_tool::workspace::write_file_atomically(path, contents.as_bytes()).map_err(Into::into)
}

pub(super) fn mutate_document<D: RevisionedDocument>(
    inner: &Mutex<D>,
    path: &Path,
    change: impl FnOnce(&mut D) -> bool,
) -> Result<()> {
    let mut document = inner.lock().unwrap_or_else(PoisonError::into_inner);
    let previous = document.clone();
    if !change(&mut document) {
        return Ok(());
    }
    let revision = document.revision_mut();
    *revision = revision.saturating_add(1);
    let result = (|| {
        let contents = toml::to_string_pretty(&*document)
            .with_context(|| format!("failed to serialize {}", D::KIND))?;
        pl_tool::workspace::write_file_atomically(path, contents.as_bytes()).map_err(Into::into)
    })();
    if result.is_err() {
        *document = previous;
    }
    result
}
