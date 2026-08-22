use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadAgentItem {
    identity: ThreadAgentIdentity,
    state: ThreadAgentState,
}

impl ThreadAgentItem {
    pub fn new(identity: ThreadAgentIdentity, state: ThreadAgentState) -> Self {
        Self { identity, state }
    }

    pub fn identity(&self) -> &ThreadAgentIdentity {
        &self.identity
    }

    pub fn state(&self) -> &ThreadAgentState {
        &self.state
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadAgentIdentity {
    id: String,
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent_path: Option<String>,
    role: String,
    task: String,
    depth: u32,
}

impl ThreadAgentIdentity {
    pub fn new(id: String, path: String, role: String, task: String, depth: u32) -> Self {
        Self {
            id,
            path,
            parent_path: None,
            role,
            task,
            depth,
        }
    }

    pub fn with_parent_path(mut self, parent_path: Option<String>) -> Self {
        self.parent_path = parent_path;
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn parent_path(&self) -> Option<&str> {
        self.parent_path.as_deref()
    }

    pub fn role(&self) -> &str {
        &self.role
    }

    pub fn task(&self) -> &str {
        &self.task
    }

    pub fn depth(&self) -> u32 {
        self.depth
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum ThreadAgentState {
    Queued(QueuedThreadAgent),
    Running(RunningThreadAgent),
    Succeeded(SucceededThreadAgent),
    Denied(DeniedThreadAgent),
    Cancelled(CancelledThreadAgent),
    Failed(FailedThreadAgent),
}

impl ThreadAgentState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded(_) | Self::Denied(_) | Self::Cancelled(_) | Self::Failed(_)
        )
    }

    pub fn terminal_at(&self) -> Option<i64> {
        match self {
            Self::Succeeded(state) => Some(state.completed_at),
            Self::Denied(state) => Some(state.denied_at),
            Self::Cancelled(state) => Some(state.cancelled_at),
            Self::Failed(state) => Some(state.failed_at),
            Self::Queued(_) | Self::Running(_) => None,
        }
    }

    pub fn failure(&self) -> Option<&str> {
        match self {
            Self::Failed(state) => Some(&state.error),
            Self::Queued(_)
            | Self::Running(_)
            | Self::Succeeded(_)
            | Self::Denied(_)
            | Self::Cancelled(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueuedThreadAgent;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunningThreadAgent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SucceededThreadAgent {
    completed_at: i64,
    summary: String,
}

impl SucceededThreadAgent {
    pub fn new(completed_at: i64, summary: String) -> Self {
        Self {
            completed_at,
            summary,
        }
    }

    pub fn completed_at(&self) -> i64 {
        self.completed_at
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeniedThreadAgent {
    denied_at: i64,
    reason: String,
}

impl DeniedThreadAgent {
    pub fn new(denied_at: i64, reason: String) -> Self {
        Self { denied_at, reason }
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn denied_at(&self) -> i64 {
        self.denied_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelledThreadAgent {
    cancelled_at: i64,
    reason: String,
}

impl CancelledThreadAgent {
    pub fn new(cancelled_at: i64, reason: String) -> Self {
        Self {
            cancelled_at,
            reason,
        }
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn cancelled_at(&self) -> i64 {
        self.cancelled_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FailedThreadAgent {
    failed_at: i64,
    error: String,
}

impl FailedThreadAgent {
    pub fn new(failed_at: i64, error: String) -> Self {
        Self { failed_at, error }
    }

    pub fn error(&self) -> &str {
        &self.error
    }

    pub fn failed_at(&self) -> i64 {
        self.failed_at
    }
}
