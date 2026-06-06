use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use futures::future::BoxFuture;
use pl_model::ToolCallKind;
use pl_protocol::TimelineItemStatus;
use tokio::sync::RwLock;

use crate::permission::{PermissionDecision, decide_tool_permission};
use crate::tool::{
    RECOVERABLE_SUBAGENT_429_MARKER, SubagentContext, ToolContext, ToolInput, ToolOutput,
    WorkspaceAccess,
};
use crate::turn::{BudgetTracker, ToolApprovalDecision, ToolExecutionMode, TurnOptions};

use super::PureCore;
use super::permission::{approval_request, request_user_approval, requested_workspace_access};
use super::turn_result::{is_cancelled, tool_allowed_in_mode, unix_seconds};

pub(super) struct ToolExecutionRecord {
    pub(super) call_id: String,
    pub(super) name: String,
    pub(super) kind: ToolCallKind,
    pub(super) result: String,
    pub(super) display_result: String,
    pub(super) arguments: String,
    pub(super) status: TimelineItemStatus,
    pub(super) exit_code: Option<i32>,
    pub(super) timed_out: bool,
}

pub(super) struct ScheduledToolExecution<'a> {
    pub(super) tool_call: pl_model::ToolCall,
    pub(super) item: pl_protocol::TimelineItem,
    pub(super) future: BoxFuture<'a, ToolExecutionRecord>,
}

pub(super) struct ToolExecutionContext<'a> {
    pub(super) core: &'a PureCore,
    pub(super) options: &'a TurnOptions,
    pub(super) mode: crate::turn::CompileMode,
    pub(super) session_id: &'a str,
    pub(super) workspace_root: &'a Path,
    pub(super) workspace_instructions: Option<String>,
    pub(super) active_subagent: Option<SubagentContext>,
    pub(super) agent_control: crate::AgentControl,
}

