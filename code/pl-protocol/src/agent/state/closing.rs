use serde::{Deserialize, Serialize};

/// 已拒绝新工作、正在释放资源的 Agent。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClosingAgentState {
    #[serde(default)]
    workspace_disposition: crate::AgentWorkspaceDisposition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    turn_id: Option<crate::TurnId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<crate::StateError>,
}

impl ClosingAgentState {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_disposition(mut self, disposition: crate::AgentWorkspaceDisposition) -> Self {
        self.workspace_disposition = disposition;
        self
    }
    pub fn with_turn(mut self, turn_id: Option<crate::TurnId>) -> Self {
        self.turn_id = turn_id;
        self
    }
    pub fn with_error(mut self, error: crate::StateError) -> Self {
        self.error = Some(error);
        self
    }
    pub fn workspace_disposition(&self) -> crate::AgentWorkspaceDisposition {
        self.workspace_disposition
    }
    pub fn turn_id(&self) -> Option<&crate::TurnId> {
        self.turn_id.as_ref()
    }
    pub fn error(&self) -> Option<&crate::StateError> {
        self.error.as_ref()
    }
    pub fn clear_turn(&mut self) {
        self.turn_id = None;
    }
}
