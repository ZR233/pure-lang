//! Studio canonical owner for Project workspace declarations.
//!
//! `workspaces/<project-id>.toml` is the sole persistent source for a Project's
//! `id`/`name`/`path`/`ssh_alias`; the `projects` table is a rebuildable search cache and
//! keeps the dynamic `created`/`updated`/`recent`/`closed` state. This owner publishes the
//! canonical in-memory snapshot, coordinates the first legacy export from the product
//! directory, and applies explicit reloads. It never overwrites a user file from the
//! database and never deletes session or user data.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::ProjectRecord;
use crate::config::{WorkspaceConfigStore, WorkspaceDeclaration, WorkspaceDeclarationError};

/// Declaration layout version; a future layout must add a new version.
pub(in crate::studio) const WORKSPACE_LAYOUT_VERSION: u32 = 1;

const WORKSPACE_LAYOUT_MARKER_FILE: &str = "workspace-layout.json";
const WORKSPACE_INTENT_FILE: &str = "workspace-intent.json";

/// Published marker recording which declaration ids the layout covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceLayoutMarker {
    layout_version: u32,
    published_at: i64,
    ids: Vec<String>,
    /// Per-file content fingerprint, so a same-id external edit is detected.
    #[serde(default)]
    entries: BTreeMap<String, String>,
}

/// Durable intent for one in-flight cross-file commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceIntent {
    txn: String,
    id: String,
    previous: Option<String>,
    target: Option<String>,
}