pub(super) async fn execute_tool_calls(
    tool_calls: &[pl_model::ToolCall],
    budget_tracker: &mut BudgetTracker,
    recorder: &mut crate::trace::TraceRecorder,
    context: ToolExecutionContext<'_>,
) -> Vec<ToolExecutionRecord> {
    let mut scheduled = Vec::new();
    let mut initial_items = HashMap::new();
    let runtime_lock = Arc::new(RwLock::new(()));

    for tool_call in tool_calls {
        if is_cancelled(context.options) {
            break;
        }
        let tool_call_id = tool_call
            .call_id
            .clone()
            .unwrap_or_else(|| tool_call.id.clone());
        let timeline_item_id = namespaced_tool_timeline_item_id(context.session_id, &tool_call_id);
        let mut item = recorder.tool_item(
            context.session_id,
            &timeline_item_id,
            tool_call.name.clone(),
            tool_call.payload_text(),
            tool_call.call_id.clone(),
            Some(tool_call.id.clone()),
        );
        initial_items.insert(timeline_item_id.clone(), item.clone());
        recorder.start_item(item.clone());
        budget_tracker.record_tool_call(&tool_call.name);

        if !tool_allowed_in_mode(context.mode, &tool_call.name) {
            let mode = context.mode.label();
            let name = &tool_call.name;
            let message = format!("Tool disabled in {mode} mode: {name}");
            item.status = TimelineItemStatus::Denied;
            item.updated_at = unix_seconds();
            if let Some(tool) = &mut item.tool {
                tool.denial_reason = Some(message.clone());
                tool.result = Some(message.clone());
            }
            recorder.complete_item(item);
            scheduled.push(ScheduledToolExecution {
                tool_call: tool_call.clone(),
                item: initial_items[&timeline_item_id].clone(),
                future: Box::pin(ready_tool_execution_record(
                    tool_call.clone(),
                    message,
                    TimelineItemStatus::Denied,
                    None,
                    false,
                )),
            });
            continue;
        }

        let Some(tool) = context.core.tools.get(&tool_call.name) else {
            let available: Vec<&str> = context.core.tools.names();
            eprintln!(
                "[pl-core] Unknown tool: {:?}, available: {:?}",
                tool_call.name, available
            );
            item.status = TimelineItemStatus::Failed;
            item.updated_at = unix_seconds();
            if let Some(tool) = &mut item.tool {
                tool.result = Some(format!("Unknown tool: {}", tool_call.name));
            }
            recorder.fail_item(item, format!("Unknown tool: {}", tool_call.name));
            scheduled.push(ScheduledToolExecution {
                tool_call: tool_call.clone(),
                item: initial_items[&timeline_item_id].clone(),
                future: Box::pin(ready_tool_execution_record(
                    tool_call.clone(),
                    format!("Unknown tool: {}", tool_call.name),
                    TimelineItemStatus::Failed,
                    None,
                    false,
                )),
            });
            continue;
        };

        let supports_parallel = tool.supports_parallel_tool_calls()
            && matches!(
                context.options.tool_execution_mode,
                ToolExecutionMode::ModelDefault | ToolExecutionMode::Parallel
            );
        let tool_context = ToolContext {
            event_tx: recorder.sender().clone(),
            options: context.options.clone(),
            workspace_access: WorkspaceAccess::WorkspaceOnly,
            mode: context.mode,
            workspace_root: context.workspace_root.to_path_buf(),
            workspace_instructions: context.workspace_instructions.clone(),
            active_subagent: context.active_subagent.clone(),
            agent_control: context.agent_control.clone(),
        };
        let approval_request = approval_request(tool_call, &tool_context);
        let requested_access = requested_workspace_access(tool_call, context.workspace_root);
        let mut execution_workspace_access = WorkspaceAccess::WorkspaceOnly;
        let decision = match decide_tool_permission(
            context.options,
            context.mode,
            &approval_request,
            requested_access,
        ) {
            PermissionDecision::Approved { workspace_access } => {
                execution_workspace_access = workspace_access;
                ToolApprovalDecision::Approved
            }
            PermissionDecision::NeedsUserApproval { workspace_access } => {
                let decision = request_user_approval(
                    context.options,
                    &approval_request,
                    recorder.sender().clone(),
                )
                .await;
                if matches!(decision, ToolApprovalDecision::Approved) {
                    execution_workspace_access = workspace_access;
                }
                decision
            }
            PermissionDecision::NeedsAiReview { workspace_access } => {
                let mut review_context = tool_context.clone();
                review_context.workspace_access = workspace_access;
                let decision = context
                    .core
                    .review_tool_call_with_ai(&approval_request, &review_context)
                    .await;
                if matches!(decision, ToolApprovalDecision::Approved) {
                    execution_workspace_access = workspace_access;
                }
                decision
            }
            PermissionDecision::Denied { reason } => ToolApprovalDecision::Denied { reason },
        };
        if is_cancelled(context.options) {
            return Vec::new();
        }

        match decision {
            ToolApprovalDecision::Approved => {
                let mut tool_context = tool_context;
                tool_context.workspace_access = execution_workspace_access;
                item.status = TimelineItemStatus::Approved;
                item.updated_at = unix_seconds();
                recorder.complete_item(item.clone());
                let tool_input = ToolInput {
                    arguments: tool_call.arguments_for_tool(),
                    session_id: context.session_id.to_string(),
                    tool_id: tool_call_id.clone(),
                };
                let lock = runtime_lock.clone();
                let tool_name = tool_call.name.clone();
                let tool_call_for_task = tool_call.clone();
                scheduled.push(ScheduledToolExecution {
                    tool_call: tool_call.clone(),
                    item,
                    future: Box::pin(async move {
                        let result = if supports_parallel {
                            let _guard = lock.read().await;
                            tool.execute(tool_input, tool_context).await
                        } else {
                            let _guard = lock.write().await;
                            tool.execute(tool_input, tool_context).await
                        };
                        tool_execution_record(tool_call_for_task, tool_name, result)
                    }),
                });
            }
            ToolApprovalDecision::Denied { reason } => {
                item.status = TimelineItemStatus::Denied;
                item.updated_at = unix_seconds();
                if let Some(tool) = &mut item.tool {
                    tool.denial_reason = Some(reason.clone());
                    tool.result = Some(format!("Tool execution denied: {reason}"));
                }
                recorder.complete_item(item);
                scheduled.push(ScheduledToolExecution {
                    tool_call: tool_call.clone(),
                    item: initial_items[&timeline_item_id].clone(),
                    future: Box::pin(ready_tool_execution_record(
                        tool_call.clone(),
                        format!("Tool execution denied: {reason}"),
                        TimelineItemStatus::Denied,
                        None,
                        false,
                    )),
                });
            }
        }
    }

    let mut records = Vec::new();
    let futures = scheduled
        .into_iter()
        .map(|scheduled| async move {
            let record = scheduled.future.await;
            (scheduled.tool_call, scheduled.item, record)
        })
        .collect::<Vec<_>>();
    for (_tool_call, mut item, record) in futures::future::join_all(futures).await {
        item.status = record.status;
        item.updated_at = unix_seconds();
        if let Some(tool) = &mut item.tool {
            tool.result = Some(record.display_result.clone());
            tool.exit_code = record.exit_code;
            tool.timed_out = record.timed_out;
        }
        if item.status == TimelineItemStatus::Failed {
            recorder.fail_item(item, record.display_result.clone());
        } else {
            recorder.complete_item(item);
        }
        records.push(record);
    }
    records
}

