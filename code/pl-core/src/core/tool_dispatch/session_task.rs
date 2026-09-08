use futures::FutureExt;

use crate::permission::{PermissionDecision, decide_tool_permission};
use crate::session_runtime::{SessionTaskSubmission, TaskStartPhase, ToolTaskReceipt};
use crate::tool::{
    ToolApprovalContext, ToolCallContext, ToolCallIdentity, ToolInput, ToolRuntimeLockPolicy,
};
use crate::turn::{ToolApprovalDecision, ToolExecutionMode, TurnOptions};

use super::super::permission::{
    approval_request, request_user_approval, requested_workspace_access,
};
use super::{ToolExecutionContext, ToolExecutionError, ToolExecutionOutcome, ToolExecutionRecord};

/// Prepares owned execution without polling a tool, requesting approval, or spawning work.
pub(super) fn prepare(
    call: &pl_model::completion::ToolCall,
    trace_part_id: &str,
    context: &ToolExecutionContext<'_>,
) -> Result<(SessionTaskSubmission, ToolExecutionRecord), ToolExecutionError> {
    let checkpoint = context.options.checkpoint.as_ref().ok_or_else(|| {
        ToolExecutionError::RespondToModel(
            "Session-owned tools require an AgentRuntime Thread owner.".into(),
        )
    })?;
    let binding = context.tool_plan.binding(&call.name).ok_or_else(|| {
        ToolExecutionError::RespondToModel(format!("Unknown tool: {}", call.name))
    })?;
    let runtime = checkpoint.runtime().clone();
    let agent_id = checkpoint.agent_id().clone();
    let task_identity = crate::canonical_json_hash(&serde_json::json!([
        agent_id.as_str(),
        context.turn_id,
        call.call_id,
    ]));
    let receipt = ToolTaskReceipt {
        status: pl_protocol::session_runtime::ToolTaskAcceptance::Accepted,
        task_id: format!("task-{}", task_identity.trim_start_matches("sha256:")),
        thread_id: agent_id.to_string(),
        turn_id: context.turn_id.to_owned(),
        call_id: call.call_id.clone(),
        item_id: trace_part_id.to_owned(),
        tool_name: call.name.clone(),
        tool_generation: binding.generation(),
    };
    let accepted = serde_json::to_string(&receipt)
        .map_err(|error| ToolExecutionError::Fatal(error.to_string()))?;
    let record = ToolExecutionRecord {
        id: call.id.clone(),
        call_id: call.call_id.clone(),
        name: call.name.clone(),
        kind: call.kind(),
        result: accepted.clone(),
        display_result: accepted,
        arguments: call.payload_text(),
        model_attachments: Vec::new(),
        outcome: ToolExecutionOutcome::Accepted,
        exit_code: None,
        timed_out: false,
        runtime_events: Vec::new(),
        execution_millis: 0,
    };
    let mut request = approval_request(call, context.active_subagent.as_ref());
    request.id = receipt.task_id.clone();
    let requested_access = requested_workspace_access(call, context.workspace.root());
    let decision = decide_tool_permission(context.options, &request, requested_access);
    let start_phase = match &decision {
        PermissionDecision::Approved { .. } => TaskStartPhase::Execution,
        PermissionDecision::NeedsUserApproval { .. } | PermissionDecision::NeedsAiReview { .. } => {
            TaskStartPhase::Approval
        }
    };
    let review = match &decision {
        PermissionDecision::NeedsAiReview { workspace_access } => Some(
            context
                .core
                .review_tool_call_with_ai(
                    &request,
                    context.options.permission_mode,
                    *workspace_access,
                    context.workspace.root(),
                )
                .boxed(),
        ),
        PermissionDecision::Approved { .. } | PermissionDecision::NeedsUserApproval { .. } => None,
    };
    let mut approval_options =
        TurnOptions::default().with_permission_mode(context.options.permission_mode);
    approval_options.interaction_callback = context.options.interaction_callback.clone();
    approval_options.user_input_mode = context.options.user_input_mode;
    let active = context.active_subagent.as_ref();
    let identity = ToolCallIdentity {
        call_id: call.call_id.clone(),
        item_id: trace_part_id.to_owned(),
        agent_id: agent_id.to_string(),
        parent_agent_id: active.and_then(|agent| agent.parent_id.clone()),
        agent_path: active.and_then(|agent| agent.agent_path.clone()),
        agent_role: active.map_or_else(|| "root".to_owned(), |agent| agent.role.clone()),
        agent_depth: active.map_or(0, |agent| agent.depth),
        session_id: context.session_id.to_owned(),
        turn_id: context.turn_id.to_owned(),
        step: context.step,
    };
    let task_id = receipt.task_id.clone();
    let tool_name = call.name.clone();
    let arguments = call.arguments_for_tool();
    let arguments_hash = crate::canonical_json_hash(&arguments);
    let manager = context.core.agent_tools.manager().clone();
    let plan = context.tool_plan.clone();
    let completion_callback = context.options.tool_completion_callback.clone();
    let call_id = call.call_id.clone();
    let cache = context.core.tool_session_runtime.cache();
    let cache_policy = binding.tool().policy().cache_policy(&arguments);
    let invalidates_cache = binding.tool().policy().invalidates_cache(&arguments);
    let effect = binding.tool().policy().effect();
    let executor_generation = binding.generation();
    let workspace_root = context.workspace.root().to_path_buf();
    let lock_policy = if matches!(
        context.options.tool_execution_mode,
        ToolExecutionMode::Sequential
    ) {
        ToolRuntimeLockPolicy::Exclusive
    } else {
        binding.tool().policy().runtime_lock_policy()
    };
    let submission = SessionTaskSubmission {
        receipt,
        arguments_hash,
        lock_policy,
        start_phase,
        execute: Box::new(move |cancellation| {
            async move {
            approval_options.cancellation_token = Some(cancellation.clone());
            let access = match decision {
                PermissionDecision::Approved { workspace_access } => workspace_access,
                PermissionDecision::NeedsUserApproval { workspace_access } => {
                    let approved = request_user_approval(&approval_options, &request, &identity.turn_id).await;
                    require_approval(&tool_name, approved)?;
                    workspace_access
                }
                PermissionDecision::NeedsAiReview { workspace_access } => {
                    let Some(review) = review else {
                        return Err(task_error(&tool_name, "AI approval was not prepared"));
                    };
                    let approved = tokio::select! {
                        approved = review => approved,
                        _ = cancellation.cancelled() => return Err(task_error(&tool_name, "Task cancelled during approval")),
                    };
                    require_approval(&tool_name, approved)?;
                    workspace_access
                }
            };
            if cancellation.is_cancelled() {
                return Err(task_error(&tool_name, "Task cancelled before execution"));
            }
            if matches!(start_phase, TaskStartPhase::Approval) {
                tokio::select! {
                    result = runtime.tool_task_running(&agent_id, task_id.clone()) => {
                        result.map_err(|error| task_error(&tool_name, &error.to_string()))?;
                    }
                    _ = cancellation.cancelled() => return Err(task_error(&tool_name, "Task cancelled before execution")),
                }
            }
            let output = runtime.task_output(&agent_id, task_id.clone()).await
                .map_err(|error| task_error(&tool_name, &error.to_string()))?;
            let approval = ToolApprovalContext::new(approval_options.permission_mode, access)
                .with_interaction(approval_options.interaction_callback, approval_options.user_input_mode);
            let context = ToolCallContext::for_task(identity, task_id, output)
                .with_approval(approval).with_cancellation(Some(cancellation));
            cache.record_effect(effect, false);
            let cached_arguments = arguments.clone();
            let result = cache.snapshot().execute_or_reuse(crate::tool::cache::ToolCacheExecutionRequest {
                tool_name: &tool_name, arguments: &cached_arguments, workspace_root: &workspace_root,
                policy: cache_policy, call_id: call_id.clone(), executor_generation,
            }, || manager.execute(&plan, &tool_name, ToolInput { arguments }, context)).await;
            cache.record_effect(effect, result.is_ok());
            if invalidates_cache { cache.invalidate_tool(&tool_name); }
            let mut result = result?;
            if let Some(callback) = completion_callback {
                let succeeded = result.success && !result.timed_out && !result.runtime_events.iter().any(|event| event.requires_control_policy() || matches!(event, crate::ToolDirective::ExecutionFailed));
                let observation = std::panic::AssertUnwindSafe(async {
                    callback(crate::ToolCompletion {
                    call_id, name: tool_name.clone(), status: if succeeded { "succeeded" } else { "failed" }.into(),
                    result: result.model_output().to_owned(), exit_code: result.exit_code, timed_out: result.timed_out,
                    }).await
                }).catch_unwind().await;
                let failure = match observation {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(error.to_string()),
                    Err(_) => Some("host completion observer panicked".to_owned()),
                };
                if let Some(error) = failure {
                    result.success = false;
                    result.model_output = crate::tool::model_visible_tool_output(&format!(
                        "Host post-tool observation failed after execution; side effects were not rolled back. The executor result is retained.\n{}", result.model_output()));
                    result.runtime_events.push(crate::ToolDirective::AuditMetadata {
                        metadata: serde_json::json!({
                            "kind": "toolCompletionObservationFailed",
                            "tool": tool_name,
                            "error": error,
                        }),
                    });
                }
            }
            Ok(result)
        }.boxed()
        }),
    };
    Ok((submission, record))
}

fn require_approval(tool: &str, decision: ToolApprovalDecision) -> crate::Result<()> {
    match decision {
        ToolApprovalDecision::Approved => Ok(()),
        ToolApprovalDecision::Denied { reason } => Err(task_error(
            tool,
            &format!("Tool execution denied: {reason}"),
        )),
    }
}

fn task_error(tool: &str, error: &str) -> crate::PureError {
    crate::PureError::ToolExecutionFailed {
        tool: tool.to_owned(),
        error: error.to_owned(),
    }
}
