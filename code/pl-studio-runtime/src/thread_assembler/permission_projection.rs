//! Typed Studio approval projection over authoritative core permission facts.
use super::{StudioThreadAssembler, approval::ApprovalPrompt};
use pl_core::{
    context::OpaquePayload,
    thread::permissions::{
        PermissionDecision, PermissionRecord, PermissionResolution, PermissionState,
    },
};
use pl_protocol::{
    CancelInteraction, InteractionCommand, InteractionRequest, InteractionScope,
    ResolveToolApproval, ToolApprovalResolution, ToolApprovalResolutionPayload,
};

#[derive(Debug, thiserror::Error)]
pub enum PermissionProjectionError {
    #[error(transparent)]
    Key(#[from] super::InteractionKeyError),
    #[error("unsupported approval payload {format} version {version}")]
    Unsupported { format: String, version: u32 },
    #[error("approval payload is invalid")]
    Decode(#[from] serde_json::Error),
    #[error("approval payload identity or decision contradicts its core record")]
    Identity,
    #[error("approval thread is not resident: {0}")]
    MissingThread(String),
    #[error("approval record is not present: {0}")]
    MissingPermission(String),
    #[error("approval projection transition failed")]
    Projection(#[from] pl_protocol::InteractionTransitionError),
    #[error("approval resolution was rejected by its Thread")]
    Thread(#[from] pl_core::thread::ThreadError),
    #[error("approval response could not be frozen")]
    Payload(#[from] pl_core::context::PayloadError),
}

fn require_format(
    payload: &OpaquePayload,
    expected: &str,
) -> Result<(), PermissionProjectionError> {
    if payload.format() != expected || payload.version() != 1 {
        return Err(PermissionProjectionError::Unsupported {
            format: payload.format().into(),
            version: payload.version(),
        });
    }
    Ok(())
}

/// Projects saved approval content without consulting current tool schemas or policy renderers.
///
/// # Errors
/// Rejects unknown content encodings and contradictory records while leaving raw history intact.
pub fn project_execution_permission(
    thread_id: &str,
    record: &PermissionRecord,
) -> Result<InteractionRequest, PermissionProjectionError> {
    if matches!(
        record.state,
        PermissionState::Pending | PermissionState::Cancelled
    ) && record.response.is_some()
    {
        return Err(PermissionProjectionError::Identity);
    }
    require_format(&record.payload, "pl.studio.tool-approval")?;
    let prompt: ApprovalPrompt = serde_json::from_str(record.payload.content())?;
    if prompt.name != record.tool_id {
        return Err(PermissionProjectionError::Identity);
    }
    let wire_id = super::interaction_key::encode(thread_id, &record.id);
    let mut projected = InteractionRequest::tool_approval(
        wire_id.clone(),
        InteractionScope {
            thread_id: thread_id.into(),
            turn_id: record.turn_id.clone(),
            item_id: Some(record.call_id.clone()),
            tool_id: Some(record.call_id.clone()),
            agent_path: Some(thread_id.into()),
            purpose: Default::default(),
        },
        pl_protocol::ToolApprovalRequest {
            name: prompt.name,
            arguments: serde_json::Value::String(prompt.arguments.content().into()),
            working_directory: prompt.working_directory,
            parent_agent_id: None,
        },
        record.created_at,
    );
    projected.revision = 1;
    let command = match record.state {
        PermissionState::Pending => None,
        PermissionState::Allowed | PermissionState::Denied => {
            let expected = match record.state {
                PermissionState::Allowed => ToolApprovalResolution::Approved,
                PermissionState::Denied => ToolApprovalResolution::Denied,
                PermissionState::Pending | PermissionState::Cancelled => {
                    return Err(PermissionProjectionError::Identity);
                }
            };
            let reason = match &record.response {
                Some(payload) => {
                    require_format(payload, "pl.studio.tool-approval-response")?;
                    let response: ToolApprovalResolutionPayload =
                        serde_json::from_str(payload.content())?;
                    if response.decision != expected {
                        return Err(PermissionProjectionError::Identity);
                    }
                    response.reason
                }
                None => None,
            };
            Some(InteractionCommand::ResolveToolApproval(
                ResolveToolApproval {
                    interaction_id: wire_id.clone(),
                    expected_revision: 1,
                    operation_id: format!("permission:{}:{}", record.id, record.revision),
                    resolved_at: record.updated_at,
                    decision: expected,
                    reason,
                },
            ))
        }
        PermissionState::Cancelled => Some(InteractionCommand::Cancel(CancelInteraction {
            interaction_id: wire_id.clone(),
            expected_revision: 1,
            operation_id: format!("permission:{}:{}", record.id, record.revision),
            reason: "The original execution permission is no longer active.".into(),
            cancelled_at: record.updated_at,
        })),
    };
    if let Some(command) = command {
        let decision = projected.decide(command)?;
        projected.apply(decision, record.updated_at);
    }
    if projected.revision != record.revision {
        return Err(PermissionProjectionError::Identity);
    }
    Ok(projected)
}

impl StudioThreadAssembler {
    /// Resolves a typed UI decision on the resident canonical owner and returns its resulting DTO.
    ///
    /// # Errors
    /// Rejects missing owners, unknown payload formats, stale revisions and expired execution leases.
    pub async fn resolve_tool_approval(
        &self,
        thread_id: &str,
        mut resolution: ResolveToolApproval,
    ) -> Result<InteractionRequest, PermissionProjectionError> {
        let (owner, local_id) = super::decode_interaction_key(&resolution.interaction_id)?;
        if owner != thread_id {
            return Err(PermissionProjectionError::Identity);
        }
        resolution.interaction_id = local_id.to_owned();
        let thread = {
            self.0
                .state()
                .entries
                .get(thread_id)
                .map(|entry| entry.thread.clone())
        }
        .ok_or_else(|| PermissionProjectionError::MissingThread(thread_id.into()))?;
        let snapshot = thread.snapshot();
        let current = snapshot
            .permissions
            .get(&resolution.interaction_id)
            .ok_or_else(|| {
                PermissionProjectionError::MissingPermission(resolution.interaction_id.clone())
            })?;
        project_execution_permission(thread_id, current)?;
        let decision = match resolution.decision {
            ToolApprovalResolution::Approved => PermissionDecision::Allow,
            ToolApprovalResolution::Denied => PermissionDecision::Deny,
        };
        let payload = OpaquePayload::new(
            "pl.studio.tool-approval-response",
            1,
            serde_json::to_string(&ToolApprovalResolutionPayload {
                decision: resolution.decision,
                reason: resolution.reason,
            })?,
        )?;
        let record = thread
            .resolve_execution_permission(PermissionResolution {
                id: resolution.interaction_id,
                expected_revision: resolution.expected_revision,
                decision,
                payload: Some(payload),
            })
            .await?;
        project_execution_permission(thread_id, &record)
    }
}