/// Studio-side failure mapping the declaration file layer plus publish/coordination errors.
#[derive(Debug, thiserror::Error)]
pub(in crate::studio) enum WorkspaceStoreError {
    #[error(transparent)]
    Declaration(#[from] WorkspaceDeclarationError),
    #[error(
        "workspace declaration for {id} conflicts with the product directory and was preserved"
    )]
    Conflict { id: String },
    #[error("published workspace declaration layout disagrees with its files: {0}")]
    LayoutMismatch(String),
    #[error("workspace declaration layout marker is corrupt and was preserved: {0}")]
    Marker(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Canonical Project declaration owner: file layer plus published in-memory snapshot.
pub(in crate::studio) struct WorkspaceDeclarations {
    store: WorkspaceConfigStore,
    marker: PathBuf,
    snapshot: Mutex<BTreeMap<String, WorkspaceDeclaration>>,
}

impl WorkspaceDeclarations {
    pub(in crate::studio) fn new(store: WorkspaceConfigStore, marker: PathBuf) -> Self {
        Self {
            store,
            marker,
            snapshot: Mutex::new(BTreeMap::new()),
        }
    }

    /// Startup coordination under the exclusive runtime lock.
    ///
    /// If the layout marker is present the declarations are loaded and must match the
    /// published id set. Otherwise the product directory is exported once (all Projects,
    /// including closed rows, keeping original ids) and the marker is published; a fresh
    /// install publishes an empty, consistent layout. Existing user files are never
    /// overwritten: a mismatch is a typed conflict that preserves the file.
    pub(in crate::studio) fn initialize(
        &self,
        directory: &[ProjectRecord],
    ) -> Result<(), WorkspaceStoreError> {
        let existing_marker = if self.marker.exists() {
            Some(self.read_marker()?)
        } else {
            None
        };
        // A pending cross-file intent is the only thing that may complete a commit; it is
        // resolved (or fails) before any other publication.
        if let Some(intent) = self.read_intent()? {
            return self.complete_intent(existing_marker, &intent);
        }
        let declarations = self.load()?;
        match &existing_marker {
            Some(marker) => {
                let mut expected = marker.ids.clone();
                expected.sort();
                let mut actual: Vec<String> = declarations.keys().cloned().collect();
                actual.sort();
                if expected != actual {
                    return Err(WorkspaceStoreError::LayoutMismatch(format!(
                        "published declaration ids {expected:?} do not match files {actual:?}"
                    )));
                }
                // A published marker binds each file's content; a same-id edit is rejected
                // until an explicit reload.
                for (id, fingerprint) in &marker.entries {
                    if self.declaration_fingerprint(id)?.as_ref() != Some(fingerprint) {
                        return Err(WorkspaceStoreError::LayoutMismatch(format!(
                            "declaration {id} content changed since publication; reload to apply"
                        )));
                    }
                }
            }
            None => {
                // First publication: export every product directory row (including closed
                // rows, keeping original ids). An extra, non-exported file is rejected.
                for id in declarations.keys() {
                    if !directory.iter().any(|project| &project.id == id) {
                        return Err(WorkspaceStoreError::LayoutMismatch(format!(
                            "workspace declaration {id} has no owning Project; preserved"
                        )));
                    }
                }
                for project in directory {
                    let declaration = WorkspaceDeclaration::new(
                        project.id.clone(),
                        project.name.clone(),
                        project.path.clone(),
                        project.ssh_alias.clone(),
                    );
                    match declarations.get(&project.id) {
                        Some(existing) if existing != &declaration => {
                            return Err(WorkspaceStoreError::Conflict {
                                id: project.id.clone(),
                            });
                        }
                        Some(_) => {}
                        None => {
                            self.store.save(&declaration)?;
                        }
                    }
                }
            }
        }
        let declarations = self.load()?;
        if !self.marker_covers(&existing_marker, &declarations)? {
            self.write_marker_for(&declarations)?;
        }
        *self.lock() = declarations;
        Ok(())
    }

    /// Loads a validated candidate set without changing the published snapshot.
    ///
    /// Every published marker id must still resolve to a file; extra valid files are
    /// accepted (they become adopted on commit).
    pub(in crate::studio) fn load_candidate(
        &self,
    ) -> Result<BTreeMap<String, WorkspaceDeclaration>, WorkspaceStoreError> {
        let marker = self.read_marker()?;
        let loaded = self.load()?;
        for id in &marker.ids {
            if !loaded.contains_key(id) {
                return Err(WorkspaceStoreError::LayoutMismatch(format!(
                    "published workspace declaration {id} is missing"
                )));
            }
        }
        Ok(loaded)
    }

    /// Publishes a candidate set: the marker is written first, then the memory snapshot, so
    /// a failure between the two never leaves a half-published state.
    pub(in crate::studio) fn commit_snapshot(
        &self,
        candidate: BTreeMap<String, WorkspaceDeclaration>,
    ) -> Result<(), WorkspaceStoreError> {
        self.write_marker_for(&candidate)?;
        *self.lock() = candidate;
        self.clear_intent()?;
        Ok(())
    }

    /// Saves one canonical declaration before any directory mutation is published.
    ///
    /// Order is: durable file, published marker (the gate), then memory snapshot. A failure
    /// after the file write leaves it unpublished, and the next startup adopts it, so the
    /// cross-file commit is recoverable and never half-published.
    pub(in crate::studio) fn declare(
        &self,
        declaration: &WorkspaceDeclaration,
    ) -> Result<(), WorkspaceStoreError> {
        // Durable intent first, then the declaration file, then the content-bound marker,
        // then the memory snapshot. A crash is completed (or fails preserved) at next start.
        let previous = self.declaration_fingerprint(&declaration.id)?;
        self.write_intent(&WorkspaceIntent {
            txn: format!("{}-{}", crate::studio::new_id("ws"), declaration.id),
            id: declaration.id.clone(),
            previous,
            target: None,
        })?;
        if let Err(error) = self.store.save(declaration) {
            // A rejected write never replaced the file, so the intent is cleared (no half state).
            let _ = self.clear_intent();
            return Err(error.into());
        }
        let target = self.declaration_fingerprint(&declaration.id)?;
        self.write_intent(&WorkspaceIntent {
            txn: format!("{}-{}", crate::studio::new_id("ws"), declaration.id),
            id: declaration.id.clone(),
            previous: None,
            target,
        })?;
        let mut candidate = self.snapshot();
        candidate.insert(declaration.id.clone(), declaration.clone());
        self.write_marker_for(&candidate)?;
        *self.lock() = candidate;
        self.clear_intent()?;
        Ok(())
    }

    /// The published canonical declaration set.
    pub(in crate::studio) fn snapshot(&self) -> BTreeMap<String, WorkspaceDeclaration> {
        self.lock().clone()
    }

    /// Declaration file path for one Project id (test helper for corruption/failure cases).
    #[cfg(test)]
    pub(in crate::studio) fn declaration_path(
        &self,
        id: &str,
    ) -> Result<PathBuf, WorkspaceStoreError> {
        Ok(self.store.declaration_path(id)?)
    }

    /// The published declaration matching a workspace address, distinguishing local and SSH.
    pub(in crate::studio) fn declaration_for_path(
        &self,
        path: &str,
        ssh_alias: Option<&str>,
    ) -> Option<WorkspaceDeclaration> {
        self.lock()
            .values()
            .find(|declaration| {
                declaration.path == path && declaration.ssh_alias.as_deref() == ssh_alias
            })
            .cloned()
    }

    fn load(&self) -> Result<BTreeMap<String, WorkspaceDeclaration>, WorkspaceStoreError> {
        let mut declarations = BTreeMap::new();
        for declaration in self.store.load_all()? {
            declarations.insert(declaration.id.clone(), declaration);
        }
        Ok(declarations)
    }

    /// Whether the existing marker already publishes exactly this id set and content.
    fn marker_covers(
        &self,
        marker: &Option<WorkspaceLayoutMarker>,
        declarations: &BTreeMap<String, WorkspaceDeclaration>,
    ) -> Result<bool, WorkspaceStoreError> {
        let Some(marker) = marker else {
            return Ok(false);
        };
        if self.entries_for(declarations)? != marker.entries {
            return Ok(false);
        }
        let mut ids: Vec<String> = declarations.keys().cloned().collect();
        ids.sort();
        let mut marker_ids = marker.ids.clone();
        marker_ids.sort();
        Ok(ids == marker_ids)
    }

    fn entries_for(
        &self,
        declarations: &BTreeMap<String, WorkspaceDeclaration>,
    ) -> Result<BTreeMap<String, String>, WorkspaceStoreError> {
        let mut entries = BTreeMap::new();
        for id in declarations.keys() {
            if let Some(fingerprint) = self.declaration_fingerprint(id)? {
                entries.insert(id.clone(), fingerprint);
            }
        }
        Ok(entries)
    }

    /// Content fingerprint of one declaration file, or `None` when it does not exist.
    fn declaration_fingerprint(&self, id: &str) -> Result<Option<String>, WorkspaceStoreError> {
        let path = self.store.declaration_path(id)?;
        match std::fs::read(&path) {
            Ok(bytes) => Ok(Some(pl_core::context::content_hash(&bytes))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(WorkspaceStoreError::Io(error)),
        }
    }

    /// Completes a pending intent: only a file matching the recorded target may be published;
    /// a mismatched file or an unusable intent keeps the on-disk state and fails preserved.
    fn complete_intent(
        &self,
        marker: Option<WorkspaceLayoutMarker>,
        intent: &WorkspaceIntent,
    ) -> Result<(), WorkspaceStoreError> {
        let actual = self.declaration_fingerprint(&intent.id)?;
        match (&intent.target, actual) {
            // The declaration file was never written: the transaction is a no-op.
            (_, None) => {
                self.clear_intent()?;
                return self.initialize_without_intent(marker);
            }
            (Some(target), Some(actual)) if &actual == target => {}
            _ => {
                return Err(WorkspaceStoreError::LayoutMismatch(format!(
                    "pending declaration intent for {} cannot be completed safely; state preserved",
                    intent.id
                )));
            }
        }
        let mut declarations = self.load()?;
        if self.marker_covers(&marker, &declarations)? {
            self.clear_intent()?;
            *self.lock() = declarations;
            return Ok(());
        }
        self.write_marker_for(&declarations)?;
        self.clear_intent()?;
        *self.lock() = declarations;
        Ok(())
    }

    fn initialize_without_intent(
        &self,
        marker: Option<WorkspaceLayoutMarker>,
    ) -> Result<(), WorkspaceStoreError> {
        let declarations = self.load()?;
        if let Some(marker) = &marker {
            let mut expected = marker.ids.clone();
            expected.sort();
            let mut actual: Vec<String> = declarations.keys().cloned().collect();
            actual.sort();
            if expected != actual {
                return Err(WorkspaceStoreError::LayoutMismatch(format!(
                    "published declaration ids {expected:?} do not match files {actual:?}"
                )));
            }
        }
        *self.lock() = declarations;
        Ok(())
    }

    fn intent_path(&self) -> PathBuf {
        self.marker.with_file_name(WORKSPACE_INTENT_FILE)
    }

    fn read_intent(&self) -> Result<Option<WorkspaceIntent>, WorkspaceStoreError> {
        let path = self.intent_path();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(WorkspaceStoreError::Io(error)),
        };
        let intent: WorkspaceIntent = serde_json::from_slice(&bytes)
            .map_err(|error| WorkspaceStoreError::Marker(format!("{}: {error}", path.display())))?;
        Ok(Some(intent))
    }

    fn write_intent(&self, intent: &WorkspaceIntent) -> Result<(), WorkspaceStoreError> {
        let bytes = serde_json::to_vec(intent).map_err(|error| {
            WorkspaceStoreError::Marker(format!("failed to encode intent: {error}"))
        })?;
        pl_tool::workspace::write_file_atomically(&self.intent_path(), &bytes)
            .map_err(|error| WorkspaceStoreError::Io(std::io::Error::other(error.to_string())))
    }

    fn clear_intent(&self) -> Result<(), WorkspaceStoreError> {
        match std::fs::remove_file(self.intent_path()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(WorkspaceStoreError::Io(error)),
        }
    }

    fn read_marker(&self) -> Result<WorkspaceLayoutMarker, WorkspaceStoreError> {
        let bytes = std::fs::read(&self.marker).map_err(WorkspaceStoreError::Io)?;
        let marker: WorkspaceLayoutMarker = serde_json::from_slice(&bytes).map_err(|error| {
            WorkspaceStoreError::Marker(format!("{}: {error}", self.marker.display()))
        })?;
        if marker.layout_version != WORKSPACE_LAYOUT_VERSION {
            return Err(WorkspaceStoreError::Marker(format!(
                "unsupported workspace layout version {} at {}",
                marker.layout_version,
                self.marker.display()
            )));
        }
        Ok(marker)
    }

    fn write_marker_for(
        &self,
        declarations: &BTreeMap<String, WorkspaceDeclaration>,
    ) -> Result<(), WorkspaceStoreError> {
        let mut ids: Vec<String> = declarations.keys().cloned().collect();
        ids.sort();
        let entries = self.entries_for(declarations)?;
        let marker = WorkspaceLayoutMarker {
            layout_version: WORKSPACE_LAYOUT_VERSION,
            published_at: crate::studio::unix_seconds(),
            ids,
            entries,
        };
        let bytes = serde_json::to_vec(&marker).map_err(|error| {
            WorkspaceStoreError::Marker(format!("failed to encode layout marker: {error}"))
        })?;
        pl_tool::workspace::write_file_atomically(&self.marker, &bytes)
            .map_err(|error| WorkspaceStoreError::Io(std::io::Error::other(error.to_string())))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, WorkspaceDeclaration>> {
        self.snapshot.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Marker path beside the product database.
pub(in crate::studio) fn layout_marker_path(product_database: &Path) -> PathBuf {
    product_database.with_file_name(WORKSPACE_LAYOUT_MARKER_FILE)
}

/// Builds a public Project record by merging a canonical declaration with dynamic state.
pub(in crate::studio) fn project_from_declaration(
    declaration: &WorkspaceDeclaration,
    updated_at: i64,
) -> ProjectRecord {
    ProjectRecord {
        id: declaration.id.clone(),
        name: declaration.name.clone(),
        path: declaration.path.clone(),
        ssh_alias: declaration.ssh_alias.clone(),
        updated_at,
    }
}

/// Diagnostic context for a declaration failure, kept for `anyhow` boundaries.
pub(in crate::studio) fn declared_workspace_dir_error(error: WorkspaceStoreError) -> anyhow::Error {
    anyhow::Error::new(error).context("workspace declaration coordination failed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorkspaceConfigStore;
    use tempfile::TempDir;

    fn owner(dir: &TempDir) -> WorkspaceDeclarations {
        WorkspaceDeclarations::new(
            WorkspaceConfigStore::new(dir.path()),
            dir.path().join("workspace-layout.json"),
        )
    }

    fn project(id: &str, name: &str, path: &str) -> ProjectRecord {
        ProjectRecord {
            id: id.into(),
            name: name.into(),
            path: path.into(),
            ssh_alias: None,
            updated_at: 1,
        }
    }

    fn declaration_path(dir: &TempDir, id: &str) -> PathBuf {
        dir.path().join("workspaces").join(format!("{id}.toml"))
    }

    #[test]
    fn first_publish_exports_all_projects_and_repeat_startup_keeps_files() {
        let dir = TempDir::new().unwrap();
        let owner = owner(&dir);
        owner
            .initialize(&[
                project("project-a", "Alpha", "/tmp/alpha"),
                project("project-b", "Beta", "/tmp/beta"),
            ])
            .unwrap();
        assert!(declaration_path(&dir, "project-a").exists());
        assert!(declaration_path(&dir, "project-b").exists());
        assert_eq!(owner.snapshot().len(), 2);

        // Repeated startup loads the published set; it neither deletes nor reintroduces rows.
        owner.initialize(&[]).unwrap();
        assert_eq!(owner.snapshot().len(), 2);
        assert!(declaration_path(&dir, "project-a").exists());
    }

    #[test]
    fn existing_conflicting_declaration_is_preserved() {
        let dir = TempDir::new().unwrap();
        let owner = owner(&dir);
        let store = WorkspaceConfigStore::new(dir.path());
        store
            .save(&WorkspaceDeclaration::new(
                "project-a",
                "User Edited",
                "/tmp/alpha",
                None,
            ))
            .unwrap();
        let error = owner
            .initialize(&[project("project-a", "From Database", "/tmp/alpha")])
            .unwrap_err();
        assert!(matches!(error, WorkspaceStoreError::Conflict { .. }));
        assert_eq!(store.read("project-a").unwrap().name, "User Edited");
    }

    #[test]
    fn external_edits_apply_only_on_explicit_reload() {
        let dir = TempDir::new().unwrap();
        let owner = owner(&dir);
        owner
            .initialize(&[project("project-a", "Alpha", "/tmp/alpha")])
            .unwrap();
        assert_eq!(owner.snapshot().get("project-a").unwrap().name, "Alpha");

        // An external edit is not observed by a pure in-memory read.
        WorkspaceConfigStore::new(dir.path())
            .save(&WorkspaceDeclaration::new(
                "project-a",
                "Edited On Disk",
                "/tmp/alpha",
                None,
            ))
            .unwrap();
        assert_eq!(owner.snapshot().get("project-a").unwrap().name, "Alpha");

        // Only an explicit reload publishes it.
        let candidate = owner.load_candidate().unwrap();
        owner.commit_snapshot(candidate).unwrap();
        assert_eq!(
            owner.snapshot().get("project-a").unwrap().name,
            "Edited On Disk"
        );
    }

    #[test]
    fn missing_or_corrupt_declarations_after_publish_are_rejected() {
        let dir = TempDir::new().unwrap();
        let owner = owner(&dir);
        owner
            .initialize(&[project("project-a", "Alpha", "/tmp/alpha")])
            .unwrap();

        std::fs::remove_file(declaration_path(&dir, "project-a")).unwrap();
        let missing = owner.initialize(&[]).unwrap_err();
        assert!(matches!(missing, WorkspaceStoreError::LayoutMismatch(_)));

        std::fs::write(declaration_path(&dir, "project-a"), "not-toml").unwrap();
        let corrupt = owner.load_candidate().unwrap_err();
        assert!(
            matches!(corrupt, WorkspaceStoreError::Declaration(_)),
            "{corrupt}"
        );
    }

    #[test]
    fn a_failed_declaration_write_keeps_the_published_snapshot() {
        let dir = TempDir::new().unwrap();
        let owner = owner(&dir);
        owner
            .initialize(&[project("project-a", "Alpha", "/tmp/alpha")])
            .unwrap();

        // An invalid declaration never becomes a published rename.
        let error = owner
            .declare(&WorkspaceDeclaration::new(
                "project-a",
                "   ",
                "/tmp/alpha",
                None,
            ))
            .unwrap_err();
        assert!(
            matches!(error, WorkspaceStoreError::Declaration(_)),
            "{error}"
        );
        assert_eq!(owner.snapshot().get("project-a").unwrap().name, "Alpha");
    }

    #[test]
    fn declare_updates_marker_and_snapshot_for_a_new_project() {
        let dir = TempDir::new().unwrap();
        let owner = owner(&dir);
        owner.initialize(&[]).unwrap();
        owner
            .declare(&WorkspaceDeclaration::new(
                "project-new",
                "New",
                "/tmp/new",
                None,
            ))
            .unwrap();
        assert_eq!(owner.snapshot().get("project-new").unwrap().name, "New");
        // The marker now covers the new id, so a reload accepts it.
        let candidate = owner.load_candidate().unwrap();
        owner.commit_snapshot(candidate).unwrap();
        assert!(owner.snapshot().contains_key("project-new"));
    }

    #[test]
    fn a_failed_marker_write_is_completed_by_the_durable_intent() {
        let dir = TempDir::new().unwrap();
        let owner = owner(&dir);
        owner
            .initialize(&[project("project-a", "Alpha", "/tmp/alpha")])
            .unwrap();

        // Occupy the marker path with a directory so the marker write fails after the file write.
        let marker = dir.path().join("workspace-layout.json");
        std::fs::remove_file(&marker).unwrap();
        std::fs::create_dir(&marker).unwrap();
        let error = owner
            .declare(&WorkspaceDeclaration::new(
                "project-b",
                "Beta",
                "/tmp/beta",
                None,
            ))
            .unwrap_err();
        assert!(matches!(error, WorkspaceStoreError::Io(_)), "{error}");
        // Not half-published: the memory snapshot is unchanged and the intent is durable.
        assert!(owner.snapshot().get("project-b").is_none());

        // The next startup completes the commit only from the recorded intent.
        std::fs::remove_dir(&marker).unwrap();
        owner.initialize(&[]).unwrap();
        assert_eq!(owner.snapshot().get("project-b").unwrap().name, "Beta");
    }

    #[test]
    fn an_extra_declaration_file_is_rejected() {
        let dir = TempDir::new().unwrap();
        let owner = owner(&dir);
        owner
            .initialize(&[project("project-a", "Alpha", "/tmp/alpha")])
            .unwrap();
        // A file that no intent or product row introduced is not adopted.
        WorkspaceConfigStore::new(dir.path())
            .save(&WorkspaceDeclaration::new(
                "project-extra",
                "Extra",
                "/tmp/extra",
                None,
            ))
            .unwrap();
        let error = owner.initialize(&[]).unwrap_err();
        assert!(
            matches!(error, WorkspaceStoreError::LayoutMismatch(_)),
            "{error}"
        );
        assert!(declaration_path(&dir, "project-extra").exists());
    }

    #[test]
    fn a_same_id_content_change_is_rejected_until_reload() {
        let dir = TempDir::new().unwrap();
        let owner = owner(&dir);
        owner
            .initialize(&[project("project-a", "Alpha", "/tmp/alpha")])
            .unwrap();
        // An external same-id edit is detected by the marker's content fingerprint.
        WorkspaceConfigStore::new(dir.path())
            .save(&WorkspaceDeclaration::new(
                "project-a",
                "Edited",
                "/tmp/alpha",
                None,
            ))
            .unwrap();
        let error = owner.initialize(&[]).unwrap_err();
        assert!(
            matches!(error, WorkspaceStoreError::LayoutMismatch(_)),
            "{error}"
        );

        // An explicit reload is the only entry point that accepts the edit.
        let candidate = owner.load_candidate().unwrap();
        owner.commit_snapshot(candidate).unwrap();
        assert_eq!(owner.snapshot().get("project-a").unwrap().name, "Edited");
        owner.initialize(&[]).unwrap();
    }
}
