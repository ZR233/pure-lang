use tokio::sync::{mpsc, oneshot};

use super::super::*;
use super::running_turn::TurnCompletion;

pub(crate) enum AgentLoopCommand {
    AdmitToolTasks {
        turn_id: TurnId,
        tasks: Vec<crate::session_runtime::SessionTaskSubmission>,
        reply: oneshot::Sender<AgentRuntimeResult<tokio::time::Instant>>,
    },
    SelectToolTaskResults {
        turn_id: TurnId,
        ids: Vec<String>,
        deadline: tokio::time::Instant,
        reply: oneshot::Sender<AgentRuntimeResult<Vec<crate::session_runtime::ToolTaskSnapshot>>>,
    },
    ToolTaskOutput {
        id: String,
        delta: String,
    },
    ListToolTasks {
        status: Option<crate::session_runtime::ToolTaskStatus>,
        cursor: Option<String>,
        reply: oneshot::Sender<AgentRuntimeResult<crate::session_runtime::ToolTaskPage>>,
    },
    ReadToolTaskResult {
        id: String,
        reply: oneshot::Sender<AgentRuntimeResult<crate::session_runtime::ToolTaskSnapshot>>,
    },
    GetToolTask {
        id: String,
        reply: oneshot::Sender<AgentRuntimeResult<crate::session_runtime::ToolTaskSnapshot>>,
    },
    CancelToolTask {
        id: String,
        reply: oneshot::Sender<AgentRuntimeResult<crate::session_runtime::ToolTaskSnapshot>>,
    },
    ToolTaskRunning {
        id: String,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    WaitSessionEvents {
        reply: oneshot::Sender<
            Result<
                crate::session_runtime::SessionEventBatch,
                crate::session_runtime::SessionMessageError,
            >,
        >,
    },
    PublishSessionEvent {
        source: String,
        id: String,
        event: Box<crate::session_runtime::SessionWakeEvent>,
        reply: oneshot::Sender<
            Result<
                crate::session_runtime::SessionMessageReceipt,
                crate::session_runtime::SessionMessageError,
            >,
        >,
    },
    Submit {
        request: AgentSubmitRequest,
        reply: oneshot::Sender<AgentRuntimeResult<TurnId>>,
    },
    SubmitCurrentSession {
        root_agent_id: ThreadId,
        request: AgentCurrentSessionSubmitRequest,
        reply: oneshot::Sender<AgentRuntimeResult<TurnId>>,
    },
    SubmitInteractionContinuation {
        root_agent_id: ThreadId,
        request: Box<AgentInteractionContinuationRequest>,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    ReconfigureIdleRole {
        role: crate::AgentRoleId,
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
    ChangeIdleThreadMode {
        mode_id: pl_protocol::ThreadModeId,
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
    PreviewConversationRecovery {
        target: ConversationRecoveryTarget,
        reply: oneshot::Sender<AgentRuntimeResult<ConversationRecoveryPreview>>,
    },
    RecoverConversation {
        request: ConversationRecoveryRequest,
        reply: oneshot::Sender<AgentRuntimeResult<ConversationRecoveryResult>>,
    },
    RecoverFaulted {
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
    CancelTurn {
        turn_id: TurnId,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    SetActivity {
        turn_id: TurnId,
        activity: AgentActivityUpdate,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    Checkpoint {
        checkpoint: Box<AgentTurnCheckpoint>,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    RecordThreadFacts {
        thread_id: ThreadId,
        facts: Vec<crate::ThreadNotificationFact>,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    Snapshot {
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
    ReportProgress {
        stage: AgentProgressStage,
        summary: String,
        next_step: String,
        detail: Option<String>,
        reply: oneshot::Sender<AgentRuntimeResult<AgentProgressCheckpoint>>,
    },
    ReadThreadContext {
        reply: oneshot::Sender<AgentRuntimeResult<ThreadContextState>>,
    },
    ReadSubmissions {
        offset: usize,
        limit: usize,
        reply: oneshot::Sender<AgentRuntimeResult<AgentSubmissionPage>>,
    },
    StartPendingInputs {
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    Close {
        workspace_disposition: pl_protocol::AgentWorkspaceDisposition,
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
    TurnFinished(Box<TurnCompletion>),
    Evict {
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
    Shutdown {
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
}

#[derive(Clone)]
pub(crate) struct AgentLoopHandle {
    ready: std::sync::Arc<std::sync::atomic::AtomicBool>,
    sender: mpsc::Sender<AgentLoopCommand>,
    cancellation: tokio_util::sync::CancellationToken,
    inbox_changed: std::sync::Arc<tokio::sync::Notify>,
    tasks_changed: std::sync::Arc<tokio::sync::Notify>,
}

impl AgentLoopHandle {
    pub(crate) fn try_send(
        &self,
        command: AgentLoopCommand,
    ) -> Result<(), mpsc::error::TrySendError<()>> {
        self.sender.try_send(command).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => mpsc::error::TrySendError::Full(()),
            mpsc::error::TrySendError::Closed(_) => mpsc::error::TrySendError::Closed(()),
        })
    }
    pub(super) fn new(sender: mpsc::Sender<AgentLoopCommand>) -> Self {
        Self {
            ready: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            sender,
            cancellation: tokio_util::sync::CancellationToken::new(),
            inbox_changed: std::sync::Arc::new(tokio::sync::Notify::new()),
            tasks_changed: std::sync::Arc::new(tokio::sync::Notify::new()),
        }
    }

    pub(crate) fn cancellation(&self) -> tokio_util::sync::CancellationToken {
        self.cancellation.clone()
    }
    pub(crate) fn mark_ready(&self) {
        self.ready.store(true, std::sync::atomic::Ordering::Release);
    }

    pub(crate) fn ensure_ready(&self) -> AgentRuntimeResult<()> {
        if self.ready.load(std::sync::atomic::Ordering::Acquire) {
            Ok(())
        } else {
            Err(AgentRuntimeError::NotReady)
        }
    }
    pub(crate) fn inbox_changed(&self) -> std::sync::Arc<tokio::sync::Notify> {
        self.inbox_changed.clone()
    }
    pub(crate) fn tasks_changed(&self) -> std::sync::Arc<tokio::sync::Notify> {
        self.tasks_changed.clone()
    }

    pub(crate) async fn send(&self, command: AgentLoopCommand) -> AgentRuntimeResult<()> {
        self.sender
            .send(command)
            .await
            .map_err(|_| AgentRuntimeError::ChannelClosed)
    }
}