pub(super) fn namespaced_tool_timeline_item_id(turn_id: &str, tool_call_id: &str) -> String {
    if tool_call_id.starts_with(turn_id) {
        return tool_call_id.to_string();
    }
    format!("{turn_id}-{tool_call_id}")
}

async fn ready_tool_execution_record(
    tool_call: pl_model::ToolCall,
    result: String,
    status: TimelineItemStatus,
    exit_code: Option<i32>,
    timed_out: bool,
) -> ToolExecutionRecord {
    ToolExecutionRecord {
        call_id: tool_call
            .call_id
            .clone()
            .unwrap_or_else(|| tool_call.id.clone()),
        name: tool_call.name.clone(),
        kind: tool_call.kind(),
        arguments: serde_json::to_string(&tool_call.arguments_for_display()).unwrap_or_default(),
        display_result: result.clone(),
        result,
        status,
        exit_code,
        timed_out,
    }
}

fn tool_execution_record(
    tool_call: pl_model::ToolCall,
    tool_name: String,
    result: std::result::Result<ToolOutput, pl_protocol::PureError>,
) -> ToolExecutionRecord {
    let (result, status, exit_code, timed_out) = match result {
        Ok(output) => (
            output.description,
            TimelineItemStatus::Completed,
            output.exit_code,
            output.timed_out,
        ),
        Err(error) => (
            format!("Tool execution error: {error}"),
            TimelineItemStatus::Failed,
            None,
            false,
        ),
    };
    let display_result = display_result_for_tool(&tool_call, &tool_name, &result, status);
    ToolExecutionRecord {
        call_id: tool_call
            .call_id
            .clone()
            .unwrap_or_else(|| tool_call.id.clone()),
        name: tool_name,
        kind: tool_call.kind(),
        arguments: serde_json::to_string(&tool_call.arguments_for_display()).unwrap_or_default(),
        result,
        display_result,
        status,
        exit_code,
        timed_out,
    }
}

fn display_result_for_tool(
    tool_call: &pl_model::ToolCall,
    tool_name: &str,
    result: &str,
    status: TimelineItemStatus,
) -> String {
    if tool_name == "request_user_input" && status == TimelineItemStatus::Completed {
        return redact_user_input_display_result(&tool_call.arguments_for_tool(), result);
    }
    result.to_string()
}

fn redact_user_input_display_result(arguments: &serde_json::Value, result: &str) -> String {
    let secret_ids = arguments
        .get("questions")
        .and_then(serde_json::Value::as_array)
        .map(|questions| {
            questions
                .iter()
                .filter(|question| {
                    question
                        .get("isSecret")
                        .or_else(|| question.get("is_secret"))
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false)
                })
                .filter_map(|question| question.get("id").and_then(serde_json::Value::as_str))
                .map(ToOwned::to_owned)
                .collect::<std::collections::HashSet<_>>()
        })
        .unwrap_or_default();
    if secret_ids.is_empty() {
        return result.to_string();
    }
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(result) else {
        return "[redacted user input]".to_string();
    };
    if let Some(answers) = value
        .get_mut("answers")
        .and_then(serde_json::Value::as_object_mut)
    {
        for id in secret_ids {
            if let Some(answer) = answers.get_mut(&id)
                && let Some(answer_object) = answer.as_object_mut()
            {
                answer_object.insert("answers".to_string(), serde_json::json!(["[redacted]"]));
            }
        }
    }
    serde_json::to_string(&value).unwrap_or_else(|_| "[redacted user input]".to_string())
}

pub(super) fn tool_results_include_recoverable_subagent_capacity(
    tool_results: &[ToolExecutionRecord],
) -> bool {
    tool_results.iter().any(|tool_result| {
        tool_result.status == TimelineItemStatus::Completed
            && matches!(
                tool_result.name.as_str(),
                "spawn_agent" | "wait_agent" | "list_agents"
            )
            && tool_result.result.contains(RECOVERABLE_SUBAGENT_429_MARKER)
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::redact_user_input_display_result;

    #[test]
    fn redacts_secret_user_input_answers_for_timeline_display() {
        let arguments = serde_json::json!({
            "questions": [
                { "id": "api_key", "header": "Key", "question": "API key?", "isSecret": true },
                { "id": "mode", "header": "Mode", "question": "Mode?", "isSecret": false }
            ]
        });
        let result = serde_json::json!({
            "answers": {
                "api_key": { "answers": ["sk-secret"] },
                "mode": { "answers": ["Fast"] }
            }
        })
        .to_string();

        let display = redact_user_input_display_result(&arguments, &result);

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&display).unwrap(),
            serde_json::json!({
                "answers": {
                    "api_key": { "answers": ["[redacted]"] },
                    "mode": { "answers": ["Fast"] }
                }
            })
        );
    }
}
