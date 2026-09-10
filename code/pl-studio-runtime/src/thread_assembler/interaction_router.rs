//! Product interaction routing reads only canonical Thread snapshots.
use super::{StudioThreadAssembler, decode_interaction_key};
use pl_protocol::{InteractionRequest, InteractionResolution, InteractionStatus};

#[derive(Debug, thiserror::Error)]
pub enum StudioInteractionError {
    #[error(transparent)]
    Key(#[from] super::InteractionKeyError),
    #[error(transparent)]
    Permission(#[from] super::PermissionProjectionError),
    #[error(transparent)]
    Plan(#[from] super::PlanInteractionError),
    #[error(transparent)]
    User(#[from] super::UserInteractionError),
    #[error("interaction identity is missing, ambiguous or incompatible with the response")]
    Identity,
}

/// Projects one interaction from live or cold replay facts without activating execution.
///
/// # Errors
/// Rejects ambiguous identities and unsupported producer payload formats.
pub fn project_thread_interaction(
    thread_id: &str,
    local_id: &str,
    snapshot: &pl_core::thread::ThreadSnapshot,
) -> Result<Option<InteractionRequest>, StudioInteractionError> {
    match (
        snapshot.permissions.get(local_id),
        snapshot.interactions.get(local_id),
    ) {
        (Some(_), Some(_)) => Err(StudioInteractionError::Identity),
        (Some(permission), None) => Ok(Some(super::project_execution_permission(
            thread_id, permission,
        )?)),
        (None, Some(record)) => match record.request.payload.format() {
            "pl.studio.plan-confirmation" => {
                Ok(Some(super::project_plan_confirmation(thread_id, record)?))
            }
            _ => Ok(Some(super::project_user_input(thread_id, record)?)),
        },
        (None, None) => Ok(None),
    }
}

impl StudioThreadAssembler {
    /// Reads a current product interaction without constructing a second mutable interaction owner.
    ///
    /// # Errors
    /// Rejects malformed IDs, ambiguous records and unknown product payload encodings.
    pub fn read_product_interaction(
        &self,
        wire_id: &str,
    ) -> Result<Option<InteractionRequest>, StudioInteractionError> {
        let (thread_id, local_id) = decode_interaction_key(wire_id)?;
        let Some(thread) = self
            .0
            .state()
            .entries
            .get(thread_id)
            .map(|entry| entry.thread.clone())
        else {
            return Ok(None);
        };
        let snapshot = thread.snapshot();
        project_thread_interaction(thread_id, local_id, &snapshot)
    }

    /// Resolves the product's existing typed response envelope on the Thread encoded in its ID.
    ///
    /// # Errors
    /// Rejects wrong response kinds, stale records and failed canonical state transitions.
    pub async fn resolve_product_interaction(
        &self,
        wire_id: &str,
        response: InteractionResolution,
    ) -> Result<InteractionRequest, StudioInteractionError> {
        let current = self
            .read_product_interaction(wire_id)?
            .ok_or(StudioInteractionError::Identity)?;
        if current.status() != InteractionStatus::Pending {
            return if current.resolution().as_ref() == Some(&response) {
                Ok(current)
            } else {
                Err(StudioInteractionError::Identity)
            };
        }
        let operation_id = format!("resolve:{wire_id}");
        let resolved_at = crate::studio::unix_seconds();
        match response {
            InteractionResolution::ToolApproval(response)
                if current.kind() == pl_protocol::InteractionKind::ToolApproval =>
            {
                Ok(self
                    .resolve_tool_approval(
                        &current.scope.thread_id,
                        pl_protocol::ResolveToolApproval {
                            interaction_id: wire_id.into(),
                            expected_revision: current.revision,
                            operation_id,
                            resolved_at,
                            decision: response.decision,
                            reason: response.reason,
                        },
                    )
                    .await?)
            }
            InteractionResolution::UserInput(response)
                if current.kind() == pl_protocol::InteractionKind::UserInput =>
            {
                let command = pl_protocol::ResolveUserInput {
                    interaction_id: wire_id.into(),
                    expected_revision: current.revision,
                    operation_id,
                    resolved_at,
                    answers: response.answers,
                };
                match current.scope.purpose {
                    pl_protocol::InteractionPurpose::General => {
                        Ok(self.resolve_user_input(command).await?)
                    }
                    pl_protocol::InteractionPurpose::AgentSessionPlanConfirmation(_) => Ok(self
                        .resolve_plan_confirmation(&current.scope.thread_id, command)
                        .await?),
                }
            }
            InteractionResolution::ToolApproval(_) | InteractionResolution::UserInput(_) => {
                Err(StudioInteractionError::Identity)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{
        context::OpaquePayload, thread::interactions::InteractionRequest as CoreRequest,
    };

    #[tokio::test]
    async fn identical_local_question_ids_are_scoped_to_their_own_threads() {
        let root = tempfile::tempdir().unwrap();
        let owner = StudioThreadAssembler::default();
        for id in ["first", "second"] {
            let thread = owner
                .assemble(super::super::tests::spec(id, None, root.path()))
                .await
                .unwrap();
            let questions = vec![pl_protocol::UserQuestion {
                id: "q".into(),
                header: "Q".into(),
                question: id.into(),
                is_other: true,
                is_secret: false,
                options: None,
            }];
            thread
                .request_interaction(CoreRequest {
                    id: "same-question".into(),
                    turn_id: "turn".into(),
                    payload: OpaquePayload::new(
                        "pl.tool.user-input",
                        1,
                        serde_json::to_string(&questions).unwrap(),
                    )
                    .unwrap(),
                })
                .await
                .unwrap();
        }
        let first = super::super::interaction_key::encode("first", "same-question");
        let second = super::super::interaction_key::encode("second", "same-question");
        assert_ne!(first, second);
        assert_eq!(
            owner
                .read_product_interaction(&first)
                .unwrap()
                .unwrap()
                .scope
                .thread_id,
            "first"
        );
        assert_eq!(
            owner
                .read_product_interaction(&second)
                .unwrap()
                .unwrap()
                .scope
                .thread_id,
            "second"
        );
        assert!(owner.close_all().await.is_empty());
    }
}
