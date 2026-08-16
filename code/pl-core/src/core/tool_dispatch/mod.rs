use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Instant;

use futures::FutureExt;
use futures::future::BoxFuture;
use futures::stream::{FuturesUnordered, StreamExt};
use pl_model::ToolCallPayload;
use pl_protocol::{InferenceOrchestrationMetrics, ToolCallKind};
use pl_trace::TracePartStatus;
use tokio::sync::RwLock;

use crate::permission::{PermissionDecision, decide_tool_permission};
use crate::session::AgentSession;
use crate::tool::cache::{ToolCachePolicy, TurnToolCacheHandle};
use crate::tool::{
    AgentWorkspace, SubagentContext, ToolBudgetTiming, ToolContext, ToolInput, ToolRuntimeEvent,
    ToolRuntimeLockPolicy, TurnToolLease, WorkspaceAccess,
};
use crate::turn::{BudgetTracker, ToolApprovalDecision, ToolExecutionMode, TurnOptions};

use super::TurnEngine;
use super::permission::{approval_request, request_user_approval, requested_workspace_access};
use super::progress::{ProgressEmitter, ProgressVerbosity};
use super::turn_result::{is_cancelled, unix_seconds};

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
    pub(super) execution_millis: u64,
}

pub(super) struct ScheduledToolExecution<'a> {
    pub(super) tool_call: pl_model::ToolCall,
    pub(super) item: pl_trace::TracePart,
    pub(super) future: BoxFuture<'a, Result<ToolExecutionRecord, ToolExecutionError>>,
    pub(super) budget_timing: ToolBudgetTiming,
    pub(super) parallel_candidate: bool,
    pub(super) duplicate_suppressed: bool,
}

pub(super) struct ToolExecutionBatch {
    pub(super) records: Vec<ToolExecutionRecord>,
    pub(super) orchestration: InferenceOrchestrationMetrics,
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
    pub(super) core: &'a TurnEngine,
    /// 本 Turn 冻结的工具 lease；调度只使用 lease 条目。
    pub(super) lease: TurnToolLease,
    pub(super) options: &'a TurnOptions,
    pub(super) session_id: &'a str,
    pub(super) workspace: AgentWorkspace,
    pub(super) workspace_instructions: Option<String>,
    pub(super) instruction_snapshot: Option<crate::instruction::InstructionSnapshot>,
    pub(super) active_subagent: Option<SubagentContext>,
    pub(super) parent_session: Arc<AgentSession>,
    pub(super) working_set: crate::TurnWorkingSetHandle,
    pub(super) tool_cache: TurnToolCacheHandle,
}

#[cfg(test)]
pub(super) async fn execute_tool_calls(
    tool_calls: &[pl_model::ToolCall],
    budget_tracker: &mut BudgetTracker,
    recorder: &mut crate::trace::TraceRecorder,
    context: ToolExecutionContext<'_>,
) -> Result<Vec<ToolExecutionRecord>, ToolExecutionError> {
    execute_tool_call_batch(tool_calls, budget_tracker, recorder, context)
        .await
        .map(|batch| batch.records)
}

