use crate::agent_runtime::agent_loop::{AgentLoopCommand, AgentLoopHandle};

/// Bounded presentation-only output. Final results and captured files are lossless.
#[derive(Clone)]
pub(crate) struct SessionTaskOutput {
    actor: AgentLoopHandle,
    id: String,
}

impl SessionTaskOutput {
    pub(crate) fn new(actor: AgentLoopHandle, id: String) -> Self {
        Self { actor, id }
    }

    pub(crate) fn emit(&self, text: String) -> Result<(), pl_trace::TraceEventSinkError> {
        let delta: String = text.chars().take(2048).collect();
        match self.actor.try_send(AgentLoopCommand::ToolTaskOutput {
            id: self.id.clone(),
            delta,
        }) {
            Ok(()) | Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => Ok(()),
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => Err(
                pl_trace::TraceEventSinkError::new("session output owner is closed"),
            ),
        }
    }
}
