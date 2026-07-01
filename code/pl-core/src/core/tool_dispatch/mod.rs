use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use futures::future::BoxFuture;
use futures::stream::{FuturesUnordered, StreamExt};
use pl_model::ToolCallPayload;
use pl_protocol::ToolCallKind;
use pl_trace::TracePartStatus;
use tokio::sync::RwLock;

use crate::permission::{PermissionDecision, decide_tool_permission};
use crate::session::CoreSession;
use crate::tool::{
    SubagentContext, ToolContext, ToolInput, ToolRuntimeEvent, ToolRuntimeLockPolicy,
    WorkspaceAccess,
};
use crate::turn::{BudgetTracker, ToolApprovalDecision, ToolExecutionMode, TurnOptions};

use super::PureCore;
use super::permission::{approval_request, request_user_approval, requested_workspace_access};
use super::progress::{ProgressEmitter, ProgressVerbosity};
use super::turn_result::{is_cancelled, tool_allowed_in_mode, unix_seconds};

mod display;
mod progress_messages;
pub(super) mod records;

use progress_messages::{emit_tool_progress, tool_start_progress_message};
use records::{
    emit_tool_snapshot, finalize_tool_item, interrupted_tool_execution_record,
    ready_tool_execution_record, respond_to_model_tool_execution_record, tool_execution_record,
};

pub(super) struct ToolExecutionRecord {
    pub(super) id: String,
    pub(super) call_id: Option<String>,
    pub(super) name: String,
    pub(super) kind: ToolCallKind,
    pub(super) result: String,
    pub(super) display_result: String,
    pub(super) arguments: String,
    pub(super) status: TracePartStatus,
    pub(super) exit_code: Option<i32>,
    pub(super) timed_out: bool,
    pub(super) revision: Option<u64>,
    pub(super) runtime_events: Vec<ToolRuntimeEvent>,
}

pub(super) struct ScheduledToolExecution<'a> {
    pub(super) tool_call: pl_model::ToolCall,
    pub(super) item: pl_trace::TracePart,
    pub(super) future: BoxFuture<'a, Result<ToolExecutionRecord, ToolExecutionError>>,
}

#[derive(Debug, Clone)]
pub(super) struct ToolInvocation {
    pub(super) name: String,
    pub(super) runtime_tool_call_id: String,
    pub(super) provider_item_id: String,
    pub(super) call_id: Option<String>,
    pub(super) payload: ToolPayload,
    pub(super) context: ToolContext,
}

#[derive(Debug, Clone)]
pub(super) enum ToolPayload {
    Function(serde_json::Value),
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ToolExecutionError {
    RespondToModel(String),
    Fatal(String),
}

pub(super) struct ToolExecutionContext<'a> {
    pub(super) core: &'a PureCore,
    pub(super) options: &'a TurnOptions,
    pub(super) mode: crate::turn::CompileMode,
    pub(super) session_id: &'a str,
    pub(super) workspace_root: &'a Path,
    pub(super) workspace_instructions: Option<String>,
    pub(super) instruction_snapshot: Option<crate::instruction::InstructionSnapshot>,
    pub(super) active_subagent: Option<SubagentContext>,
    pub(super) agent_control: crate::AgentControl,
    pub(super) parent_session: Arc<CoreSession>,
}

