//! Immutable history contracts and side-effect-free replay, available without database features.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Immutable envelope returned by the session owner. Payload interpretation belongs to its type owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntry {
    pub session_id: String,
    pub id: String,
    pub turn_id: Option<String>,
    pub type_id: String,
    pub schema_version: u32,
    pub ordinal: u64,
    pub revision: u64,
    pub created_at: i64,
    pub updated_at: i64,
    /// Content encoded by its owner. Storage must not parse or normalize these bytes.
    pub payload: String,
}

/// A committed change to a logical entry. Replacements retain the preceding history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SessionEntryChange {
    Put { entry: SessionEntry },
    Delete { entry_id: String },
}

/// One atomic group of entry changes. Contents remain opaque during replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntryCommit {
    pub(crate) session_id: String,
    pub(crate) sequence: u64,
    pub(crate) thread_revision: Option<u64>,
    pub(crate) changes: Vec<SessionEntryChange>,
}

impl SessionEntryCommit {
    /// Returns the owning session.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Returns the contiguous, one-based history sequence for this session.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Returns the Thread revision, absent for independently registered immutable resources.
    pub fn thread_revision(&self) -> Option<u64> {
        self.thread_revision
    }

    /// Returns ordered changes without interpreting their payloads.
    pub fn changes(&self) -> &[SessionEntryChange] {
        &self.changes
    }
}

/// A violation of history identity, ordering, or envelope integrity.
#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    #[error("history ownership or sequence mismatch at commit {sequence}")]
    Ordering { sequence: u64 },
    #[error("invalid history record envelope: {id}")]
    InvalidEnvelope { id: String },
    #[error("history deletes missing record: {id}")]
    MissingRecord { id: String },
    #[error("history sequence is exhausted")]
    SequenceExhausted,
}

impl SessionEntryCommit {
    /// Creates a commit envelope; replay validates its sequence against preceding commits.
    ///
    /// # Errors
    /// Rejects zero sequences, missing identities and invalid record envelopes.
    pub fn new(
        session_id: String,
        sequence: u64,
        thread_revision: Option<u64>,
        changes: Vec<SessionEntryChange>,
    ) -> Result<Self, ReplayError> {
        let commit = Self {
            session_id,
            sequence,
            thread_revision,
            changes,
        };
        validate_commit(&commit, &commit.session_id, sequence)?;
        Ok(commit)
    }
}

pub(crate) fn validate_commit(
    commit: &SessionEntryCommit,
    session_id: &str,
    sequence: u64,
) -> Result<(), ReplayError> {
    if session_id.is_empty()
        || commit.session_id != session_id
        || sequence == 0
        || commit.sequence != sequence
    {
        return Err(ReplayError::Ordering { sequence });
    }
    for change in &commit.changes {
        match change {
            SessionEntryChange::Put { entry } => {
                if entry.session_id != session_id
                    || entry.id.is_empty()
                    || entry.type_id.is_empty()
                    || entry.schema_version == 0
                {
                    return Err(ReplayError::InvalidEnvelope {
                        id: entry.id.clone(),
                    });
                }
            }
            SessionEntryChange::Delete { entry_id } => {
                if entry_id.is_empty() {
                    return Err(ReplayError::InvalidEnvelope {
                        id: entry_id.clone(),
                    });
                }
            }
        }
    }
    Ok(())
}

/// Materialized history at a validated commit boundary. It owns no model or tool instances.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplayState {
    session_id: Option<String>,
    sequence: u64,
    entries: BTreeMap<String, SessionEntry>,
}

