use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Instant;

use futures::FutureExt;
use futures::future::BoxFuture;
use futures::stream::{FuturesUnordered, StreamExt};
use pl_model::completion::ToolCallPayload;
use pl_protocol::{InferenceOrchestrationMetrics, ToolCallKind};
use pl_trace::{TracePartAction, TraceToolActivePhase, TraceToolFailureKind, TraceToolInvocation};
use tokio::sync::RwLock;

use crate::ToolCompletion;
use crate::permission::{PermissionDecision, decide_tool_permission};
use crate::tool::cache::{ToolCacheExecutionRequest, ToolCachePolicy, TurnToolCacheHandle};
use crate::tool::{
    AgentWorkspace, SubagentContext, ToolApprovalContext, ToolBatchPolicy, ToolBudgetTiming,
    ToolCallContext, ToolCallIdentity, ToolDirective, ToolInput, ToolPlan, ToolRuntimeLockPolicy,
    WorkspaceAccess,
};
use crate::turn::{BudgetTracker, ToolApprovalDecision, ToolExecutionMode, TurnOptions};

use super::TurnEngine;
use super::permission::{approval_request, request_user_approval, requested_workspace_access};
use super::progress::{ProgressEmitter, ProgressVerbosity};
use super::turn_result::is_cancelled;
use crate::time::unix_seconds;

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
    pub(super) call_id: String,
    pub(super) name: String,
    pub(super) kind: ToolCallKind,
    pub(super) result: String,
    pub(super) display_result: String,
    pub(super) model_attachments: Vec<pl_protocol::ThreadAttachment>,
    pub(super) arguments: String,
    pub(super) outcome: ToolExecutionOutcome,
    pub(super) exit_code: Option<i32>,
    pub(super) timed_out: bool,
    pub(super) runtime_events: Vec<ToolDirective>,
    pub(super) execution_millis: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToolExecutionOutcome {
    Succeeded,
    Failed(TraceToolFailureKind),
    Denied,
    Cancelled,
}

impl ToolExecutionOutcome {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed(TraceToolFailureKind::Execution) => "failed",
            Self::Failed(TraceToolFailureKind::TimedOut) => "timedOut",
            Self::Failed(TraceToolFailureKind::BudgetLimited) => "budgetLimited",
            Self::Denied => "denied",
            Self::Cancelled => "cancelled",
        }
    }
}

pub(super) struct ScheduledToolExecution<'a> {
    pub(super) tool_call: pl_model::completion::ToolCall,
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
    pub(super) payload: ToolPayload,
    pub(super) context: ToolCallContext,
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
    pub(super) tool_plan: ToolPlan,
    pub(super) options: &'a TurnOptions,
    pub(super) session_id: &'a str,
    pub(super) turn_id: &'a str,
    pub(super) step: u32,
    pub(super) workspace: AgentWorkspace,
    pub(super) active_subagent: Option<SubagentContext>,
    pub(super) tool_cache: TurnToolCacheHandle,
}

#[cfg(test)]
pub(super) async fn execute_tool_calls(
    tool_calls: &[pl_model::completion::ToolCall],
    budget_tracker: &mut BudgetTracker,
    recorder: &mut crate::trace::TraceRecorder,
    context: ToolExecutionContext<'_>,
) -> Result<Vec<ToolExecutionRecord>, ToolExecutionError> {
    execute_tool_call_batch(tool_calls, budget_tracker, recorder, context)
        .await
        .map(|batch| batch.records)
}

