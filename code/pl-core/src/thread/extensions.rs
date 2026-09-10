//! Versioned opaque application records with atomic compare-and-swap batches.
use super::*;
use std::collections::BTreeMap;

/// Current application record. Revision belongs to Thread; format/version belong to the producer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionRecord {
    pub revision: u64,
    pub payload: OpaquePayload,
}

/// Explicit extension mutations cannot grant framework authority.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum ExtensionMutation {
    Put {
        id: String,
        expected_revision: Option<u64>,
        payload: OpaquePayload,
    },
    Delete {
        id: String,
        expected_revision: u64,
    },
}

/// Committed versions, including deletion tombstones, retained for cold replay.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum ExtensionChange {
    Put { id: String, record: ExtensionRecord },
    Delete { id: String, revision: u64 },
}

impl Owner {
    pub(super) fn mutate_extensions(
        &mut self,
        mutations: Vec<ExtensionMutation>,
    ) -> Result<BTreeMap<String, ExtensionRecord>, ThreadError> {
        let result = stage_extensions(&mut self.state, mutations)?;
        self.publish();
        Ok(result)
    }

    pub(super) fn update_application(
        &mut self,
        update: ApplicationUpdate,
    ) -> Result<ThreadSnapshot, ThreadError> {
        if !self.pending.is_empty() {
            return Err(ThreadError::PendingTools);
        }
        let mut candidate = self.state.clone();
        stage_extensions(&mut candidate, update.mutations)?;
        super::facts::stage_facts(&mut candidate, update.facts)?;
        self.state = candidate;
        self.publish();
        Ok(self.state.clone())
    }
}

/// One atomic application checkpoint: opaque state mutations and its actual model projection.
#[derive(Debug)]
pub struct ApplicationUpdate {
    pub mutations: Vec<ExtensionMutation>,
    pub facts: Vec<RuntimeFact>,
}

pub(super) fn stage_extensions(
    state: &mut ThreadSnapshot,
    mutations: Vec<ExtensionMutation>,
) -> Result<BTreeMap<String, ExtensionRecord>, ThreadError> {
    if state.lifecycle != ThreadLifecycle::Open {
        return Err(ThreadError::Closed);
    }
    let mut records = state.extensions.clone();
    let mut sequence = state.extension_sequence;
    let mut changes = state.extension_changes.to_vec();
    for mutation in mutations {
        let (id, expected) = match &mutation {
            ExtensionMutation::Put {
                id,
                expected_revision,
                ..
            } => (id, *expected_revision),
            ExtensionMutation::Delete {
                id,
                expected_revision,
            } => (id, Some(*expected_revision)),
        };
        if id.is_empty() {
            return Err(ThreadError::InvalidIdentity);
        }
        let actual = records.get(id).map(|record| record.revision);
        if actual != expected {
            return Err(ThreadError::ExtensionConflict {
                id: id.clone(),
                expected,
                actual,
            });
        }
        sequence = sequence
            .checked_add(1)
            .ok_or(ThreadError::RevisionExhausted)?;
        match mutation {
            ExtensionMutation::Put { id, payload, .. } => {
                let record = ExtensionRecord {
                    revision: sequence,
                    payload,
                };
                records.insert(id.clone(), record.clone());
                changes.push(ExtensionChange::Put { id, record });
            }
            ExtensionMutation::Delete { id, .. } => {
                records.remove(&id);
                changes.push(ExtensionChange::Delete {
                    id,
                    revision: sequence,
                });
            }
        }
    }
    state.extensions = records.clone();
    state.extension_sequence = sequence;
    state.extension_changes = changes.into();
    Ok(records)
}
