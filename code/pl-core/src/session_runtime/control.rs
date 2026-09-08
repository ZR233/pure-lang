use crate::agent_runtime::agent_loop::{AgentLoopCommand, AgentLoopHandle};
use crate::{AgentRuntimeError, ThreadId};
use tokio::sync::oneshot;

use super::{
    SessionEventBatch, SessionMessageError, ToolTaskPage, ToolTaskSnapshot, ToolTaskStatus,
};

/// A tool's session-scoped control capability. It cannot select another Thread.
#[derive(Clone)]
pub struct SessionControl {
    actor: AgentLoopHandle,
    thread_id: ThreadId,
}

impl SessionControl {
    pub(crate) fn new(actor: AgentLoopHandle, thread_id: ThreadId) -> Self {
        Self { actor, thread_id }
    }

    /// Waits for events without acknowledging them or starting a Turn.
    ///
    /// # Errors
    /// Fails when the owner closes or another waiter is active.
    pub async fn wait(&self) -> Result<SessionEventBatch, SessionMessageError> {
        self.actor
            .ensure_ready()
            .map_err(|_| SessionMessageError::NotReady)?;
        let (reply, receiver) = oneshot::channel();
        self.actor
            .send(AgentLoopCommand::WaitSessionEvents { reply })
            .await
            .map_err(|_| SessionMessageError::OwnerUnavailable)?;
        receiver
            .await
            .map_err(|_| SessionMessageError::OwnerUnavailable)?
    }

    /// Queries task summaries without consuming events.
    ///
    /// # Errors
    /// Fails when the owner is unavailable.
    pub async fn list(
        &self,
        status: Option<ToolTaskStatus>,
        cursor: Option<String>,
    ) -> Result<ToolTaskPage, AgentRuntimeError> {
        self.actor.ensure_ready()?;
        let (reply, receiver) = oneshot::channel();
        self.actor
            .send(AgentLoopCommand::ListToolTasks {
                status,
                cursor,
                reply,
            })
            .await?;
        receiver
            .await
            .map_err(|_| AgentRuntimeError::ChannelClosed)?
    }

    /// Reads a bounded task preview, including results from an earlier Turn.
    ///
    /// # Errors
    /// Fails for unknown task IDs or an unavailable owner.
    pub async fn get(&self, task_id: String) -> Result<ToolTaskSnapshot, AgentRuntimeError> {
        self.actor.ensure_ready()?;
        let (reply, receiver) = oneshot::channel();
        self.actor
            .send(AgentLoopCommand::GetToolTask { id: task_id, reply })
            .await?;
        receiver
            .await
            .map_err(|_| AgentRuntimeError::ChannelClosed)?
    }

    /// Reads the complete immutable result, loading durable content on demand.
    ///
    /// This does not consume events or retain the loaded body in the owner.
    /// # Errors
    /// Returns an unknown task, unavailable owner, or storage integrity failure.
    pub async fn read_complete(
        &self,
        task_id: String,
    ) -> Result<ToolTaskSnapshot, AgentRuntimeError> {
        self.actor.ensure_ready()?;
        let (reply, receiver) = oneshot::channel();
        self.actor
            .send(AgentLoopCommand::ReadToolTaskResult { id: task_id, reply })
            .await?;
        receiver
            .await
            .map_err(|_| AgentRuntimeError::ChannelClosed)?
    }

    /// Requests cancellation; running work must exit before becoming cancelled.
    ///
    /// # Errors
    /// Fails for unknown tasks or an unavailable owner.
    pub async fn cancel(&self, task_id: String) -> Result<ToolTaskSnapshot, AgentRuntimeError> {
        self.actor.ensure_ready()?;
        let (reply, receiver) = oneshot::channel();
        self.actor
            .send(AgentLoopCommand::CancelToolTask { id: task_id, reply })
            .await?;
        receiver
            .await
            .map_err(|_| AgentRuntimeError::ChannelClosed)?
    }
}

impl std::fmt::Debug for SessionControl {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionControl")
            .field("thread_id", &self.thread_id)
            .finish_non_exhaustive()
    }
}
