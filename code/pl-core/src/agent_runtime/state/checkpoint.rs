use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

const MAX_RECEIPTS: usize = 64;

/// Invalid, conflicting or expired checkpoint identities; never permission to replay effects.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CheckpointError {
    #[error("checkpoint sequence must be nonzero")]
    InvalidSequence,
    #[error("checkpoint {sequence} conflicts with its committed payload")]
    Conflict { sequence: u64 },
    #[error("checkpoint {sequence} is outside the retained receipt window")]
    Expired { sequence: u64 },
    #[error("invalid checkpoint receipt snapshot")]
    InvalidSnapshot,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReceiptState {
    through_sequence: u64,
    receipts: BTreeMap<u64, Receipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    fingerprint: String,
    inference_id: Option<String>,
}

/// Bounded committed checkpoint identities owned by the Thread, reset when a new Turn starts.
#[derive(Debug, Clone, Default)]
pub struct CheckpointHistory {
    state: Arc<ReceiptState>,
}

impl CheckpointHistory {
    pub(crate) fn is_replay(
        &self,
        sequence: u64,
        fingerprint: &str,
        inference_id: Option<&str>,
    ) -> Result<bool, CheckpointError> {
        if sequence == 0 {
            return Err(CheckpointError::InvalidSequence);
        }
        if let Some(receipt) = self.state.receipts.get(&sequence) {
            return compare(receipt, sequence, fingerprint);
        }
        if sequence <= self.state.through_sequence {
            return Err(CheckpointError::Expired { sequence });
        }
        if let Some(id) = inference_id
            && let Some(receipt) = self
                .state
                .receipts
                .values()
                .find(|receipt| receipt.inference_id.as_deref() == Some(id))
        {
            return compare(receipt, sequence, fingerprint);
        }
        Ok(false)
    }

    pub(crate) fn record(
        &mut self,
        sequence: u64,
        fingerprint: String,
        inference_id: Option<String>,
    ) -> Result<(), CheckpointError> {
        if sequence == 0 || sequence <= self.state.through_sequence {
            return Err(CheckpointError::InvalidSequence);
        }
        let state = Arc::make_mut(&mut self.state);
        state.receipts.insert(
            sequence,
            Receipt {
                fingerprint,
                inference_id,
            },
        );
        state.through_sequence = sequence;
        while state.receipts.len() > MAX_RECEIPTS {
            state.receipts.pop_first();
        }
        Ok(())
    }
}

fn compare(receipt: &Receipt, sequence: u64, fingerprint: &str) -> Result<bool, CheckpointError> {
    if receipt.fingerprint == fingerprint {
        Ok(true)
    } else {
        Err(CheckpointError::Conflict { sequence })
    }
}

impl Serialize for CheckpointHistory {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.state.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CheckpointHistory {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let state = ReceiptState::deserialize(deserializer)?;
        let mut inferences = std::collections::BTreeSet::new();
        if state.receipts.len() > MAX_RECEIPTS
            || state
                .receipts
                .last_key_value()
                .map_or(0, |(sequence, _)| *sequence)
                != state.through_sequence
            || state.receipts.iter().any(|(sequence, receipt)| {
                *sequence == 0
                    || receipt
                        .inference_id
                        .as_deref()
                        .is_some_and(|id| id.is_empty() || !inferences.insert(id))
                    || !receipt
                        .fingerprint
                        .strip_prefix("sha256:")
                        .is_some_and(|hash| {
                            hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                        })
            })
        {
            return Err(serde::de::Error::custom(CheckpointError::InvalidSnapshot));
        }
        Ok(Self {
            state: Arc::new(state),
        })
    }
}
