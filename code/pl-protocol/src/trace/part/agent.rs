use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceAgentPart {
    identity: TraceAgentIdentity,
    state: TraceAgentState,
}

impl TraceAgentPart {
    pub fn new(identity: TraceAgentIdentity, state: TraceAgentState) -> Self {
        Self { identity, state }
    }

    pub fn identity(&self) -> &TraceAgentIdentity {
        &self.identity
    }

    pub fn state(&self) -> &TraceAgentState {
        &self.state
    }

    pub(super) fn transition(&self, next_state: TraceAgentState) -> Result<Self, &'static str> {
        let valid = match (&self.state, &next_state) {
            (TraceAgentState::Queued(_), TraceAgentState::Running(_))
            | (TraceAgentState::Queued(_), TraceAgentState::Denied(_))
            | (TraceAgentState::Queued(_), TraceAgentState::Cancelled(_))
            | (TraceAgentState::Queued(_), TraceAgentState::Failed(_))
            | (TraceAgentState::Running(_), TraceAgentState::Succeeded(_))
            | (TraceAgentState::Running(_), TraceAgentState::Cancelled(_))
            | (TraceAgentState::Running(_), TraceAgentState::Failed(_)) => true,
            (current, next) if current == next => true,
            _ => false,
        };
        if !valid {
            return Err("illegal agent trace lifecycle transition");
        }
        let mut next = self.clone();
        next.state = next_state;
        Ok(next)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceAgentIdentity {
    id: String,
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent_path: Option<String>,
    role: String,
    task: String,
    depth: u32,
}

impl TraceAgentIdentity {
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
pub enum TraceAgentState {
    Queued(QueuedTraceAgent),
    Running(RunningTraceAgent),
    Succeeded(SucceededTraceAgent),
    Denied(DeniedTraceAgent),
    Cancelled(CancelledTraceAgent),
    Failed(FailedTraceAgent),
}

impl TraceAgentState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded(_) | Self::Denied(_) | Self::Cancelled(_) | Self::Failed(_)
        )
    }

    pub fn summary(&self) -> Option<&str> {
        match self {
            Self::Succeeded(state) => Some(&state.summary),
            Self::Queued(_)
            | Self::Running(_)
            | Self::Denied(_)
            | Self::Cancelled(_)
            | Self::Failed(_) => None,
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
pub struct QueuedTraceAgent;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunningTraceAgent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SucceededTraceAgent {
    summary: String,
}

impl SucceededTraceAgent {
    pub fn new(summary: String) -> Self {
        Self { summary }
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeniedTraceAgent {
    reason: String,
}

impl DeniedTraceAgent {
    pub fn new(reason: String) -> Self {
        Self { reason }
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelledTraceAgent {
    reason: String,
}

impl CancelledTraceAgent {
    pub fn new(reason: String) -> Self {
        Self { reason }
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FailedTraceAgent {
    error: String,
}

impl FailedTraceAgent {
    pub fn new(error: String) -> Self {
        Self { error }
    }

    pub fn error(&self) -> &str {
        &self.error
    }
}
