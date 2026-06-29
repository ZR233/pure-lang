use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::future::BoxFuture;
use futures::stream::{FuturesUnordered, StreamExt};
use pl_model::ToolCallPayload;
use pl_protocol::{PureError, ToolCallKind};
use pl_trace::{AgentEvent, TraceEventKind, TracePartStatus};
use tokio::sync::RwLock;

use crate::permission::{PermissionDecision, decide_tool_permission};
use crate::session::CoreSession;
use crate::tool::{
    RECOVERABLE_SUBAGENT_429_MARKER, SubagentContext, ToolContext, ToolInput, ToolOutput,
    ToolRuntimeEvent, ToolRuntimeLockPolicy, WorkspaceAccess,
};
use crate::turn::{BudgetTracker, ToolApprovalDecision, ToolExecutionMode, TurnOptions};

use super::PureCore;
use super::permission::{approval_request, request_user_approval, requested_workspace_access};
use super::progress::{ProgressEmitter, ProgressVerbosity};
use super::turn_result::{is_cancelled, tool_allowed_in_mode, unix_seconds};

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

#[derive(Debug, Clone)]
pub(super) struct ToolOutputEnvelope {
    pub(super) model_visible_text: String,
    pub(super) display_text: String,
    pub(super) full_output_file: Option<PathBuf>,
    pub(super) exit_code: Option<i32>,
    pub(super) timed_out: bool,
    pub(super) runtime_events: Vec<ToolRuntimeEvent>,
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

fn emit_tool_progress(progress: &mut ProgressEmitter, record: &ToolExecutionRecord) {
    progress.tool_detail(tool_terminal_progress_message(record));
}

fn tool_start_progress_message(name: &str) -> String {
    match name {
        "plan_exit" => "正在提交计划。".to_string(),
        "request_user_input" => "正在等待用户输入。".to_string(),
        "spawn_agent" => "正在创建子代理。".to_string(),
        "wait_agent" => "正在等待子代理。".to_string(),
        "list_agents" => "正在检查子代理状态。".to_string(),
        "send_message" => "正在给子代理发送消息。".to_string(),
        "followup_task" => "正在给子代理追加任务。".to_string(),
        "close_agent" => "正在关闭子代理。".to_string(),
        _ => format!("正在执行工具 `{name}`。"),
    }
}

fn tool_terminal_progress_message(record: &ToolExecutionRecord) -> String {
    let name = &record.name;
    match record.status {
        TracePartStatus::Completed => match name.as_str() {
            "plan_exit" => "计划已生成，等待确认。".to_string(),
            "request_user_input" => "用户输入已收到。".to_string(),
            "spawn_agent" => "子代理已创建。".to_string(),
            "wait_agent" if record.timed_out => "等待子代理已超时。".to_string(),
            "wait_agent" => "子代理等待已结束。".to_string(),
            "list_agents" => "子代理状态已更新。".to_string(),
            "send_message" => "子代理消息已发送。".to_string(),
            "followup_task" => "子代理任务已追加。".to_string(),
            "close_agent" => "子代理已关闭。".to_string(),
            _ => format!("工具 `{name}` 已完成。"),
        },
        TracePartStatus::Denied => format!("工具 `{name}` 已拒绝。"),
        TracePartStatus::Failed => match name.as_str() {
            "plan_exit" => "计划提交失败。".to_string(),
            "request_user_input" => "用户输入请求失败。".to_string(),
            "spawn_agent" | "wait_agent" | "list_agents" | "send_message" | "followup_task"
            | "close_agent" => format!("子代理工具 `{name}` 执行失败。"),
            _ => format!("工具 `{name}` 执行失败。"),
        },
        TracePartStatus::Interrupted => format!("工具 `{name}` 已中断。"),
        TracePartStatus::BudgetLimited => format!("工具 `{name}` 因预算限制停止。"),
        TracePartStatus::Started
        | TracePartStatus::Streaming
        | TracePartStatus::AwaitingApproval
        | TracePartStatus::Approved
        | TracePartStatus::Running => format!("工具 `{name}` 已结束。"),
    }
}

fn emit_tool_snapshot(
    recorder: &mut crate::trace::TraceRecorder,
    item: &mut pl_trace::TracePart,
    status: TracePartStatus,
) {
    item.status = status;
    item.updated_at = unix_seconds();
    recorder.update_item_snapshot(item.clone());
}

fn finalize_tool_item(
    recorder: &mut crate::trace::TraceRecorder,
    mut item: pl_trace::TracePart,
    record: &ToolExecutionRecord,
) {
    item.status = record.status;
    item.updated_at = unix_seconds();
    if let Some(tool) = &mut item.tool {
        tool.result = Some(record.display_result.clone());
        tool.exit_code = record.exit_code;
        tool.timed_out = record.timed_out;
    }
    if let Some(revision) = record.revision {
        item.revision = item.revision.max(revision);
    }
    let status = item.status;
    match status {
        TracePartStatus::Failed
        | TracePartStatus::Denied
        | TracePartStatus::Interrupted
        | TracePartStatus::BudgetLimited => {
            recorder.fail_item(item, record.display_result.clone());
        }
        TracePartStatus::Started
        | TracePartStatus::Streaming
        | TracePartStatus::AwaitingApproval
        | TracePartStatus::Approved
        | TracePartStatus::Running
        | TracePartStatus::Completed => recorder.complete_item(item),
    }
    if status == TracePartStatus::Completed {
        for event in &record.runtime_events {
            match event {
                ToolRuntimeEvent::SkillActivated { activation } => {
                    recorder.record_trace_only(TraceEventKind::SkillActivated {
                        activation: activation.clone(),
                    });
                    recorder.broadcast(AgentEvent::SkillActivated {
                        activation: activation.clone(),
                    });
                }
                ToolRuntimeEvent::ToolResultRevision { .. } => {}
            }
        }
    }
}

async fn ready_tool_execution_record(
    tool_call: pl_model::ToolCall,
    error: ToolExecutionError,
    status: TracePartStatus,
    exit_code: Option<i32>,
    timed_out: bool,
) -> Result<ToolExecutionRecord, ToolExecutionError> {
    match error {
        ToolExecutionError::RespondToModel(message) => Ok(tool_execution_record_from_envelope(
            tool_call.clone(),
            tool_call.name.clone(),
            ToolOutputEnvelope {
                model_visible_text: message.clone(),
                display_text: message,
                full_output_file: None,
                exit_code,
                timed_out,
                runtime_events: Vec::new(),
            },
            status,
        )),
        ToolExecutionError::Fatal(message) => Err(ToolExecutionError::Fatal(message)),
    }
}

fn tool_execution_record(
    tool_call: pl_model::ToolCall,
    tool_name: String,
    result: std::result::Result<ToolOutput, PureError>,
) -> Result<ToolExecutionRecord, ToolExecutionError> {
    let (envelope, status) = match result {
        Ok(output) => (
            ToolOutputEnvelope {
                model_visible_text: output.description.clone(),
                display_text: output.description,
                full_output_file: (!output.output_file.as_os_str().is_empty())
                    .then_some(output.output_file),
                exit_code: output.exit_code,
                timed_out: output.timed_out,
                runtime_events: output.runtime_events,
            },
            TracePartStatus::Completed,
        ),
        Err(error) => {
            return Err(ToolExecutionError::RespondToModel(format!(
                "Tool execution error: {error}"
            )));
        }
    };
    Ok(tool_execution_record_from_envelope(
        tool_call, tool_name, envelope, status,
    ))
}

fn tool_execution_record_from_envelope(
    tool_call: pl_model::ToolCall,
    tool_name: String,
    envelope: ToolOutputEnvelope,
    status: TracePartStatus,
) -> ToolExecutionRecord {
    let ToolOutputEnvelope {
        model_visible_text,
        display_text,
        full_output_file: _full_output_file,
        exit_code,
        timed_out,
        runtime_events,
    } = envelope;
    let revision = runtime_events.iter().find_map(|event| match event {
        ToolRuntimeEvent::ToolResultRevision { revision } => Some(*revision),
        ToolRuntimeEvent::SkillActivated { .. } => None,
    });
    let display_result = display_result_for_tool(&tool_call, &tool_name, &display_text, status);
    ToolExecutionRecord {
        id: tool_call.id.clone(),
        call_id: tool_call.call_id.clone(),
        name: tool_name,
        kind: tool_call.kind(),
        arguments: serde_json::to_string(&tool_call.arguments_for_display()).unwrap_or_default(),
        result: model_visible_text,
        display_result,
        status,
        exit_code,
        timed_out,
        revision,
        runtime_events,
    }
}

fn interrupted_tool_execution_record(tool_call: pl_model::ToolCall) -> ToolExecutionRecord {
    tool_execution_record_from_envelope(
        tool_call.clone(),
        tool_call.name.clone(),
        ToolOutputEnvelope {
            model_visible_text: "Tool execution interrupted".to_string(),
            display_text: "Tool execution interrupted".to_string(),
            full_output_file: None,
            exit_code: None,
            timed_out: false,
            runtime_events: Vec::new(),
        },
        TracePartStatus::Interrupted,
    )
}

fn respond_to_model_tool_execution_record(
    tool_call: pl_model::ToolCall,
    message: String,
) -> ToolExecutionRecord {
    tool_execution_record_from_envelope(
        tool_call.clone(),
        tool_call.name.clone(),
        ToolOutputEnvelope {
            model_visible_text: message.clone(),
            display_text: message,
            full_output_file: None,
            exit_code: None,
            timed_out: false,
            runtime_events: Vec::new(),
        },
        TracePartStatus::Failed,
    )
}

fn display_result_for_tool(
    tool_call: &pl_model::ToolCall,
    tool_name: &str,
    result: &str,
    status: TracePartStatus,
) -> String {
    if tool_name == "request_user_input" && status == TracePartStatus::Completed {
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
        tool_result.status == TracePartStatus::Completed
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

    use super::{
        ToolExecutionRecord, redact_user_input_display_result, tool_start_progress_message,
        tool_terminal_progress_message,
    };
    use pl_protocol::ToolCallKind;
    use pl_trace::TracePartStatus;

    fn completed_record(name: &str) -> ToolExecutionRecord {
        ToolExecutionRecord {
            id: "item-1".to_string(),
            call_id: None,
            name: name.to_string(),
            kind: ToolCallKind::Function,
            result: String::new(),
            display_result: String::new(),
            arguments: "{}".to_string(),
            status: TracePartStatus::Completed,
            exit_code: None,
            timed_out: false,
            revision: None,
            runtime_events: Vec::new(),
        }
    }

    #[test]
    fn redacts_secret_user_input_answers_for_display() {
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

    #[test]
    fn progress_messages_describe_plan_and_subagent_lifecycle() {
        assert_eq!(tool_start_progress_message("plan_exit"), "正在提交计划。");
        assert_eq!(
            tool_terminal_progress_message(&completed_record("plan_exit")),
            "计划已生成，等待确认。"
        );
        assert_eq!(
            tool_start_progress_message("spawn_agent"),
            "正在创建子代理。"
        );
        assert_eq!(
            tool_terminal_progress_message(&completed_record("spawn_agent")),
            "子代理已创建。"
        );
        assert_eq!(
            tool_start_progress_message("wait_agent"),
            "正在等待子代理。"
        );
        assert_eq!(
            tool_terminal_progress_message(&completed_record("wait_agent")),
            "子代理等待已结束。"
        );
        assert_eq!(
            tool_start_progress_message("read_file"),
            "正在执行工具 `read_file`。"
        );
        assert_eq!(
            tool_terminal_progress_message(&completed_record("read_file")),
            "工具 `read_file` 已完成。"
        );
    }
}
