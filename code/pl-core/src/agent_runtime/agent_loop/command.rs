use tokio::sync::{mpsc, oneshot};

use super::super::*;
use super::running_turn::TurnCompletion;

pub(crate) enum AgentLoopCommand {
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
    PreviewConversationRecovery {
        target: ConversationRecoveryTarget,
        reply: oneshot::Sender<AgentRuntimeResult<ConversationRecoveryPreview>>,
    },
    RecoverConversation {
        request: ConversationRecoveryRequest,
        reply: oneshot::Sender<AgentRuntimeResult<ConversationRecoveryResult>>,
    },
    CancelTurn {
        turn_id: TurnId,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    SetActivity {
        turn_id: TurnId,
        kind: ActiveKind,
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
    ReadSession {
        reply: oneshot::Sender<AgentRuntimeResult<AgentSessionDigest>>,
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
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
    TurnFinished(Box<TurnCompletion>),
    Shutdown {
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
}

#[derive(Clone)]
pub(crate) struct AgentLoopHandle {
    sender: mpsc::Sender<AgentLoopCommand>,
}

impl AgentLoopHandle {
    pub(super) fn new(sender: mpsc::Sender<AgentLoopCommand>) -> Self {
        Self { sender }
    }

    pub(crate) async fn send(&self, command: AgentLoopCommand) -> AgentRuntimeResult<()> {
        self.sender
            .send(command)
            .await
            .map_err(|_| AgentRuntimeError::ChannelClosed)
    }
}
