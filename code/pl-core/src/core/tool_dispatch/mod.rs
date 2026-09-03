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
mod tool_execution_tests {
    use futures::FutureExt;
    use pretty_assertions::assert_eq;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use pl_model::completion::ToolCall;
    use pl_protocol::PureError;
    use pl_trace::{TraceEventKind, TracePartKind};

    use crate::trace::TraceRecorder;

    use super::super::test_support::*;
    use super::*;
    use crate::tool::cache::ToolCachePolicy;
    use crate::tool::{
        LocalWorkspaceFileTool, StaticTool, ToolCallContext, ToolPolicy, ToolResult,
        WorkspaceFileToolKind,
    };
    use crate::tool::{ToolBatchPolicy, ToolBudgetTiming, ToolRuntimeLockPolicy};

    #[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct EmptyTestToolInput {}

    #[derive(Debug)]
    struct ProviderCallIdEchoTool;

    impl StaticTool for ProviderCallIdEchoTool {
        type Input = serde_json::Value;

        fn definition(&self) -> crate::tool::StaticToolDefinition {
            test_static_tool_definition(
                "provider_call_id_echo",
                "Returns the stable provider call identity from ToolCallContext",
            )
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            })
        }

        fn policy(&self) -> ToolPolicy {
            ToolPolicy::default()
        }

        fn execute(
            &self,
            _input: Self::Input,
            context: ToolCallContext,
        ) -> impl std::future::Future<Output = crate::Result<ToolResult>> + Send {
            async move { Ok(ToolResult::success(context.identity().call_id.clone())) }.boxed()
        }
    }

    #[derive(Debug)]
    struct BudgetPausedWaitTool;

    #[derive(Debug)]
    struct FailingApplyPatchTool {
        executions: std::sync::Arc<AtomicUsize>,
    }

    #[derive(Debug)]
    struct CountingSpawnAgentTool {
        executions: std::sync::Arc<AtomicUsize>,
    }

    #[derive(Debug)]
    struct CountingExecTool {
        executions: std::sync::Arc<AtomicUsize>,
    }

    #[derive(Debug)]
    struct CountingSoloTool {
        executions: std::sync::Arc<AtomicUsize>,
    }

    #[derive(Debug)]
    struct CountingCacheableTool {
        executions: std::sync::Arc<AtomicUsize>,
    }

    #[derive(Debug)]
    struct BatchFailingReadTool {
        executions: std::sync::Arc<AtomicUsize>,
        first_started: std::sync::Arc<tokio::sync::Notify>,
        release_first: std::sync::Arc<tokio::sync::Notify>,
    }

    #[derive(Debug)]
    struct BatchEpochProcessTool {
        first_started: std::sync::Arc<tokio::sync::Notify>,
        release_first: std::sync::Arc<tokio::sync::Notify>,
    }

    impl StaticTool for FailingApplyPatchTool {
        type Input = serde_json::Value;

        fn definition(&self) -> crate::tool::StaticToolDefinition {
            test_static_tool_definition("apply_patch", "Test-only failing patch tool")
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string" },
                    "cwd": { "type": "string" }
                },
                "required": ["input"],
                "additionalProperties": false
            })
        }

        fn policy(&self) -> ToolPolicy {
            ToolPolicy::default()
        }

        fn execute(
            &self,
            _input: Self::Input,
            _context: ToolCallContext,
        ) -> impl std::future::Future<Output = crate::Result<ToolResult>> + Send {
            async move {
                self.executions.fetch_add(1, Ordering::SeqCst);
                Err(PureError::ToolExecutionFailed {
                    tool: "apply_patch".to_string(),
                    error: "failed to find expected lines".to_string(),
                })
            }
            .boxed()
        }
    }

    impl StaticTool for CountingSpawnAgentTool {
        type Input = serde_json::Value;

        fn definition(&self) -> crate::tool::StaticToolDefinition {
            test_static_tool_definition("spawn_agent", "Test-only agent spawn tool")
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" },
                    "role": { "type": "string" },
                    "forkTurns": { "type": "string" },
                    "metadata": { "type": "object" }
                },
                "required": ["message", "role"],
                "additionalProperties": false
            })
        }

        fn policy(&self) -> ToolPolicy {
            ToolPolicy::default().with_effect(crate::ToolEffect::AgentControl)
        }

        fn execute(
            &self,
            input: Self::Input,
            _context: ToolCallContext,
        ) -> impl std::future::Future<Output = crate::Result<ToolResult>> + Send {
            async move {
                let execution = self.executions.fetch_add(1, Ordering::SeqCst) + 1;
                Ok(ToolResult::success(
                    serde_json::json!({
                        "agentId": format!("agent-{execution}"),
                        "message": input["message"],
                    })
                    .to_string(),
                ))
            }
            .boxed()
        }
    }

    impl StaticTool for CountingExecTool {
        type Input = serde_json::Value;

        fn definition(&self) -> crate::tool::StaticToolDefinition {
            test_static_tool_definition("exec", "Test-only command tool")
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "cwd": { "type": "string" }
                },
                "required": ["command"],
                "additionalProperties": false
            })
        }

        fn policy(&self) -> ToolPolicy {
            ToolPolicy::default().with_effect(crate::ToolEffect::Process)
        }

        fn execute(
            &self,
            input: Self::Input,
            _context: ToolCallContext,
        ) -> impl std::future::Future<Output = crate::Result<ToolResult>> + Send {
            async move {
                self.executions.fetch_add(1, Ordering::SeqCst);
                Ok(ToolResult::success(
                    serde_json::json!({
                        "status": "completed",
                        "command": input["command"],
                    })
                    .to_string(),
                ))
            }
            .boxed()
        }
    }

    impl StaticTool for CountingSoloTool {
        type Input = serde_json::Value;

        fn definition(&self) -> crate::tool::StaticToolDefinition {
            test_static_tool_definition("solo_state", "Test-only Solo state tool")
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object", "additionalProperties": false})
        }

        fn policy(&self) -> ToolPolicy {
            ToolPolicy::read_only().with_batch_policy(ToolBatchPolicy::Solo)
        }

        fn execute(
            &self,
            _input: Self::Input,
            _context: ToolCallContext,
        ) -> impl std::future::Future<Output = crate::Result<ToolResult>> + Send {
            async move {
                self.executions.fetch_add(1, Ordering::SeqCst);
                Ok(ToolResult::success("mutated"))
            }
            .boxed()
        }
    }

    #[tokio::test]
    async fn solo_tool_rejects_the_entire_mixed_batch_without_side_effects() {
        let solo_executions = std::sync::Arc::new(AtomicUsize::new(0));
        let exec_executions = std::sync::Arc::new(AtomicUsize::new(0));
        let mut core = test_turn_engine();
        core.register_test_tool(CountingSoloTool {
            executions: solo_executions.clone(),
        });
        core.register_test_tool(CountingExecTool {
            executions: exec_executions.clone(),
        });
        let calls = [
            ToolCall::function(
                "solo-item",
                "solo_state",
                serde_json::json!({}),
                "solo-call",
            ),
            ToolCall::function(
                "exec-item",
                "exec",
                serde_json::json!({"command": "true"}),
                "exec-call",
            ),
        ];
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("session-solo".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let batch = execute_tool_call_batch(
            &calls,
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-solo",
                turn_id: "turn-solo",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();

        assert_eq!(solo_executions.load(Ordering::SeqCst), 0);
        assert_eq!(exec_executions.load(Ordering::SeqCst), 0);
        assert_eq!(batch.records.len(), 2);
        assert!(batch.records.iter().all(|record| {
            matches!(record.outcome, ToolExecutionOutcome::Failed(_))
                && record.result.contains("Solo")
        }));
    }

    impl StaticTool for CountingCacheableTool {
        type Input = serde_json::Value;

        fn definition(&self) -> crate::tool::StaticToolDefinition {
            test_static_tool_definition("cacheable_read", "Test-only cacheable read tool")
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"],
                "additionalProperties": false
            })
        }

        fn policy(&self) -> ToolPolicy {
            ToolPolicy::read_only().with_cache_policy(ToolCachePolicy::UntilWorkspaceMutation)
        }

        fn execute(
            &self,
            input: Self::Input,
            _context: ToolCallContext,
        ) -> impl std::future::Future<Output = crate::Result<ToolResult>> + Send {
            async move {
                self.executions.fetch_add(1, Ordering::SeqCst);
                Ok(ToolResult::success(
                    serde_json::json!({
                        "path": input["path"],
                        "content": "x".repeat(8_192),
                    })
                    .to_string(),
                ))
            }
            .boxed()
        }
    }

    impl StaticTool for BatchFailingReadTool {
        type Input = serde_json::Value;

        fn definition(&self) -> crate::tool::StaticToolDefinition {
            test_static_tool_definition("read_file", "Test-only deterministic read failure")
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"],
                "additionalProperties": false
            })
        }

        fn policy(&self) -> ToolPolicy {
            ToolPolicy::read_only()
                .with_cache_policy(ToolCachePolicy::UntilWorkspaceMutation)
                .with_runtime_lock_policy(ToolRuntimeLockPolicy::Exclusive)
        }

        fn execute(
            &self,
            _input: Self::Input,
            context: ToolCallContext,
        ) -> impl std::future::Future<Output = crate::Result<ToolResult>> + Send {
            async move {
                self.executions.fetch_add(1, Ordering::SeqCst);
                if context.identity().call_id == "read-call-1" {
                    self.first_started.notify_one();
                    self.release_first.notified().await;
                }
                Err(PureError::ToolExecutionFailed {
                    tool: "read_file".to_string(),
                    error: "startLine exceeds file length".to_string(),
                })
            }
            .boxed()
        }
    }

    impl StaticTool for BatchEpochProcessTool {
        type Input = serde_json::Value;

        fn definition(&self) -> crate::tool::StaticToolDefinition {
            test_static_tool_definition(
                "batch_epoch_process",
                "Test-only process effect between duplicate reads",
            )
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            })
        }

        fn policy(&self) -> ToolPolicy {
            ToolPolicy::default()
                .with_effect(crate::ToolEffect::Process)
                .with_runtime_lock_policy(ToolRuntimeLockPolicy::None)
        }

        fn execute(
            &self,
            _input: Self::Input,
            _context: ToolCallContext,
        ) -> impl std::future::Future<Output = crate::Result<ToolResult>> + Send {
            async move {
                self.first_started.notified().await;
                self.release_first.notify_one();
                Ok(ToolResult::success("epoch advanced"))
            }
            .boxed()
        }
    }

    impl StaticTool for BudgetPausedWaitTool {
        type Input = serde_json::Value;

        fn definition(&self) -> crate::tool::StaticToolDefinition {
            test_static_tool_definition("wait_agents", "Test-only blocking wait")
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            })
        }

        fn policy(&self) -> ToolPolicy {
            ToolPolicy::default().with_budget_timing(ToolBudgetTiming::PauseWhenOnlyScheduledTool)
        }

        fn execute(
            &self,
            _input: Self::Input,
            _context: ToolCallContext,
        ) -> impl std::future::Future<Output = crate::Result<ToolResult>> + Send {
            async {
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                Ok(ToolResult::success("progress"))
            }
            .boxed()
        }
    }

    fn read_file_result_text(result: Option<&str>) -> String {
        serde_json::from_str::<serde_json::Value>(result.expect("tool result"))
            .expect("read_file json")
            .get("text")
            .and_then(serde_json::Value::as_str)
            .expect("text")
            .to_string()
    }

    #[tokio::test]
    async fn identical_apply_patch_arguments_with_distinct_call_ids_execute_independently() {
        let executions = std::sync::Arc::new(AtomicUsize::new(0));
        let mut core = test_turn_engine();
        core.register_test_tool(FailingApplyPatchTool {
            executions: std::sync::Arc::clone(&executions),
        });
        let arguments = serde_json::json!({
            "cwd": ".",
            "input": "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-old\n+new\n*** End Patch"
        });
        let calls = [
            ToolCall::function(
                "patch-item-1",
                "apply_patch",
                arguments.clone(),
                "patch-call-1",
            ),
            ToolCall::function(
                "patch-item-2",
                "apply_patch",
                arguments.clone(),
                "patch-call-2",
            ),
            ToolCall::function(
                "patch-item-3",
                "apply_patch",
                serde_json::json!({
                    "cwd": ".",
                    "input": "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-old\n+different\n*** End Patch"
                }),
                "patch-call-3",
            ),
        ];
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let records = execute_tool_calls(
            &calls,
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();

        assert_eq!(executions.load(Ordering::SeqCst), 3);
        assert_eq!(records.len(), 3);
        assert!(records.iter().all(|record| matches!(
            record.outcome,
            ToolExecutionOutcome::Failed(pl_trace::TraceToolFailureKind::Execution)
        )));
    }

    #[tokio::test]
    async fn identical_spawn_agent_arguments_with_distinct_call_ids_execute_independently() {
        let executions = std::sync::Arc::new(AtomicUsize::new(0));
        let mut core = test_turn_engine();
        core.register_test_tool(CountingSpawnAgentTool {
            executions: std::sync::Arc::clone(&executions),
        });
        let assignment = serde_json::json!({
            "message": "Inspect component A without modifying files.",
            "role": "explorer",
            "forkTurns": "none",
            "metadata": { "scope": "src/component-a" }
        });
        let calls = [
            ToolCall::function(
                "spawn-item-1",
                "spawn_agent",
                assignment.clone(),
                "spawn-call-1",
            ),
            ToolCall::function("spawn-item-2", "spawn_agent", assignment, "spawn-call-2"),
            ToolCall::function(
                "spawn-item-3",
                "spawn_agent",
                serde_json::json!({
                    "message": "Inspect component B without modifying files.",
                    "role": "explorer",
                    "forkTurns": "none",
                    "metadata": { "scope": "src/component-b" }
                }),
                "spawn-call-3",
            ),
        ];
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let records = execute_tool_calls(
            &calls,
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();

        assert_eq!(executions.load(Ordering::SeqCst), 3);
        assert_eq!(records.len(), 3);
        assert!(
            records
                .iter()
                .all(|record| record.outcome == ToolExecutionOutcome::Succeeded)
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&records[1].result).unwrap()["agentId"],
            "agent-2"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&records[2].result).unwrap()["agentId"],
            "agent-3"
        );
    }

    #[tokio::test]
    async fn identical_exec_arguments_with_distinct_call_ids_execute_independently() {
        let executions = std::sync::Arc::new(AtomicUsize::new(0));
        let mut core = test_turn_engine();
        core.register_test_tool(CountingExecTool {
            executions: std::sync::Arc::clone(&executions),
        });
        let command = serde_json::json!({
            "command": "verify component-a",
            "cwd": "."
        });
        let calls = [
            ToolCall::function("exec-item-1", "exec", command.clone(), "exec-call-1"),
            ToolCall::function("exec-item-2", "exec", command.clone(), "exec-call-2"),
            ToolCall::function("exec-item-3", "exec", command, "exec-call-3"),
            ToolCall::function(
                "exec-item-4",
                "exec",
                serde_json::json!({
                    "command": "verify component-b",
                    "cwd": "."
                }),
                "exec-call-4",
            ),
        ];
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let records = execute_tool_calls(
            &calls,
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();

        assert_eq!(executions.load(Ordering::SeqCst), 4);
        assert_eq!(records.len(), 4);
        for record in &records {
            assert_eq!(record.outcome, ToolExecutionOutcome::Succeeded);
        }

        let repeated_response = ToolCall::function(
            "exec-item-5",
            "exec",
            serde_json::json!({
                "command": "verify component-a",
                "cwd": "."
            }),
            "exec-call-5",
        );
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("session-2".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));
        let records = execute_tool_calls(
            &[repeated_response],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();

        assert_eq!(executions.load(Ordering::SeqCst), 5);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, ToolExecutionOutcome::Succeeded);
    }

    #[tokio::test]
    async fn identical_cacheable_calls_return_compact_receipts_per_provider_response() {
        let executions = std::sync::Arc::new(AtomicUsize::new(0));
        let mut core = test_turn_engine();
        core.register_test_tool(CountingCacheableTool {
            executions: std::sync::Arc::clone(&executions),
        });
        let arguments = serde_json::json!({ "path": "src/lib.rs" });
        let calls = [
            ToolCall::function(
                "read-item-1",
                "cacheable_read",
                arguments.clone(),
                "read-call-1",
            ),
            ToolCall::function("read-item-2", "cacheable_read", arguments, "read-call-2"),
            ToolCall::function(
                "read-item-3",
                "cacheable_read",
                serde_json::json!({ "path": "src/main.rs" }),
                "read-call-3",
            ),
        ];
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let batch = execute_tool_call_batch(
            &calls,
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();
        assert_eq!(batch.orchestration.duplicate_suppressed, 1);
        let records = batch.records;

        assert_eq!(executions.load(Ordering::SeqCst), 2);
        assert_eq!(records.len(), 3);
        assert!(records[0].result.len() > 8_000);
        let duplicate = serde_json::from_str::<serde_json::Value>(&records[1].result).unwrap();
        assert_eq!(duplicate["status"], "duplicateSuppressed");
        assert_eq!(duplicate["reusedFromCallId"], "read-call-1");
        assert_eq!(duplicate["scope"], "providerResponse");
        assert!(records[1].result.len() < 256);
        assert!(records[2].result.len() > 8_000);
    }

    #[tokio::test]
    async fn tool_batch_reports_parallel_candidates_and_critical_path() {
        let mut core = test_turn_engine();
        core.register_test_tool(
            crate::tool::static_tool::<EmptyTestToolInput>(crate::tool::StaticToolDefinition::new(
                crate::tool::ToolName::bare("parallel_metric_read").unwrap(),
                "Test-only parallel metric tool",
            ))
            .policy(
                ToolPolicy::read_only()
                    .with_parallel_tool_calls()
                    .with_runtime_lock_policy(ToolRuntimeLockPolicy::Shared),
            )
            .build(|_input, _context| async move {
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                Ok(ToolResult::success("ok"))
            }),
        );
        let calls = [
            ToolCall::function(
                "read-item-1",
                "parallel_metric_read",
                serde_json::json!({}),
                "read-call-1",
            ),
            ToolCall::function(
                "read-item-2",
                "parallel_metric_read",
                serde_json::json!({}),
                "read-call-2",
            ),
        ];
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let batch = execute_tool_call_batch(
            &calls,
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();

        assert_eq!(batch.records.len(), 2);
        assert_eq!(batch.orchestration.parallel_candidates, 2);
        assert_eq!(batch.orchestration.actual_parallel_calls, 2);
        assert!(
            batch.orchestration.tool_execution_millis
                >= batch.orchestration.tool_critical_path_millis
        );
        assert!(batch.orchestration.tool_batch_elapsed_millis >= 20);
    }

    #[tokio::test]
    async fn mcp_registered_tools_use_policy_approval_batch_lock_and_trace_pipeline() {
        let mut core = test_turn_engine();
        let harness = crate::mcp::test_support::McpTestHarness::install_read_tool(&mut core).await;
        let calls = [
            ToolCall::function(
                "mcp-item-1",
                "mcp__docs__lookup",
                serde_json::json!({ "query": "first" }),
                "mcp-call-1",
            ),
            ToolCall::function(
                "mcp-item-2",
                "mcp__docs__lookup",
                serde_json::json!({ "query": "second" }),
                "mcp-call-2",
            ),
        ];
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let mut denied_recorder = TraceRecorder::new("session-mcp-denied".to_string(), event_tx, 0);
        let mut denied_budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let denied = execute_tool_call_batch(
            &calls[..1],
            &mut denied_budget,
            &mut denied_recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default()
                    .with_execution_policy(crate::AgentExecutionPolicy::default()),
                session_id: "turn-mcp-denied",
                turn_id: "turn-mcp-denied",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();
        assert_eq!(denied.records[0].outcome, ToolExecutionOutcome::Denied);

        let approvals = std::sync::Arc::new(AtomicUsize::new(0));
        let callback_approvals = approvals.clone();
        let options = TurnOptions::default()
            .with_execution_policy(crate::AgentExecutionPolicy {
                allowed_effects: crate::ToolEffectSet::from_effects([crate::ToolEffect::Read]),
                ..Default::default()
            })
            .with_interaction_callback(std::sync::Arc::new(move |_interaction| {
                let callback_approvals = callback_approvals.clone();
                async move {
                    callback_approvals.fetch_add(1, Ordering::SeqCst);
                    pl_protocol::InteractionResolution::ToolApproval(
                        pl_protocol::ToolApprovalResolutionPayload {
                            decision: pl_protocol::ToolApprovalResolution::Approved,
                            reason: None,
                        },
                    )
                }
                .boxed()
            }));
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-mcp".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));
        let batch = execute_tool_call_batch(
            &calls,
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &options,
                session_id: "turn-mcp",
                turn_id: "turn-mcp",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();

        assert!(
            batch
                .records
                .iter()
                .all(|record| record.outcome == ToolExecutionOutcome::Succeeded)
        );
        assert_eq!(batch.orchestration.parallel_candidates, 2);
        assert_eq!(batch.orchestration.actual_parallel_calls, 2);
        assert_eq!(approvals.load(Ordering::SeqCst), 0);
        let completed = recorder
            .drain()
            .into_iter()
            .filter_map(|event| match event.kind {
                TraceEventKind::TracePartCompleted { item }
                    if item.kind() == TracePartKind::Tool =>
                {
                    Some(item)
                }
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(completed.len(), 2);
        assert!(completed.iter().all(|item| {
            let output = item
                .tool()
                .and_then(pl_trace::TraceToolPart::terminal_output)
                .expect("MCP trace tool output");
            output.output_artifacts().is_empty() && !output.audit_metadata().is_empty()
        }));

        drop(core);
        harness.shutdown().await;
    }

    #[tokio::test]
    async fn tool_batch_critical_path_includes_serialized_exclusive_calls() {
        let mut core = test_turn_engine();
        core.register_test_tool(
            crate::tool::static_tool::<EmptyTestToolInput>(crate::tool::StaticToolDefinition::new(
                crate::tool::ToolName::bare("exclusive_metric_read").unwrap(),
                "Test-only exclusive metric tool",
            ))
            .build(|_input, _context| async move {
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                Ok(ToolResult::success("ok"))
            }),
        );
        let calls = [
            ToolCall::function(
                "exclusive-item-1",
                "exclusive_metric_read",
                serde_json::json!({}),
                "exclusive-call-1",
            ),
            ToolCall::function(
                "exclusive-item-2",
                "exclusive_metric_read",
                serde_json::json!({}),
                "exclusive-call-2",
            ),
        ];
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let batch = execute_tool_call_batch(
            &calls,
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();

        assert_eq!(batch.orchestration.parallel_candidates, 0);
        assert_eq!(batch.orchestration.actual_parallel_calls, 0);
        assert!(batch.orchestration.tool_critical_path_millis >= 40);
        assert_eq!(batch.orchestration.parallel_saved_millis(), 0);
    }

    #[tokio::test]
    async fn provider_response_uses_one_cache_epoch_across_concurrent_process_effect() {
        let executions = std::sync::Arc::new(AtomicUsize::new(0));
        let first_started = std::sync::Arc::new(tokio::sync::Notify::new());
        let release_first = std::sync::Arc::new(tokio::sync::Notify::new());
        let mut core = test_turn_engine();
        core.register_test_tool(BatchFailingReadTool {
            executions: std::sync::Arc::clone(&executions),
            first_started: std::sync::Arc::clone(&first_started),
            release_first: std::sync::Arc::clone(&release_first),
        });
        core.register_test_tool(BatchEpochProcessTool {
            first_started,
            release_first,
        });
        let arguments = serde_json::json!({ "path": "missing.rs" });
        let calls = [
            ToolCall::function("read-item-1", "read_file", arguments.clone(), "read-call-1"),
            ToolCall::function(
                "process-item",
                "batch_epoch_process",
                serde_json::json!({}),
                "process-call",
            ),
            ToolCall::function("read-item-2", "read_file", arguments, "read-call-2"),
        ];
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let records = execute_tool_calls(
            &calls,
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();

        assert_eq!(executions.load(Ordering::SeqCst), 1);
        assert_eq!(records.len(), 3);
        assert!(matches!(
            records[0].outcome,
            ToolExecutionOutcome::Failed(pl_trace::TraceToolFailureKind::Execution),
        ));
        assert_eq!(records[1].outcome, ToolExecutionOutcome::Succeeded);
        assert_eq!(records[2].outcome, ToolExecutionOutcome::Succeeded);
        let duplicate = serde_json::from_str::<serde_json::Value>(&records[2].result).unwrap();
        assert_eq!(duplicate["status"], "duplicateSuppressed");
        assert_eq!(duplicate["reusedFromCallId"], "read-call-1");
        assert_eq!(duplicate["scope"], "providerResponse");
    }

    #[tokio::test]
    async fn invalid_function_arguments_are_returned_to_the_model_without_running_the_tool() {
        let core = test_turn_engine();
        let tool_call = ToolCall::invalid_function(
            "call-1",
            "github_api_request",
            "{\"method\":\"POST\"\n\"path\":\"/repos/o/r/pulls/1/reviews\"}",
            "expected `,` or `}` at line 2 column 1",
            "call-1",
        );
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();

        assert_eq!(records.len(), 1);
        assert!(matches!(
            records[0].outcome,
            ToolExecutionOutcome::Failed(pl_trace::TraceToolFailureKind::Execution),
        ));
        assert!(records[0].result.contains("Invalid JSON arguments"));
        assert!(records[0].result.contains("github_api_request"));
    }

    #[tokio::test]
    async fn single_wait_agents_call_pauses_active_wall_clock_budget() {
        let mut core = test_turn_engine();
        core.register_test_tool(BudgetPausedWaitTool);
        let tool_call =
            ToolCall::function("wait-1", "wait_agents", serde_json::json!({}), "wait-1");
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(5),
        ));

        execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();

        assert!(budget.check_wall_clock().is_ok());
        assert_eq!(budget.usage().wait_calls, 1);
    }

    #[tokio::test]
    async fn mixed_tool_batch_keeps_wait_agents_time_in_active_budget() {
        let mut core = test_turn_engine();
        core.register_test_tool(BudgetPausedWaitTool);
        core.register_test_tool(ProviderCallIdEchoTool);
        let calls = [
            ToolCall::function("wait-1", "wait_agents", serde_json::json!({}), "wait-1"),
            ToolCall::function(
                "echo-1",
                "provider_call_id_echo",
                serde_json::json!({}),
                "echo-1",
            ),
        ];
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(5),
        ));

        execute_tool_calls(
            &calls,
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();

        assert!(budget.check_wall_clock().is_err());
        assert_eq!(budget.usage().wait_calls, 1);
    }

    #[tokio::test]
    async fn chat_tool_call_replays_item_id_as_call_id() {
        let mut core = test_turn_engine();
        core.register_test_tool(ProviderCallIdEchoTool);
        let tool_call = ToolCall::function(
            "chat-tool-call-1",
            "provider_call_id_echo",
            serde_json::json!({}),
            "chat-tool-call-1",
        );
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].result, "chat-tool-call-1");
        assert_eq!(records[0].call_id, "chat-tool-call-1");
        let terminal_call_id = recorder
            .drain()
            .into_iter()
            .find_map(|event| match event.kind {
                TraceEventKind::TracePartCompleted { item } => item
                    .tool()
                    .and_then(|tool| tool.invocation().call_id())
                    .map(ToOwned::to_owned),
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .expect("terminal tool trace");
        assert_eq!(terminal_call_id, "chat-tool-call-1");
    }

    #[tokio::test]
    async fn tool_execution_reuses_streamed_trace_part() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let workspace_root = std::env::temp_dir().join(format!("pure-tool-reuse-{unique}"));
        tokio::fs::create_dir_all(&workspace_root).await.unwrap();
        tokio::fs::write(workspace_root.join("note.txt"), "provider item reuse")
            .await
            .unwrap();
        let mut core = test_turn_engine();
        core.register_test_tool(LocalWorkspaceFileTool::new(
            WorkspaceFileToolKind::ReadFile,
            crate::tool::ToolWorkspace::new(crate::tool::AgentWorkspace::local(
                workspace_root.clone(),
            )),
        ));
        let tool_call = ToolCall::function(
            "provider-item-1",
            "read_file",
            serde_json::json!({"path": "note.txt"}),
            "call-1",
        );
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let streamed_item = recorder.tool_item(
            "turn-1",
            "turn-1-provider-item-1",
            "read_file".to_string(),
            "{\"path\":\"note.txt\"}".to_string(),
            Some("call-1".to_string()),
            Some("provider-item-1".to_string()),
        );
        recorder.start_item(streamed_item);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(workspace_root.clone()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();
        let events = recorder.drain();
        let terminal_tool = events
            .iter()
            .find_map(|event| match &event.kind {
                TraceEventKind::TracePartCompleted { item }
                    if item.kind() == TracePartKind::Tool
                        && item.item_id() == "turn-1-provider-item-1" =>
                {
                    Some(item)
                }
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .expect("completed tool item");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, ToolExecutionOutcome::Succeeded);
        let tool = terminal_tool.tool().expect("tool trace metadata");
        assert_eq!(tool.invocation().call_id(), Some("call-1"));
        assert_eq!(
            tool.invocation().provider_item_id(),
            Some("provider-item-1")
        );
        assert_eq!(tool.invocation().arguments(), "{\"path\":\"note.txt\"}");
        assert_eq!(
            read_file_result_text(
                tool.terminal_output()
                    .map(pl_trace::TraceToolOutput::result)
            ),
            "provider item reuse"
        );
        assert_eq!(
            tool_statuses(&events, "turn-1-provider-item-1"),
            vec![
                TestToolPhase::Started,
                TestToolPhase::Running,
                TestToolPhase::Succeeded,
            ]
        );
        assert!(runtime_progress_texts(&mut event_rx).is_empty());
        let _ = tokio::fs::remove_dir_all(workspace_root).await;
    }

    #[tokio::test]
    async fn tool_execution_reuses_streamed_trace_part_when_provider_id_arrives_late() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let workspace_root = std::env::temp_dir().join(format!("pure-tool-late-provider-{unique}"));
        tokio::fs::create_dir_all(&workspace_root).await.unwrap();
        tokio::fs::write(workspace_root.join("note.txt"), "late provider id")
            .await
            .unwrap();
        let mut core = test_turn_engine();
        core.register_test_tool(LocalWorkspaceFileTool::new(
            WorkspaceFileToolKind::ReadFile,
            crate::tool::ToolWorkspace::new(crate::tool::AgentWorkspace::local(
                workspace_root.clone(),
            )),
        ));
        let tool_call = ToolCall::function(
            "provider-item-1",
            "read_file",
            serde_json::json!({"path": "note.txt"}),
            "call-1",
        );
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let streamed_item = recorder.tool_item(
            "turn-1",
            "turn-1-call-1",
            "read_file".to_string(),
            "{\"path\":\"note".to_string(),
            Some("call-1".to_string()),
            None,
        );
        recorder.start_item(streamed_item);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(workspace_root.clone()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();
        let events = recorder.drain();
        let completed_tool_ids = events
            .iter()
            .filter_map(|event| match &event.kind {
                TraceEventKind::TracePartCompleted { item }
                    if item.kind() == TracePartKind::Tool =>
                {
                    Some(item.item_id())
                }
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, ToolExecutionOutcome::Succeeded);
        assert_eq!(completed_tool_ids, vec!["turn-1-call-1"]);
        let terminal_tool = events
            .iter()
            .find_map(|event| match &event.kind {
                TraceEventKind::TracePartCompleted { item }
                    if item.item_id() == "turn-1-call-1" =>
                {
                    Some(item)
                }
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .expect("completed late-provider tool item");
        let tool = terminal_tool.tool().expect("tool trace metadata");
        assert_eq!(tool.invocation().call_id(), Some("call-1"));
        assert_eq!(
            tool.invocation().provider_item_id(),
            Some("provider-item-1")
        );
        assert_eq!(
            read_file_result_text(
                tool.terminal_output()
                    .map(pl_trace::TraceToolOutput::result)
            ),
            "late provider id"
        );
        assert_eq!(
            tool_statuses(&events, "turn-1-call-1"),
            vec![
                TestToolPhase::Started,
                TestToolPhase::Running,
                TestToolPhase::Succeeded,
            ]
        );
        assert!(tool_statuses(&events, "turn-1-provider-item-1").is_empty());
        let _ = tokio::fs::remove_dir_all(workspace_root).await;
    }

    #[tokio::test]
    async fn tool_runtime_deltas_use_trace_part_id() {
        let mut core = test_turn_engine();
        core.register_test_tool(DeltaEchoTool);
        let tool_call = ToolCall::function(
            "provider-item-1",
            "delta_echo",
            serde_json::json!({}),
            "call-1",
        );
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();
        let mut live_events = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            live_events.push(event);
        }
        let events = recorder.drain();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, ToolExecutionOutcome::Succeeded);
        assert_eq!(
            live_tool_result_deltas(&live_events, "turn-1-provider-item-1"),
            vec!["runtime delta".to_string()]
        );
        assert_eq!(
            tool_statuses(&events, "turn-1-provider-item-1"),
            vec![
                TestToolPhase::Started,
                TestToolPhase::Running,
                TestToolPhase::Succeeded,
            ]
        );
    }
}
