use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingInteractionState {
    operation_id: String,
}

impl PendingInteractionState {
    pub fn new(operation_id: String) -> Self {
        Self { operation_id }
    }
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelledInteractionState {
    operation_id: String,
    cancelled_at: i64,
    reason: String,
}

impl CancelledInteractionState {
    pub fn new(operation_id: String, cancelled_at: i64, reason: String) -> Self {
        Self {
            operation_id,
            cancelled_at,
            reason,
        }
    }
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn cancelled_at(&self) -> i64 {
        self.cancelled_at
    }
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExpiredInteractionState {
    operation_id: String,
    expired_at: i64,
}

impl ExpiredInteractionState {
    pub fn new(operation_id: String, expired_at: i64) -> Self {
        Self {
            operation_id,
            expired_at,
        }
    }
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn expired_at(&self) -> i64 {
        self.expired_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelInteraction {
    pub interaction_id: String,
    pub expected_revision: u64,
    pub operation_id: String,
    pub reason: String,
    pub cancelled_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpireInteraction {
    pub interaction_id: String,
    pub expected_revision: u64,
    pub operation_id: String,
    pub expired_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReopenRecoveredInteraction {
    pub interaction_id: String,
    pub expected_revision: u64,
    pub operation_id: String,
    pub reopened_at: i64,
}