pub(super) async fn execute_tool_call_batch(
    tool_calls: &[pl_model::completion::ToolCall],
    budget_tracker: &mut BudgetTracker,
    recorder: &mut crate::trace::TraceRecorder,
    context: ToolExecutionContext<'_>,
) -> Result<ToolExecutionBatch, ToolExecutionError> {
    let mut scheduled = Vec::new();
    let mut scheduled_exact_once_calls = HashMap::<(String, String), String>::new();
    let runtime_lock = Arc::new(RwLock::new(()));
    let tool_cache_snapshot = context.tool_cache.snapshot();
    let sid = &context.turn_id;
    let mut progress = ProgressEmitter::new_scoped(
        context.turn_id.to_string(),
        format!("{sid}:tool-progress"),
        ProgressVerbosity::from_env(),
    );
    let solo_batch_violation = tool_calls.len() != 1
        && tool_calls.iter().any(|tool_call| {
            context
                .tool_plan
                .binding(&tool_call.name)
                .is_some_and(|binding| {
                    binding.tool().policy().batch_policy() == ToolBatchPolicy::Solo
                })
        });

    for tool_call in tool_calls {
        context.options.apply_budget_refresh(budget_tracker);
        if is_cancelled(context.options) {
            break;
        }
        if tool_call.name.is_empty() {
            return Err(ToolExecutionError::Fatal(
                "tool call missing tool name".to_string(),
            ));
        }
        let trace_part_id = tool_trace_part_id(context.turn_id, tool_call);
        let mut item = recorder
            .latest_tool_trace_part(
                &trace_part_id,
                Some(tool_call.call_id.as_str()),
                Some(tool_call.id.as_str()),
            )
            .unwrap_or_else(|| {
                let item = recorder.tool_item(
                    context.turn_id,
                    &trace_part_id,
                    tool_call.name.clone(),
                    tool_call.payload_text(),
                    Some(tool_call.call_id.clone()),
                    Some(tool_call.id.clone()),
                );
                recorder.start_item(item.clone());
                item
            });
        let invocation = TraceToolInvocation::new(
            trace_part_id.clone(),
            tool_call.name.clone(),
            tool_call.payload_text(),
        )
        .with_provider_identity(Some(tool_call.call_id.clone()), Some(tool_call.id.clone()));
        if let Err(error) = item.apply(item.command(
            unix_seconds(),
            TracePartAction::UpdateToolInvocation { invocation },
        )) {
            tracing::error!(%error, "failed to update tool invocation before dispatch");
        }
        budget_tracker.record_tool_call(&tool_call.name);

        if solo_batch_violation {
            scheduled.push(ScheduledToolExecution {
                tool_call: tool_call.clone(),
                item,
                future: ready_tool_execution_record(
                    tool_call.clone(),
                    ToolExecutionError::RespondToModel(
                        "This provider response contains a Solo tool call. The entire batch was rejected without side effects; retry the Solo call as the only tool call."
                            .to_string(),
                    ),
                    ToolExecutionOutcome::Failed(TraceToolFailureKind::Execution),
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

        if let Some(message) = tool_call.invalid_arguments_message() {
            scheduled.push(ScheduledToolExecution {
                tool_call: tool_call.clone(),
                item,
                future: ready_tool_execution_record(
                    tool_call.clone(),
                    ToolExecutionError::RespondToModel(message),
                    ToolExecutionOutcome::Failed(TraceToolFailureKind::Execution),
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

        let registered_tool = context.tool_plan.binding(&tool_call.name);
        let effect = registered_tool.and_then(|entry| entry.tool().policy().effect());
        let allowed = context
            .options
            .execution_policy
            .as_ref()
            .is_none_or(|policy| policy.allows_effect(effect));
        if !allowed {
            let name = &tool_call.name;
            let message = format!("Tool disabled by execution policy: {name}");
            scheduled.push(ScheduledToolExecution {
                tool_call: tool_call.clone(),
                item,
                future: ready_tool_execution_record(
                    tool_call.clone(),
                    ToolExecutionError::RespondToModel(message),
                    ToolExecutionOutcome::Denied,
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
        let Some(binding) = registered_tool else {
            let available = context.tool_plan.names().collect::<Vec<_>>();
            tracing::warn!(
                tool = %tool_call.name,
                available = ?available,
                "model requested an unknown tool"
            );
            let name = &tool_call.name;
            scheduled.push(ScheduledToolExecution {
                tool_call: tool_call.clone(),
                item,
                future: ready_tool_execution_record(
                    tool_call.clone(),
                    ToolExecutionError::RespondToModel(format!("Unknown tool: {name}")),
                    ToolExecutionOutcome::Failed(TraceToolFailureKind::Execution),
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

        let executor_generation = binding.generation();
        let tool = binding.tool();
        let supports_parallel = tool.policy().supports_parallel_tool_calls()
            && matches!(
                context.options.tool_execution_mode,
                ToolExecutionMode::ModelDefault | ToolExecutionMode::Parallel
            );
        let runtime_lock_policy = if matches!(
            context.options.tool_execution_mode,
            ToolExecutionMode::ModelDefault | ToolExecutionMode::Parallel
        ) {
            tool.policy().runtime_lock_policy()
        } else {
            ToolRuntimeLockPolicy::Exclusive
        };
        let parallel_candidate = matches!(runtime_lock_policy, ToolRuntimeLockPolicy::None)
            || matches!(runtime_lock_policy, ToolRuntimeLockPolicy::Shared) && supports_parallel;
        let mut approval_request = approval_request(tool_call, context.active_subagent.as_ref());
        approval_request.id = trace_part_id.clone();
        let requested_access = requested_workspace_access(tool_call, context.workspace.root());
        let mut execution_workspace_access = WorkspaceAccess::WorkspaceOnly;
        let mut awaited_approval = false;
        let decision =
            match decide_tool_permission(context.options, &approval_request, requested_access) {
                PermissionDecision::Approved { workspace_access } => {
                    execution_workspace_access = workspace_access;
                    ToolApprovalDecision::Approved
                }
                PermissionDecision::NeedsUserApproval { workspace_access } => {
                    awaited_approval = true;
                    emit_tool_snapshot(recorder, &mut item, TraceToolActivePhase::AwaitingApproval);
                    let name = &tool_call.name;
                    progress.tool_detail(recorder, format!("工具 `{name}` 正在等待授权。"));
                    let decision =
                        request_user_approval(context.options, &approval_request, context.turn_id)
                            .await;
                    if matches!(decision, ToolApprovalDecision::Approved) {
                        execution_workspace_access = workspace_access;
                    }
                    decision
                }
                PermissionDecision::NeedsAiReview { workspace_access } => {
                    awaited_approval = true;
                    emit_tool_snapshot(recorder, &mut item, TraceToolActivePhase::AwaitingApproval);
                    let name = &tool_call.name;
                    progress.tool_detail(recorder, format!("正在审查工具 `{name}`。"));
                    let decision = context
                        .core
                        .review_tool_call_with_ai(
                            &approval_request,
                            context.options.permission_mode,
                            workspace_access,
                            context.workspace.root(),
                        )
                        .await;
                    if matches!(decision, ToolApprovalDecision::Approved) {
                        execution_workspace_access = workspace_access;
                    }
                    decision
                }
            };
        if is_cancelled(context.options) {
            scheduled.push(ScheduledToolExecution {
                tool_call: tool_call.clone(),
                item,
                future: ready_tool_execution_record(
                    tool_call.clone(),
                    ToolExecutionError::RespondToModel("Tool execution interrupted".to_string()),
                    ToolExecutionOutcome::Cancelled,
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
                if awaited_approval {
                    emit_tool_snapshot(recorder, &mut item, TraceToolActivePhase::Approved);
                }
                emit_tool_snapshot(recorder, &mut item, TraceToolActivePhase::Running);
                let active = context.active_subagent.as_ref();
                let identity = ToolCallIdentity {
                    call_id: tool_call.call_id.clone(),
                    item_id: trace_part_id.clone(),
                    agent_id: active
                        .map(|agent| agent.id.clone())
                        .unwrap_or_else(|| context.session_id.to_string()),
                    parent_agent_id: active.and_then(|agent| agent.parent_id.clone()),
                    agent_path: active.and_then(|agent| agent.agent_path.clone()),
                    agent_role: active
                        .map_or_else(|| "root".to_string(), |agent| agent.role.clone()),
                    agent_depth: active.map_or(0, |agent| agent.depth),
                    session_id: context.session_id.to_string(),
                    turn_id: context.turn_id.to_string(),
                    step: context.step,
                    started_sequence: item.started_sequence(),
                    revision_base: item.revision(),
                };
                let approval = ToolApprovalContext::new(
                    context.options.permission_mode,
                    execution_workspace_access,
                )
                .with_interaction(
                    context.options.interaction_callback.clone(),
                    context.options.user_input_mode,
                );
                let tool_context = ToolCallContext::new(identity, recorder.sender().clone())
                    .with_trace_sink(recorder.trace_sink())
                    .with_cancellation(context.options.cancellation_token.clone())
                    .with_approval(approval);
                progress.tool_detail(recorder, tool_start_progress_message(&tool_call.name));
                let invocation = ToolInvocation::from_tool_call(tool_call, tool_context);
                let _display_arguments = invocation.payload.arguments_for_display();
                let tool_input = ToolInput {
                    arguments: invocation.payload.arguments_for_tool(),
                };
                let cache_policy = tool.policy().cache_policy(&tool_input.arguments);
                let invalidates_cache = tool.policy().invalidates_cache(&tool_input.arguments);
                let lock = runtime_lock.clone();
                let tool_name = invocation.name.clone();
                let tool_call_for_task = tool_call.clone();
                let tool_context = invocation.context;
                let cache = context.tool_cache.clone();
                let cache_snapshot = tool_cache_snapshot.clone();
                let cache_arguments = tool_input.arguments.clone();
                let cache_workspace_root = context.workspace.root().to_path_buf();
                let cache_call_id = tool_call.call_id.clone();
                let tool_manager = context.core.agent_tools.manager().clone();
                let tool_plan = context.tool_plan.clone();
                let tool_effect = effect;
                let budget_timing = tool.policy().budget_timing();
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
                                ToolExecutionOutcome::Succeeded,
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
                                ToolCacheExecutionRequest {
                                    tool_name: &tool_name,
                                    arguments: &cache_arguments,
                                    workspace_root: &cache_workspace_root,
                                    policy: cache_policy,
                                    call_id: cache_call_id,
                                    executor_generation,
                                },
                                || {
                                    tool_manager.execute(
                                        &tool_plan,
                                        &tool_name,
                                        tool_input,
                                        tool_context,
                                    )
                                },
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
                scheduled.push(ScheduledToolExecution {
                    tool_call: tool_call.clone(),
                    item,
                    future: ready_tool_execution_record(
                        tool_call.clone(),
                        ToolExecutionError::RespondToModel(format!(
                            "Tool execution denied: {reason}"
                        )),
                        ToolExecutionOutcome::Denied,
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
    context.options.apply_budget_refresh(budget_tracker);
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

fn tool_trace_part_id(turn_id: &str, tool_call: &pl_model::completion::ToolCall) -> String {
    let tool_call_id = if tool_call.id.is_empty() {
        tool_call.call_id.as_str()
    } else {
        &tool_call.id
    };
    namespaced_tool_trace_part_id(turn_id, tool_call_id)
}

impl ToolInvocation {
    fn from_tool_call(
        tool_call: &pl_model::completion::ToolCall,
        context: ToolCallContext,
    ) -> Self {
        Self {
            name: tool_call.name.clone(),
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
                            emit_tool_progress(progress, recorder, &record);
                            notify_tool_completion(options, &record).await?;
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
        emit_tool_progress(progress, recorder, &record);
        notify_tool_completion(options, &record).await?;
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

async fn notify_tool_completion(
    options: &TurnOptions,
    record: &ToolExecutionRecord,
) -> Result<(), ToolExecutionError> {
    let Some(callback) = options.tool_completion_callback.as_ref() else {
        return Ok(());
    };
    callback(ToolCompletion {
        call_id: record.call_id.clone(),
        name: record.name.clone(),
        status: record.outcome.as_str().to_string(),
        result: record.result.clone(),
        exit_code: record.exit_code,
        timed_out: record.timed_out,
    })
    .await
    .map_err(|error| {
        ToolExecutionError::Fatal(format!(
            "host post-tool observation failed after {}: {error:#}",
            record.name
        ))
    })
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
        .filter(|event| matches!(event, ToolDirective::CacheHit { .. }))
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
mod unit_tests;
