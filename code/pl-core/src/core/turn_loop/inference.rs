use pl_protocol::Result;
use pl_trace::AgentEvent;

use crate::AgentInferenceCommit;
use crate::runtime_usage::{agent_runtime_delta, identity_for_subagent};
use crate::session::AgentSession;
use crate::trace::TraceRecorder;
use crate::turn::TurnOptions;

pub(super) fn from_billing(
    active_subagent: Option<&crate::tool::SubagentContext>,
    billing: pl_protocol::InferenceBillingRecord,
) -> AgentInferenceCommit {
    AgentInferenceCommit {
        runtime_delta: agent_runtime_delta(identity_for_subagent(active_subagent), &billing),
        billing,
    }
}

pub(super) async fn record(
    options: &TurnOptions,
    session: &AgentSession,
    recorder: &mut TraceRecorder,
    inference: AgentInferenceCommit,
) -> Result<()> {
    let Some(checkpoint) = &options.checkpoint else {
        recorder.broadcast(AgentEvent::AgentRuntimeUpdated {
            delta: inference.runtime_delta,
        });
        return Ok(());
    };
    let consumed_mail_ids = match &options.mailbox {
        Some(mailbox) => mailbox.pending_acknowledgements().await,
        None => Vec::new(),
    };
    checkpoint
        .checkpoint_inference_mailbox(session.clone(), inference, consumed_mail_ids.clone())
        .await
        .map_err(|error| pl_protocol::PureError::MemoryError(error.to_string()))?;
    if let Some(mailbox) = &options.mailbox {
        mailbox.acknowledge(&consumed_mail_ids).await;
    }
    Ok(())
}