pub(super) async fn execute_tool_calls(
    tool_calls: &[pl_model::ToolCall],
    budget_tracker: &mut BudgetTracker,
    recorder: &mut crate::trace::TraceRecorder,
    context: ToolExecutionContext<'_>,
) -> Result<Vec<ToolExecutionRecord>, ToolExecutionError> {
    let mut scheduled = Vec::new();
    let runtime_lock = Arc::new(RwLock::new(()));
    let mut progress = ProgressEmitter::new_scoped(
        recorder.sender().clone(),
        context.session_id.to_string(),
        format!("{}:tool-progress", context.session_id),
        ProgressVerbosity::from_env(),
    );

    for tool_call in tool_calls {
        if is_cancelled(context.options) {
            break;
        }
        if tool_call.name.is_empty() {
            return Err(ToolExecutionError::Fatal(
                "tool call missing tool name".to_string(),
            ));
        }
        let trace_part_id = tool_trace_part_id(context.session_id, tool_call);
        let mut item = recorder
            .latest_tool_trace_part(
                &trace_part_id,
                tool_call.call_id.as_deref(),
                Some(tool_call.id.as_str()),
            )
            .unwrap_or_else(|| {
                let item = recorder.tool_item(
                    context.session_id,
                    &trace_part_id,
                    tool_call.name.clone(),
                    tool_call.payload_text(),
                    tool_call.call_id.clone(),
                    Some(tool_call.id.clone()),
                );
                recorder.start_item(item.clone());
                item
            });
        if let Some(tool) = &mut item.tool {
            tool.tool_call_id = trace_part_id.clone();
            tool.call_id = tool_call.call_id.clone();
            tool.provider_item_id = Some(tool_call.id.clone());
            tool.name = tool_call.name.clone();
            tool.arguments = tool_call.payload_text();
        }
        budget_tracker.record_tool_call(&tool_call.name);

        if !tool_allowed_in_mode(context.mode, &tool_call.name) {
            let mode = context.mode.label();
            let name = &tool_call.name;
            let message = format!("Tool disabled in {mode} mode: {name}");
            if let Some(tool) = &mut item.tool {
                tool.denial_reason = Some(message.clone());
            }
            emit_tool_snapshot(recorder, &mut item, TracePartStatus::Denied);
            scheduled.push(ScheduledToolExecution {
                tool_call: tool_call.clone(),
                item,
                future: Box::pin(ready_tool_execution_record(
                    tool_call.clone(),
                    ToolExecutionError::RespondToModel(message),
                    TracePartStatus::Denied,
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
            emit_tool_snapshot(recorder, &mut item, TracePartStatus::Failed);
            scheduled.push(ScheduledToolExecution {
                tool_call: tool_call.clone(),
                item,
                future: Box::pin(ready_tool_execution_record(
                    tool_call.clone(),
                    ToolExecutionError::RespondToModel(format!("Unknown tool: {}", tool_call.name)),
                    TracePartStatus::Failed,
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
        let runtime_lock_policy = if matches!(
            context.options.tool_execution_mode,
            ToolExecutionMode::ModelDefault | ToolExecutionMode::Parallel
        ) {
            tool.runtime_lock_policy()
        } else {
            ToolRuntimeLockPolicy::Exclusive
        };
        let tool_context = ToolContext {
            event_tx: recorder.sender().clone(),
            options: context.options.clone(),
            workspace_access: WorkspaceAccess::WorkspaceOnly,
            mode: context.mode,
            workspace_root: context.workspace_root.to_path_buf(),
            workspace_instructions: context.workspace_instructions.clone(),
            instruction_snapshot: context.instruction_snapshot.clone(),
            active_subagent: context.active_subagent.clone(),
            agent_control: context.agent_control.clone(),
            lsp_runtime: context.core.lsp_runtime.clone(),
            parent_session: context.parent_session.clone(),
        };
        let mut approval_request = approval_request(tool_call, &tool_context);
        approval_request.id = trace_part_id.clone();
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
                emit_tool_snapshot(recorder, &mut item, TracePartStatus::AwaitingApproval);
                progress.tool_detail(format!("工具 `{}` 正在等待授权。", tool_call.name));
                let decision =
                    request_user_approval(context.options, &approval_request, context.session_id)
                        .await;
                if matches!(decision, ToolApprovalDecision::Approved) {
                    execution_workspace_access = workspace_access;
                }
                match &decision {
                    ToolApprovalDecision::Approved => {}
                    ToolApprovalDecision::Denied { reason } => {
                        if let Some(tool) = &mut item.tool {
                            tool.denial_reason = Some(reason.clone());
                        }
                    }
                }
                decision
            }
            PermissionDecision::NeedsAiReview { workspace_access } => {
                emit_tool_snapshot(recorder, &mut item, TracePartStatus::AwaitingApproval);
                progress.tool_detail(format!("正在审查工具 `{}`。", tool_call.name));
                let mut review_context = tool_context.clone();
                review_context.workspace_access = workspace_access;
                let decision = context
                    .core
                    .review_tool_call_with_ai(&approval_request, &review_context)
                    .await;
                if matches!(decision, ToolApprovalDecision::Approved) {
                    execution_workspace_access = workspace_access;
                }
                match &decision {
                    ToolApprovalDecision::Approved => {}
                    ToolApprovalDecision::Denied { reason } => {
                        if let Some(tool) = &mut item.tool {
                            tool.denial_reason = Some(reason.clone());
                        }
                    }
                }
                decision
            }
            PermissionDecision::Denied { reason } => {
                if let Some(tool) = &mut item.tool {
                    tool.denial_reason = Some(reason.clone());
                }
                ToolApprovalDecision::Denied { reason }
            }
        };
        if is_cancelled(context.options) {
            emit_tool_snapshot(recorder, &mut item, TracePartStatus::Interrupted);
            scheduled.push(ScheduledToolExecution {
                tool_call: tool_call.clone(),
                item,
                future: Box::pin(ready_tool_execution_record(
                    tool_call.clone(),
                    ToolExecutionError::RespondToModel("Tool execution interrupted".to_string()),
                    TracePartStatus::Interrupted,
                    None,
                    false,
                )),
            });
            break;
        }

        match decision {
            ToolApprovalDecision::Approved => {
                let mut tool_context = tool_context;
                tool_context.workspace_access = execution_workspace_access;
                emit_tool_snapshot(recorder, &mut item, TracePartStatus::Approved);
                emit_tool_snapshot(recorder, &mut item, TracePartStatus::Running);
                progress.tool_detail(tool_start_progress_message(&tool_call.name));
                let invocation =
                    ToolInvocation::from_tool_call(tool_call, trace_part_id.clone(), tool_context);
                let _runtime_identity = (
                    invocation.provider_item_id.as_str(),
                    invocation.call_id.as_deref(),
                );
                let _display_arguments = invocation.payload.arguments_for_display();
                let tool_input = ToolInput {
                    arguments: invocation.payload.arguments_for_tool(),
                    session_id: context.session_id.to_string(),
                    tool_id: invocation.runtime_tool_call_id.clone(),
                    revision_base: item.revision,
                };
                let lock = runtime_lock.clone();
                let tool_name = invocation.name.clone();
                let tool_call_for_task = tool_call.clone();
                let tool_context = invocation.context;
                scheduled.push(ScheduledToolExecution {
                    tool_call: tool_call.clone(),
                    item,
                    future: Box::pin(async move {
                        let result = match runtime_lock_policy {
                            ToolRuntimeLockPolicy::Shared if supports_parallel => {
                                let _guard = lock.read().await;
                                tool.execute(tool_input, tool_context).await
                            }
                            ToolRuntimeLockPolicy::None => {
                                tool.execute(tool_input, tool_context).await
                            }
                            ToolRuntimeLockPolicy::Exclusive | ToolRuntimeLockPolicy::Shared => {
                                let _guard = lock.write().await;
                                tool.execute(tool_input, tool_context).await
                            }
                        };
                        tool_execution_record(tool_call_for_task, tool_name, result)
                    }),
                });
            }
            ToolApprovalDecision::Denied { reason } => {
                if let Some(tool) = &mut item.tool {
                    tool.denial_reason = Some(reason.clone());
                }
                emit_tool_snapshot(recorder, &mut item, TracePartStatus::Denied);
                scheduled.push(ScheduledToolExecution {
                    tool_call: tool_call.clone(),
                    item,
                    future: Box::pin(ready_tool_execution_record(
                        tool_call.clone(),
                        ToolExecutionError::RespondToModel(format!(
                            "Tool execution denied: {reason}"
                        )),
                        TracePartStatus::Denied,
                        None,
                        false,
                    )),
                });
            }
        }
    }

    collect_scheduled_tools(scheduled, recorder, context.options, &mut progress).await
}

pub(super) fn namespaced_tool_trace_part_id(turn_id: &str, tool_call_id: &str) -> String {
    if tool_call_id.starts_with(turn_id) {
        return tool_call_id.to_string();
    }
    format!("{turn_id}-{tool_call_id}")
}

fn tool_trace_part_id(turn_id: &str, tool_call: &pl_model::ToolCall) -> String {
    let tool_call_id = if tool_call.id.is_empty() {
        tool_call.call_id.as_deref().unwrap_or("tool_call")
    } else {
        &tool_call.id
    };
    namespaced_tool_trace_part_id(turn_id, tool_call_id)
}

impl ToolInvocation {
    fn from_tool_call(
        tool_call: &pl_model::ToolCall,
        runtime_tool_call_id: String,
        context: ToolContext,
    ) -> Self {
        Self {
            name: tool_call.name.clone(),
            runtime_tool_call_id,
            provider_item_id: tool_call.id.clone(),
            call_id: tool_call.call_id.clone(),
            payload: ToolPayload::from_tool_call_payload(&tool_call.payload),
            context,
        }
    }
}

impl ToolPayload {
    fn from_tool_call_payload(payload: &ToolCallPayload) -> Self {
        match payload {
            ToolCallPayload::Function { arguments } => Self::Function(arguments.clone()),
            ToolCallPayload::Custom { input } => Self::Custom(input.clone()),
        }
    }

    fn arguments_for_tool(&self) -> serde_json::Value {
        match self {
            Self::Function(arguments) => arguments.clone(),
            Self::Custom(input) => serde_json::json!({
                "input": input,
                "patch": input,
            }),
        }
    }

    fn arguments_for_display(&self) -> serde_json::Value {
        match self {
            Self::Function(arguments) => arguments.clone(),
            Self::Custom(input) => serde_json::json!({ "input": input }),
        }
    }
}

async fn collect_scheduled_tools(
    scheduled: Vec<ScheduledToolExecution<'_>>,
    recorder: &mut crate::trace::TraceRecorder,
    options: &TurnOptions,
    progress: &mut ProgressEmitter,
) -> Result<Vec<ToolExecutionRecord>, ToolExecutionError> {
    let mut pending = BTreeMap::new();
    let mut futures = FuturesUnordered::new();
    let scheduled_count = scheduled.len();

    for (index, scheduled) in scheduled.into_iter().enumerate() {
        pending.insert(index, (scheduled.tool_call.clone(), scheduled.item.clone()));
        futures.push(async move {
            let record = scheduled.future.await;
            (index, scheduled.tool_call, scheduled.item, record)
        });
    }

    let mut ordered_records = std::iter::repeat_with(|| None)
        .take(scheduled_count)
        .collect::<Vec<Option<ToolExecutionRecord>>>();

    loop {
        if futures.is_empty() {
            break;
        }
        let next = match &options.cancellation_token {
            Some(token) => {
                tokio::select! {
                    next = futures.next() => next,
                    _ = token.cancelled() => {
                        for (index, (tool_call, item)) in pending {
                            let record = interrupted_tool_execution_record(tool_call);
                            finalize_tool_item(recorder, item, &record);
                            emit_tool_progress(progress, &record);
                            ordered_records[index] = Some(record);
                        }
                        return Ok(ordered_records.into_iter().flatten().collect());
                    }
                }
            }
            None => futures.next().await,
        };

        let Some((index, tool_call, item, record)) = next else {
            break;
        };
        pending.remove(&index);
        let record = match record {
            Ok(record) => record,
            Err(ToolExecutionError::RespondToModel(message)) => {
                respond_to_model_tool_execution_record(tool_call, message)
            }
            Err(ToolExecutionError::Fatal(message)) => {
                return Err(ToolExecutionError::Fatal(message));
            }
        };
        finalize_tool_item(recorder, item, &record);
        emit_tool_progress(progress, &record);
        ordered_records[index] = Some(record);
    }

    Ok(ordered_records.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests;
