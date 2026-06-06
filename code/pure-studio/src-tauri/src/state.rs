use std::collections::HashMap;
use std::sync::Arc;

use pl_core::StudioRuntime;
use pl_protocol::PureError;
use serde::Serialize;
use tokio::sync::{Mutex, oneshot};
use tokio_util::sync::CancellationToken;

pub type ApprovalWaiters = Arc<Mutex<HashMap<String, ApprovalWaiter>>>;
pub type UserInputWaiters = Arc<Mutex<HashMap<String, UserInputWaiter>>>;
pub type ActiveTurns = Arc<Mutex<HashMap<String, CancellationToken>>>;
pub type CommandResult<T> = std::result::Result<T, CommandError>;

#[derive(Clone)]
pub struct AppState {
    pub studio: StudioRuntime,
    pub approvals: ApprovalWaiters,
    pub user_inputs: UserInputWaiters,
    pub active_turns: ActiveTurns,
}

pub struct ApprovalWaiter {
    pub session_id: String,
    pub sender: oneshot::Sender<pl_core::ToolApprovalDecision>,
}

pub struct UserInputWaiter {
    pub session_id: String,
    pub sender: oneshot::Sender<pl_core::UserInputResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub message: String,
}

impl CommandError {
    pub fn from_display(error: impl std::fmt::Display) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

impl From<anyhow::Error> for CommandError {
    fn from(error: anyhow::Error) -> Self {
        Self::from_display(error)
    }
}

impl From<PureError> for CommandError {
    fn from(error: PureError) -> Self {
        Self::from_display(error)
    }
}

impl From<std::io::Error> for CommandError {
    fn from(error: std::io::Error) -> Self {
        Self::from_display(error)
    }
}