pub(super) async fn execute_tool_call_batch(
    tool_calls: &[pl_model::ToolCall],
    budget_tracker: &mut BudgetTracker,
    recorder: &mut crate::trace::TraceRecorder,
    context: ToolExecutionContext<'_>,
) -> Result<ToolExecutionBatch, ToolExecutionError> {
    let mut scheduled = Vec::new();
    let mut scheduled_exact_once_calls = HashMap::<(String, String), String>::new();
    let runtime_lock = Arc::new(RwLock::new(()));
    let tool_cache_snapshot = context.tool_cache.snapshot();
    let sid = &context.session_id;
    let mut progress = ProgressEmitter::new_scoped(
        recorder.sender().clone(),
        context.session_id.to_string(),
        format!("{sid}:tool-progress"),
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
                    Some(tool_call.stable_call_id().to_string()),
                    Some(tool_call.id.clone()),
                );
                recorder.start_item(item.clone());
                item
            });
        if let Some(tool) = &mut item.tool {
            tool.tool_call_id = trace_part_id.clone();
            tool.call_id = Some(tool_call.stable_call_id().to_string());
            tool.provider_item_id = Some(tool_call.id.clone());
            tool.name = tool_call.name.clone();
            tool.arguments = tool_call.payload_text();
        }
        budget_tracker.record_tool_call(&tool_call.name);

        if let Some(message) = tool_call.invalid_arguments_message() {
            emit_tool_snapshot(recorder, &mut item, TracePartStatus::Failed);
            scheduled.push(ScheduledToolExecution {
                tool_call: tool_call.clone(),
                item,
                future: ready_tool_execution_record(
                    tool_call.clone(),
                    ToolExecutionError::RespondToModel(message),
                    TracePartStatus::Failed,
                    None,
                    false,
                )
                .boxed(),
                budget_timing: ToolBudgetTiming::Count,
                parallel_candidate: false,
                duplicate_suppressed: false,
            });
            continue;
        }

        let registered_tool = context.lease.entry(&tool_call.name);
        let effect = registered_tool.and_then(|entry| entry.tool().effect());
        let allowed = context
            .options
            .execution_policy
            .as_ref()
            .is_none_or(|policy| policy.allows_tool(&tool_call.name, effect));
        if !allowed {
            let name = &tool_call.name;
            let message = format!("Tool disabled by execution policy: {name}");
            if let Some(tool) = &mut item.tool {
                tool.denial_reason = Some(message.clone());
            }
            emit_tool_snapshot(recorder, &mut item, TracePartStatus::Denied);
            scheduled.push(ScheduledToolExecution {
                tool_call: tool_call.clone(),
                item,
                future: ready_tool_execution_record(
                    tool_call.clone(),
                    ToolExecutionError::RespondToModel(message),
                    TracePartStatus::Denied,
                    None,
                    false,
                )
                .boxed(),
                budget_timing: ToolBudgetTiming::Count,
                parallel_candidate: false,
                duplicate_suppressed: false,
            });
            continue;
        }
        let Some(tool) = registered_tool else {
            let available: Vec<&str> = context.lease.names();
            tracing::warn!(
                tool = %tool_call.name,
                available = ?available,
                "model requested an unknown tool"
            );
            emit_tool_snapshot(recorder, &mut item, TracePartStatus::Failed);
            let name = &tool_call.name;
            scheduled.push(ScheduledToolExecution {
                tool_call: tool_call.clone(),
                item,
                future: ready_tool_execution_record(
                    tool_call.clone(),
                    ToolExecutionError::RespondToModel(format!("Unknown tool: {name}")),
                    TracePartStatus::Failed,
                    None,
                    false,
                )
                .boxed(),
                budget_timing: ToolBudgetTiming::Count,
                parallel_candidate: false,
                duplicate_suppressed: false,
            });
            continue;
        };

        let tool = tool.tool();
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
        let parallel_candidate = matches!(runtime_lock_policy, ToolRuntimeLockPolicy::None)
            || matches!(runtime_lock_policy, ToolRuntimeLockPolicy::Shared) && supports_parallel;
        let tool_context = ToolContext {
            event_tx: recorder.sender().clone(),
            options: context.options.clone(),
            workspace_access: WorkspaceAccess::WorkspaceOnly,
            workspace: context.workspace.clone(),
            workspace_instructions: context.workspace_instructions.clone(),
            instruction_snapshot: context.instruction_snapshot.clone(),
            provider_call_id: Some(tool_call.stable_call_id().to_string()),
            active_subagent: context.active_subagent.clone(),
            lsp_runtime: context.core.lsp_runtime.clone(),
            parent_session: context.parent_session.clone(),
            working_set: context.working_set.clone(),
            tool_cache: context.tool_cache.clone(),
        };
        let mut approval_request = approval_request(tool_call, &tool_context);
        approval_request.id = trace_part_id.clone();
        let requested_access = requested_workspace_access(tool_call, context.workspace.root());
        let mut execution_workspace_access = WorkspaceAccess::WorkspaceOnly;
        let decision =
            match decide_tool_permission(context.options, &approval_request, requested_access) {
                PermissionDecision::Approved { workspace_access } => {
                    execution_workspace_access = workspace_access;
                    ToolApprovalDecision::Approved
                }
                PermissionDecision::NeedsUserApproval { workspace_access } => {
                    emit_tool_snapshot(recorder, &mut item, TracePartStatus::AwaitingApproval);
                    let name = &tool_call.name;
                    progress.tool_detail(format!("工具 `{name}` 正在等待授权。"));
                    let decision = request_user_approval(
                        context.options,
                        &approval_request,
                        context.session_id,
                    )
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
                    let name = &tool_call.name;
                    progress.tool_detail(format!("正在审查工具 `{name}`。"));
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
            };
        if is_cancelled(context.options) {
            emit_tool_snapshot(recorder, &mut item, TracePartStatus::Interrupted);
            scheduled.push(ScheduledToolExecution {
                tool_call: tool_call.clone(),
                item,
                future: ready_tool_execution_record(
                    tool_call.clone(),
                    ToolExecutionError::RespondToModel("Tool execution interrupted".to_string()),
                    TracePartStatus::Interrupted,
                    None,
                    false,
                )
                .boxed(),
                budget_timing: ToolBudgetTiming::Count,
                parallel_candidate: false,
                duplicate_suppressed: false,
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
                let cache_policy = tool.cache_policy(&tool_input.arguments);
                let invalidates_cache = tool.invalidates_cache(&tool_input.arguments);
                let lock = runtime_lock.clone();
                let tool_name = invocation.name.clone();
                let tool_call_for_task = tool_call.clone();
                let tool_context = invocation.context;
                let cache = context.tool_cache.clone();
                let cache_snapshot = tool_cache_snapshot.clone();
                let cache_arguments = tool_input.arguments.clone();
                let cache_workspace_root = context.workspace.root().to_path_buf();
                let cache_call_id = tool_call.stable_call_id().to_string();
                let tool_effect = effect;
                let budget_timing = tool.budget_timing();
                let suppress_exact_arguments = cache_policy != ToolCachePolicy::Never;
                if suppress_exact_arguments {
                    let argument_hash = crate::working_set::canonical_content_hash(
                        crate::working_set::canonical_json_string(&cache_arguments).as_bytes(),
                    );
                    let dedupe_key = (tool_name.clone(), argument_hash.clone());
                    if let Some(original_call_id) =
                        scheduled_exact_once_calls.get(&dedupe_key).cloned()
                    {
                        let duplicate_call_id = cache_call_id;
                        tracing::info!(
                            target: "pl_core::tool_metrics",
                            tool = tool_name,
                            original_call_id,
                            duplicate_call_id,
                            argument_hash,
                            duplicate_suppressed = true,
                            "suppressed identical tool call in one provider response"
                        );
                        let receipt = serde_json::json!({
                            "status": "duplicateSuppressed",
                            "reusedFromCallId": original_call_id,
                            "argumentHash": argument_hash,
                            "scope": "providerResponse",
                        })
                        .to_string();
                        scheduled.push(ScheduledToolExecution {
                            tool_call: tool_call.clone(),
                            item,
                            future: ready_tool_execution_record(
                                tool_call.clone(),
                                ToolExecutionError::RespondToModel(receipt),
                                TracePartStatus::Completed,
                                Some(0),
                                false,
                            )
                            .boxed(),
                            budget_timing,
                            parallel_candidate: false,
                            duplicate_suppressed: true,
                        });
                        continue;
                    }
                    scheduled_exact_once_calls.insert(dedupe_key, cache_call_id.clone());
                }
                scheduled.push(ScheduledToolExecution {
                    tool_call: tool_call.clone(),
                    item,
                    future: async move {
                        let execute = || {
                            cache_snapshot.execute_or_reuse(
                                &tool_name,
                                &cache_arguments,
                                &cache_workspace_root,
                                cache_policy,
                                cache_call_id,
                                || tool.execute(tool_input, tool_context),
                            )
                        };
                        let (result, execution_elapsed) = match runtime_lock_policy {
                            ToolRuntimeLockPolicy::Shared if supports_parallel => {
                                let _guard = lock.read().await;
                                let started_at = Instant::now();
                                let result = execute().await;
                                (result, started_at.elapsed())
                            }
                            ToolRuntimeLockPolicy::None => {
                                let started_at = Instant::now();
                                let result = execute().await;
                                (result, started_at.elapsed())
                            }
                            ToolRuntimeLockPolicy::Exclusive | ToolRuntimeLockPolicy::Shared => {
                                let _guard = lock.write().await;
                                let started_at = Instant::now();
                                let result = execute().await;
                                (result, started_at.elapsed())
                            }
                        };
                        if invalidates_cache && result.is_ok() {
                            cache.invalidate_tool(&tool_name);
                        }
                        cache.record_effect(tool_effect, result.is_ok());
                        let mut record =
                            tool_execution_record(tool_call_for_task, tool_name, result)?;
                        record.execution_millis = execution_elapsed.as_millis() as u64;
                        Ok(record)
                    }
                    .boxed(),
                    budget_timing,
                    parallel_candidate,
                    duplicate_suppressed: false,
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
                    future: ready_tool_execution_record(
                        tool_call.clone(),
                        ToolExecutionError::RespondToModel(format!(
                            "Tool execution denied: {reason}"
                        )),
                        TracePartStatus::Denied,
                        None,
                        false,
                    )
                    .boxed(),
                    budget_timing: ToolBudgetTiming::Count,
                    parallel_candidate: false,
                    duplicate_suppressed: false,
                });
            }
        }
    }

    let pause_started_at = matches!(
        scheduled.as_slice(),
        [ScheduledToolExecution {
            budget_timing: ToolBudgetTiming::PauseWhenOnlyScheduledTool,
            ..
        }]
    )
    .then(Instant::now);
    let result = collect_scheduled_tools(scheduled, recorder, context.options, &mut progress).await;
    if let Some(started_at) = pause_started_at {
        budget_tracker.exclude_wall_clock(started_at.elapsed());
    }
    result
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
            Self::Custom(input) => serde_json::json!({ "input": input }),
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
) -> Result<ToolExecutionBatch, ToolExecutionError> {
    let batch_started_at = Instant::now();
    let mut pending = BTreeMap::new();
    let mut futures = FuturesUnordered::new();
    let scheduled_count = scheduled.len();
    let parallel_candidates = scheduled
        .iter()
        .filter(|scheduled| scheduled.parallel_candidate)
        .count() as u64;
    let duplicate_suppressed = scheduled
        .iter()
        .filter(|scheduled| scheduled.duplicate_suppressed)
        .count() as u64;

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
    let mut tool_execution_millis = 0_u64;

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
                        return Ok(tool_execution_batch(
                            ordered_records.into_iter().flatten().collect(),
                            batch_started_at,
                            tool_execution_millis,
                            parallel_candidates,
                            duplicate_suppressed,
                        ));
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
        tool_execution_millis = tool_execution_millis.saturating_add(record.execution_millis);
        finalize_tool_item(recorder, item, &record);
        emit_tool_progress(progress, &record);
        ordered_records[index] = Some(record);
    }

    Ok(tool_execution_batch(
        ordered_records.into_iter().flatten().collect(),
        batch_started_at,
        tool_execution_millis,
        parallel_candidates,
        duplicate_suppressed,
    ))
}

fn tool_execution_batch(
    records: Vec<ToolExecutionRecord>,
    batch_started_at: Instant,
    tool_execution_millis: u64,
    parallel_candidates: u64,
    duplicate_suppressed: u64,
) -> ToolExecutionBatch {
    let tool_cache_hits = records
        .iter()
        .flat_map(|record| &record.runtime_events)
        .filter(|event| matches!(event, ToolRuntimeEvent::CacheHit { .. }))
        .count() as u64;
    let tool_batch_elapsed_millis = batch_started_at.elapsed().as_millis() as u64;
    ToolExecutionBatch {
        records,
        orchestration: InferenceOrchestrationMetrics {
            parallel_candidates,
            actual_parallel_calls: if parallel_candidates > 1 {
                parallel_candidates
            } else {
                0
            },
            tool_batch_elapsed_millis,
            tool_execution_millis,
            tool_critical_path_millis: tool_execution_millis.min(tool_batch_elapsed_millis),
            tool_cache_hits,
            duplicate_suppressed,
            ..InferenceOrchestrationMetrics::default()
        },
    }
}

#[cfg(test)]
mod tests;
