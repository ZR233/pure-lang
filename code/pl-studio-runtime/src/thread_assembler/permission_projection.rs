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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn record() -> PermissionRecord {
        let prompt = ApprovalPrompt {
            name: "exec".into(),
            arguments: OpaquePayload::new("custom.command", 17, "  任意命令\r\n9007199254740993 ")
                .unwrap(),
            working_directory: Some("/outside".into()),
        };
        PermissionRecord {
            id: "permission:call".into(),
            task_id: "task:call".into(),
            call_id: "call".into(),
            turn_id: "turn".into(),
            tool_id: "exec".into(),
            revision: 1,
            payload: OpaquePayload::new(
                "pl.studio.tool-approval",
                1,
                serde_json::to_string(&prompt).unwrap(),
            )
            .unwrap(),
            state: PermissionState::Pending,
            created_at: 10,
            updated_at: 10,
            response: None,
        }
    }

    #[test]
    fn approval_dto_preserves_original_arguments_times_and_human_response() {
        let mut saved = record();
        let pending = project_execution_permission("thread", &saved).unwrap();
        let pl_protocol::InteractionContent::ToolApproval(content) = &pending.content else {
            panic!("expected approval content")
        };
        assert_eq!(
            content.request().arguments,
            serde_json::Value::String("  任意命令\r\n9007199254740993 ".into())
        );
        assert_eq!(pending.revision, 1);
        assert_eq!(pending.created_at, 10);
        let response = ToolApprovalResolutionPayload {
            decision: ToolApprovalResolution::Denied,
            reason: Some("  保留原样理由\n".into()),
        };
        saved.state = PermissionState::Denied;
        saved.revision = 2;
        saved.updated_at = 12;
        saved.response = Some(
            OpaquePayload::new(
                "pl.studio.tool-approval-response",
                1,
                serde_json::to_string(&response).unwrap(),
            )
            .unwrap(),
        );
        let terminal = project_execution_permission("thread", &saved).unwrap();
        assert_eq!(terminal.revision, 2);
        assert_eq!(terminal.updated_at, 12);
        assert_eq!(
            terminal.resolution(),
            Some(pl_protocol::InteractionResolution::ToolApproval(response))
        );
    }

    #[test]
    fn unknown_approval_format_is_not_projected_as_an_empty_or_approved_request() {
        let mut saved = record();
        let raw = "opaque future record\n";
        saved.payload = OpaquePayload::new("future.approval", 88, raw).unwrap();
        assert!(matches!(
            project_execution_permission("thread", &saved),
            Err(PermissionProjectionError::Unsupported { .. })
        ));
        assert_eq!(saved.payload.content(), raw);
    }
}