impl ReplayState {
    /// Applies one complete commit atomically, without interpreting payload content.
    ///
    /// # Errors
    /// Rejects missing commits, ownership changes, invalid envelopes and missing deletion targets.
    /// The prior state remains unchanged on any error.
    pub fn apply(&mut self, commit: &SessionEntryCommit) -> Result<(), ReplayError> {
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or(ReplayError::SequenceExhausted)?;
        let session_id = self.session_id.as_deref().unwrap_or(&commit.session_id);
        validate_commit(commit, session_id, sequence)?;
        // Validate the complete ordered batch before mutating the materialized records.
        let mut presence = BTreeMap::new();
        for change in &commit.changes {
            match change {
                SessionEntryChange::Put { entry } => {
                    presence.insert(entry.id.as_str(), true);
                }
                SessionEntryChange::Delete { entry_id } => {
                    let exists = presence
                        .get(entry_id.as_str())
                        .copied()
                        .unwrap_or_else(|| self.entries.contains_key(entry_id));
                    if !exists {
                        return Err(ReplayError::MissingRecord {
                            id: entry_id.clone(),
                        });
                    }
                    presence.insert(entry_id.as_str(), false);
                }
            }
        }
        for change in &commit.changes {
            match change {
                SessionEntryChange::Put { entry } => {
                    self.entries.insert(entry.id.clone(), entry.clone());
                }
                SessionEntryChange::Delete { entry_id } => {
                    self.entries.remove(entry_id);
                }
            }
        }
        self.session_id = Some(commit.session_id.clone());
        self.sequence = sequence;
        Ok(())
    }

    /// Last successfully applied commit sequence; zero means no history has been loaded.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Materializes immutable entry snapshots in stored ordinal order.
    pub fn entries(&self) -> Vec<SessionEntry> {
        let mut entries = self.entries.values().cloned().collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            left.ordinal
                .cmp(&right.ordinal)
                .then(left.id.cmp(&right.id))
        });
        entries
    }
}

/// Replays complete contiguous history without invoking payload decoders.
///
/// # Errors
/// Rejects invalid history using the same checks as incremental replay.
pub fn replay_entries(history: &[SessionEntryCommit]) -> Result<Vec<SessionEntry>, ReplayError> {
    let mut state = ReplayState::default();
    for commit in history {
        state.apply(commit)?;
    }
    Ok(state.entries())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> SessionEntry {
        SessionEntry {
            session_id: "thread".into(),
            id: "custom".into(),
            type_id: "unknown/v99".into(),
            schema_version: 99,
            ordinal: 1,
            turn_id: Some("turn".into()),
            revision: 1,
            created_at: 1,
            updated_at: 1,
            payload: "  原文\0not JSON\n".into(),
        }
    }

    #[test]
    fn memory_replay_preserves_unknown_content_and_validates_sequence() {
        let original = entry();
        let first = SessionEntryCommit::new(
            "thread".into(),
            1,
            None,
            vec![SessionEntryChange::Put {
                entry: original.clone(),
            }],
        )
        .unwrap();
        assert_eq!(
            replay_entries(std::slice::from_ref(&first)).unwrap(),
            vec![original]
        );
        let gap = SessionEntryCommit::new("thread".into(), 3, None, Vec::new()).unwrap();
        assert!(matches!(
            replay_entries(&[first.clone(), gap]),
            Err(ReplayError::Ordering { .. })
        ));
        let deletion = SessionEntryCommit::new(
            "thread".into(),
            2,
            None,
            vec![SessionEntryChange::Delete {
                entry_id: "custom".into(),
            }],
        )
        .unwrap();
        assert!(replay_entries(&[first, deletion]).unwrap().is_empty());
    }
    #[test]
    fn invalid_commit_leaves_incremental_replay_at_the_previous_boundary() {
        let first = SessionEntryCommit::new(
            "thread".into(),
            1,
            None,
            vec![SessionEntryChange::Put { entry: entry() }],
        )
        .unwrap();
        let mut state = ReplayState::default();
        state.apply(&first).unwrap();
        let previous = state.clone();
        let invalid = SessionEntryCommit::new(
            "thread".into(),
            2,
            None,
            vec![
                SessionEntryChange::Delete {
                    entry_id: "custom".into(),
                },
                SessionEntryChange::Delete {
                    entry_id: "absent".into(),
                },
            ],
        )
        .unwrap();
        assert!(matches!(
            state.apply(&invalid),
            Err(ReplayError::MissingRecord { .. })
        ));
        assert_eq!(state, previous);
        let valid = SessionEntryCommit::new(
            "thread".into(),
            2,
            None,
            vec![SessionEntryChange::Delete {
                entry_id: "custom".into(),
            }],
        )
        .unwrap();
        state.apply(&valid).unwrap();
        assert_eq!(state.sequence(), 2);
        assert_eq!(state.entries(), replay_entries(&[first, valid]).unwrap());
    }
}
