use anyhow::Result;

use crate::studio::ids::unix_seconds;
use crate::{
    InteractionCommand, InteractionRequest, InteractionResolution, ResolveToolApproval,
    ResolveUserInput,
};

use super::StudioRuntime;

impl StudioRuntime {
    pub(super) async fn submit_durable_interaction_continuation(
        &self,
        current: &InteractionRequest,
        resolution: InteractionResolution,
        message: String,
        metadata: serde_json::Value,
    ) -> Result<InteractionRequest> {
        let thread_id = &current.scope.thread_id;
        let (handle, canonical_owner) = self.ensure_thread_agent(thread_id).await?;
        let mail_id =
            pl_core::AgentInteractionContinuationRequest::stable_mail_id(&current.interaction_id);
        let now = unix_seconds();
        let mut resolved = current.clone();
        let operation_id = format!("resolve:{}", current.interaction_id);
        let command = match resolution {
            InteractionResolution::UserInput(value) => {
                InteractionCommand::ResolveUserInput(ResolveUserInput {
                    interaction_id: current.interaction_id.clone(),
                    expected_revision: current.revision,
                    operation_id,
                    resolved_at: now,
                    answers: value.answers,
                })
            }
            InteractionResolution::ToolApproval(value) => {
                InteractionCommand::ResolveToolApproval(ResolveToolApproval {
                    interaction_id: current.interaction_id.clone(),
                    expected_revision: current.revision,
                    operation_id,
                    resolved_at: now,
                    decision: value.decision,
                    reason: value.reason,
                })
            }
        };
        let decision = resolved.decide(command)?;
        resolved.apply(decision, now);

        handle
            .submit_interaction_continuation(
                canonical_owner,
                pl_core::AgentInteractionContinuationRequest::new(
                    resolved.clone(),
                    pl_core::AgentCurrentSessionSubmitRequest::start(message)
                        .with_presentation(pl_core::MailboxPresentation::Hidden)
                        .with_mail_id(mail_id)
                        .with_metadata(metadata),
                ),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        Ok(resolved)
    }
}
