mod interaction;

pub(crate) use interaction::interaction as bridge_interaction;

use anyhow::Result;
use pl_protocol::{
    BudgetLimitKind, BudgetLimitSnapshot, McpAvailabilityDescriptor, McpHealthSnapshot,
    McpServerDescriptor, PromptPrefixChangedReason, RuntimeCostAmount, Thread, ThreadAgentState,
    ThreadAttachment, ThreadContentLifecycle, ThreadInferenceState, ThreadItem, ThreadItemDelta,
    ThreadItemDeltaState, ThreadItemState, ThreadNotification, ThreadNotificationEnvelope,
    ThreadRuntimeSnapshot, ThreadRuntimeUsage, ThreadSnapshot, ThreadStatus,
    ThreadSubscriptionUpdate, ThreadTextChannel, ThreadToolFailureKind, ThreadToolOutput,
    ThreadToolState, TodoItem, TodoListSnapshot, TodoStatus, TokenUsageSnapshot, Turn,
    TurnCancellationCause, TurnCompletion, TurnPhase, TurnRolloverOutcome, TurnState,
    WorkflowRunLifecycle, WorkflowRuntimeRunSnapshot, WorkflowRuntimeSnapshot,
};

use crate::api::studio::types::*;

pub(crate) fn bridge_thread_update(
    update: ThreadSubscriptionUpdate,
) -> Result<Option<BridgeThreadSubscriptionUpdate>> {
    match update {
        ThreadSubscriptionUpdate::Snapshot { snapshot } => {
            Ok(Some(BridgeThreadSubscriptionUpdate::Snapshot {
                snapshot: Box::new(bridge_thread_snapshot(*snapshot)?),
            }))
        }
        ThreadSubscriptionUpdate::Notification { notification } => {
            Ok(thread_notification(*notification)?.map(|notification| {
                BridgeThreadSubscriptionUpdate::Notification {
                    notification: Box::new(notification),
                }
            }))
        }
    }
}

fn thread_notification(
    envelope: ThreadNotificationEnvelope,
) -> Result<Option<BridgeThreadNotificationEnvelope>> {
    let notification = match envelope.notification {
        ThreadNotification::TurnStarted { turn: value } => BridgeThreadNotification::TurnStarted {
            turn: Box::new(bridge_turn(value)),
        },
        ThreadNotification::TurnUpdated { turn: value } => BridgeThreadNotification::TurnUpdated {
            turn: Box::new(bridge_turn(value)),
        },
        ThreadNotification::TurnCompleted { turn: value } => {
            BridgeThreadNotification::TurnCompleted {
                turn: Box::new(bridge_turn(value)),
            }
        }
        ThreadNotification::ItemStarted { item: value } => {
            let Some(item) = bridge_thread_item(*value)? else {
                return Ok(None);
            };
            BridgeThreadNotification::ItemStarted {
                item: Box::new(item),
            }
        }
        ThreadNotification::ItemDelta { delta } => BridgeThreadNotification::ItemDelta {
            delta: Box::new(item_delta(delta)),
        },
        ThreadNotification::ItemCompleted { item: value } => {
            let Some(item) = bridge_thread_item(*value)? else {
                return Ok(None);
            };
            BridgeThreadNotification::ItemCompleted {
                item: Box::new(item),
            }
        }
        ThreadNotification::InteractionChanged { interaction: value } => {
            BridgeThreadNotification::InteractionChanged {
                interaction: Box::new(interaction::interaction(*value)?),
            }
        }
        ThreadNotification::ThreadRuntimeUpdated { runtime: value } => {
            BridgeThreadNotification::ThreadRuntimeUpdated {
                runtime: Box::new(runtime_snapshot(*value)),
            }
        }
        ThreadNotification::Lagged { dropped } => BridgeThreadNotification::Lagged { dropped },
    };
    Ok(Some(BridgeThreadNotificationEnvelope {
        thread_id: envelope.thread_id,
        revision: envelope.revision,
        emitted_at: envelope.emitted_at,
        notification,
    }))
}

/// wire 快照的 item 窗口上限（低于 GUI 侧历史窗口上限，留出加载余量）。
/// 超过后按整 Turn 从最旧方向截断，被截内容经 `history_cursor` 回源。
const SNAPSHOT_ITEM_WINDOW: usize = 400;

pub(crate) fn bridge_thread_snapshot(value: ThreadSnapshot) -> Result<BridgeThreadSnapshot> {
    let runtime_availability = if value.runtime.is_some() {
        BridgeThreadRuntimeAvailability::Active
    } else {
        BridgeThreadRuntimeAvailability::Inactive
    };
    let all_items = value
        .items
        .into_iter()
        .map(bridge_thread_item)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let (items, history_cursor) = snapshot_item_window(all_items);
    Ok(BridgeThreadSnapshot {
        schema_version: value.schema_version,
        revision: value.revision,
        thread: bridge_thread(value.thread),
        active_turn: value.active_turn.map(bridge_turn),
        items,
        history_cursor,
        interactions: value
            .interactions
            .into_iter()
            .map(interaction::interaction)
            .collect::<Result<Vec<_>>>()?,
        runtime: value.runtime.map(runtime_snapshot),
        runtime_availability,
    })
}

/// 按 item 窗口上限截断快照；窗口起点回退到整 Turn 边界，锚点 Turn 完整保留。
/// `history_cursor` 是窗口首 Turn 的 id：以它做 before 锚点回源恰好取回被截段。
fn snapshot_item_window(
    mut items: Vec<BridgeThreadItem>,
) -> (Vec<BridgeThreadItem>, Option<String>) {
    if items.len() <= SNAPSHOT_ITEM_WINDOW {
        return (items, None);
    }
    let mut start = items.len() - SNAPSHOT_ITEM_WINDOW;
    while start > 0 && items[start - 1].turn_id == items[start].turn_id {
        start -= 1;
    }
    if start == 0 {
        return (items, None);
    }
    let cursor = items[start].turn_id.clone();
    (items.split_off(start), Some(cursor))
}

pub(crate) fn bridge_thread(value: Thread) -> BridgeThread {
    BridgeThread {
        id: value.id,
        project_id: value.project_id,
        title: value.title,
        mode: value.mode.as_str().to_string(),
        root_thread_id: value.root_thread_id,
        parent_thread_id: value.parent_thread_id,
        role: value.role,
        agent_path: value.agent_path,
        status: match value.status {
            ThreadStatus::Idle => BridgeThreadStatus::Idle,
            ThreadStatus::Queued => BridgeThreadStatus::Queued,
            ThreadStatus::Running => BridgeThreadStatus::Running,
            ThreadStatus::WaitingTool => BridgeThreadStatus::WaitingTool,
            ThreadStatus::WaitingInteraction => BridgeThreadStatus::WaitingInteraction,
            ThreadStatus::Cancelling => BridgeThreadStatus::Cancelling,
            ThreadStatus::Closing => BridgeThreadStatus::Closing,
            ThreadStatus::Closed => BridgeThreadStatus::Closed,
            ThreadStatus::Faulted => BridgeThreadStatus::Faulted,
        },
        created_at: value.created_at,
        updated_at: value.updated_at,
        archived: value.archived,
    }
}

pub(crate) fn bridge_turn(value: Turn) -> BridgeTurn {
    BridgeTurn {
        id: value.id,
        thread_id: value.thread_id,
        revision: value.revision,
        state: bridge_turn_state(&value.state),
        updated_at: value.updated_at,
    }
}

fn bridge_turn_state(value: &TurnState) -> BridgeTurnState {
    match value {
        TurnState::Queued(state) => BridgeTurnState::Queued {
            queued_at: state.queued_at(),
        },
        TurnState::Running(state) => BridgeTurnState::Running {
            started_at: state.started_at(),
            phase: turn_phase(state.phase()),
        },
        TurnState::Completed(state) => BridgeTurnState::Completed {
            started_at: state.started_at(),
            completed_at: state.completed_at(),
            completion: match state.completion() {
                TurnCompletion::Normal => BridgeTurnCompletion::Normal,
                TurnCompletion::InteractionRequested => BridgeTurnCompletion::InteractionRequested,
            },
        },
        TurnState::Cancelled(state) => BridgeTurnState::Cancelled {
            started_at: state.started_at(),
            requested_at: state.requested_at(),
            completed_at: state.completed_at(),
            cause: turn_cancellation_cause(state.cause().clone()),
        },
        TurnState::Failed(state) => BridgeTurnState::Failed {
            started_at: state.started_at(),
            completed_at: state.completed_at(),
            failure: bridge_turn_failure(state.failure().clone()),
        },
        TurnState::BudgetLimited(state) => BridgeTurnState::BudgetLimited {
            started_at: state.started_at(),
            completed_at: state.completed_at(),
            limit: turn_budget_limit(*state.limit()),
            rollover: turn_rollover(state.rollover().clone()),
        },
    }
}

fn turn_cancellation_cause(value: TurnCancellationCause) -> BridgeTurnCancellationCause {
    match value {
        TurnCancellationCause::UserRequested => BridgeTurnCancellationCause::UserRequested,
        TurnCancellationCause::RuntimeShutdown => BridgeTurnCancellationCause::RuntimeShutdown,
        TurnCancellationCause::AgentClosed => BridgeTurnCancellationCause::AgentClosed,
        TurnCancellationCause::Recovery => BridgeTurnCancellationCause::Recovery,
        TurnCancellationCause::Coalesced { target_turn_id } => {
            BridgeTurnCancellationCause::Coalesced { target_turn_id }
        }
    }
}

fn turn_rollover(value: TurnRolloverOutcome) -> BridgeTurnRolloverOutcome {
    match value {
        TurnRolloverOutcome::NotAttempted => BridgeTurnRolloverOutcome::NotAttempted,
        TurnRolloverOutcome::Succeeded => BridgeTurnRolloverOutcome::Succeeded,
        TurnRolloverOutcome::Failed { error } => BridgeTurnRolloverOutcome::Failed { error },
    }
}

fn turn_budget_limit(value: BudgetLimitSnapshot) -> BridgeTurnBudgetLimit {
    BridgeTurnBudgetLimit {
        kind: match value.kind {
            BudgetLimitKind::ModelStep => BridgeTurnBudgetLimitKind::ModelStep,
            BudgetLimitKind::ToolCall => BridgeTurnBudgetLimitKind::ToolCall,
            BudgetLimitKind::Wait => BridgeTurnBudgetLimitKind::Wait,
            BudgetLimitKind::WallClock => BridgeTurnBudgetLimitKind::WallClock,
            BudgetLimitKind::AgentCount => BridgeTurnBudgetLimitKind::AgentCount,
            BudgetLimitKind::AgentDepth => BridgeTurnBudgetLimitKind::AgentDepth,
            BudgetLimitKind::Finalization => BridgeTurnBudgetLimitKind::Finalization,
        },
        usage: BridgeTurnBudgetUsage {
            model_steps: value.usage.model_steps,
            tool_calls: value.usage.tool_calls,
            wait_calls: value.usage.wait_calls,
            elapsed_ms: value.usage.elapsed_ms,
        },
    }
}

fn bridge_turn_failure(value: pl_protocol::TurnFailure) -> BridgeTurnFailureDto {
    BridgeTurnFailureDto {
        category: format!("{:?}", value.category).to_ascii_lowercase(),
        provider_kind: value
            .provider_kind
            .map(|kind| format!("{kind:?}").to_ascii_lowercase()),
        code: value.code,
        http_status: value.http_status,
        message: value.message,
        retryable: value.retry.is_retryable(),
        retry_after_ms: value.retry.retry_after_ms(),
    }
}

fn turn_phase(value: TurnPhase) -> BridgeTurnPhase {
    match value {
        TurnPhase::Preparing => BridgeTurnPhase::Preparing,
        TurnPhase::Thinking => BridgeTurnPhase::Thinking,
        TurnPhase::Responding => BridgeTurnPhase::Responding,
        TurnPhase::Planning => BridgeTurnPhase::Planning,
        TurnPhase::RunningTool => BridgeTurnPhase::RunningTool,
        TurnPhase::Persisting => BridgeTurnPhase::Persisting,
    }
}

pub(crate) fn bridge_thread_item(value: ThreadItem) -> Result<Option<BridgeThreadItem>> {
    if matches!(value.state(), ThreadItemState::ContextCompaction(_)) {
        return Ok(None);
    }
    let state = item_state(value.state())?;
    Ok(Some(BridgeThreadItem {
        id: value.id,
        thread_id: value.thread_id,
        turn_id: value.turn_id,
        ordinal: value.ordinal,
        revision: value.revision,
        created_at: value.created_at,
        updated_at: value.updated_at,
        state,
    }))
}

fn item_state(value: &ThreadItemState) -> Result<BridgeThreadItemState> {
    Ok(match value {
        ThreadItemState::Text(value) => BridgeThreadItemState::Text {
            channel: match value.channel() {
                ThreadTextChannel::User => BridgeThreadTextChannel::User,
                ThreadTextChannel::Commentary => BridgeThreadTextChannel::Commentary,
                ThreadTextChannel::Final => BridgeThreadTextChannel::Final,
            },
            text: value.text().to_string(),
            attachments: value.attachments().iter().map(attachment).collect(),
            lifecycle: content_lifecycle(value.lifecycle()),
        },
        ThreadItemState::Thinking(value) => BridgeThreadItemState::Thinking {
            summary: value.summary().to_vec(),
            content: value.content().to_vec(),
            lifecycle: content_lifecycle(value.lifecycle()),
        },
        ThreadItemState::Tool(value) => BridgeThreadItemState::Tool {
            invocation: BridgeThreadToolInvocation {
                tool_call_id: value.invocation().tool_call_id().to_string(),
                call_id: value.invocation().call_id().map(str::to_string),
                provider_item_id: value.invocation().provider_item_id().map(str::to_string),
                name: value.invocation().name().to_string(),
                arguments: value.invocation().arguments().to_string(),
                working_directory: value.invocation().working_directory().map(str::to_string),
            },
            state: tool_state(value.state())?,
        },
        ThreadItemState::Agent(value) => {
            let identity = value.identity();
            BridgeThreadItemState::Agent {
                identity: BridgeThreadAgentIdentity {
                    id: identity.id().to_string(),
                    path: identity.path().to_string(),
                    parent_path: identity.parent_path().map(str::to_string),
                    role: identity.role().to_string(),
                    task: identity.task().to_string(),
                    depth: identity.depth(),
                },
                state: match value.state() {
                    ThreadAgentState::Queued(_) => BridgeThreadAgentState::Queued,
                    ThreadAgentState::Running(_) => BridgeThreadAgentState::Running,
                    ThreadAgentState::Succeeded(state) => BridgeThreadAgentState::Succeeded {
                        completed_at: state.completed_at(),
                        summary: state.summary().to_string(),
                    },
                    ThreadAgentState::Denied(state) => BridgeThreadAgentState::Denied {
                        denied_at: state.denied_at(),
                        reason: state.reason().to_string(),
                    },
                    ThreadAgentState::Cancelled(state) => BridgeThreadAgentState::Cancelled {
                        cancelled_at: state.cancelled_at(),
                        reason: state.reason().to_string(),
                    },
                    ThreadAgentState::Failed(state) => BridgeThreadAgentState::Failed {
                        failed_at: state.failed_at(),
                        error: state.error().to_string(),
                    },
                },
            }
        }
        ThreadItemState::Turn(value) => BridgeThreadItemState::Turn {
            state: bridge_turn_state(value.state()),
        },
        ThreadItemState::Inference(value) => BridgeThreadItemState::Inference {
            inference_id: value.inference_id().to_string(),
            model: value.model().to_string(),
            state: match value.state() {
                ThreadInferenceState::Running(_) => BridgeThreadInferenceState::Running,
                ThreadInferenceState::Completed(state) => BridgeThreadInferenceState::Completed {
                    completed_at: state.completed_at(),
                    usage: token_usage(state.usage().clone()),
                },
                ThreadInferenceState::Failed(state) => BridgeThreadInferenceState::Failed {
                    failed_at: state.failed_at(),
                    error: state.error().to_string(),
                },
                ThreadInferenceState::Cancelled(state) => BridgeThreadInferenceState::Cancelled {
                    cancelled_at: state.cancelled_at(),
                    reason: state.reason().to_string(),
                },
            },
        },
        ThreadItemState::Skill(value) => {
            let activation = value.activation();
            BridgeThreadItemState::Skill {
                name: activation.name.clone(),
                source: activation.source.clone(),
                provider_id: activation.provider_id.clone(),
                resource_base: match &activation.resource_base {
                    pl_protocol::SkillActivationResourceBase::Directory { path } => {
                        BridgeSkillResourceBase::Directory { path: path.clone() }
                    }
                    pl_protocol::SkillActivationResourceBase::Url { url } => {
                        BridgeSkillResourceBase::Url { url: url.clone() }
                    }
                    pl_protocol::SkillActivationResourceBase::Opaque { description } => {
                        BridgeSkillResourceBase::Opaque {
                            description: description.clone(),
                        }
                    }
                },
                cause: match &activation.cause {
                    pl_protocol::SkillActivationCause::Tool { tool_call_id } => {
                        BridgeSkillActivationCause::Tool {
                            tool_call_id: tool_call_id.clone(),
                        }
                    }
                    pl_protocol::SkillActivationCause::UserGesture { invocation_id } => {
                        BridgeSkillActivationCause::UserGesture {
                            invocation_id: invocation_id.clone(),
                        }
                    }
                },
                activated_at: activation.activated_at,
            }
        }
        ThreadItemState::File(value) => BridgeThreadItemState::File {
            path: value.path().to_string(),
            media_type: value.media_type().map(str::to_string),
            completed_at: value.completed_at(),
        },
        ThreadItemState::ContextCompaction(value) => BridgeThreadItemState::ContextCompaction {
            before_tokens: value.before_tokens(),
            after_tokens: value.after_tokens(),
            compacted_at: value.compacted_at(),
        },
    })
}

fn content_lifecycle(value: &ThreadContentLifecycle) -> BridgeThreadContentLifecycle {
    match value {
        ThreadContentLifecycle::Streaming(_) => BridgeThreadContentLifecycle::Streaming,
        ThreadContentLifecycle::Completed(state) => BridgeThreadContentLifecycle::Completed {
            completed_at: state.completed_at(),
        },
        ThreadContentLifecycle::Failed(state) => BridgeThreadContentLifecycle::Failed {
            failed_at: state.failed_at(),
            error: state.error().to_string(),
        },
        ThreadContentLifecycle::Cancelled(state) => BridgeThreadContentLifecycle::Cancelled {
            cancelled_at: state.cancelled_at(),
            reason: state.reason().to_string(),
        },
    }
}

fn attachment(value: &ThreadAttachment) -> BridgeThreadAttachment {
    BridgeThreadAttachment {
        id: value.id.clone(),
        modality: match value.modality {
            pl_protocol::AttachmentModality::Image => BridgeAttachmentModality::Image,
            pl_protocol::AttachmentModality::Video => BridgeAttachmentModality::Video,
            pl_protocol::AttachmentModality::File => BridgeAttachmentModality::File,
        },
        media_type: value.media_type.clone(),
        filename: value.filename.clone(),
        width: value.width,
        height: value.height,
        byte_size: value.byte_size,
    }
}

fn tool_state(value: &ThreadToolState) -> Result<BridgeThreadToolState> {
    Ok(match value {
        ThreadToolState::Started(_) => BridgeThreadToolState::Started,
        ThreadToolState::Streaming(_) => BridgeThreadToolState::Streaming,
        ThreadToolState::AwaitingApproval(_) => BridgeThreadToolState::AwaitingApproval,
        ThreadToolState::Approved(_) => BridgeThreadToolState::Approved,
        ThreadToolState::Running(state) => BridgeThreadToolState::Running {
            streamed_output: state.streamed_output().to_string(),
        },
        ThreadToolState::Succeeded(state) => BridgeThreadToolState::Succeeded {
            completed_at: state.completed_at(),
            output: tool_output(state.output())?,
        },
        ThreadToolState::Failed(state) => BridgeThreadToolState::Failed {
            failed_at: state.failed_at(),
            failure: BridgeThreadToolFailure {
                kind: match state.failure().kind() {
                    ThreadToolFailureKind::Execution => BridgeThreadToolFailureKind::Execution,
                    ThreadToolFailureKind::TimedOut => BridgeThreadToolFailureKind::TimedOut,
                    ThreadToolFailureKind::BudgetLimited => {
                        BridgeThreadToolFailureKind::BudgetLimited
                    }
                },
                message: state.failure().message().to_string(),
            },
            output: state.output().map(tool_output).transpose()?,
        },
        ThreadToolState::Denied(state) => BridgeThreadToolState::Denied {
            denied_at: state.denied_at(),
            reason: state.reason().to_string(),
        },
        ThreadToolState::Cancelled(state) => BridgeThreadToolState::Cancelled {
            cancelled_at: state.cancelled_at(),
            reason: state.reason().to_string(),
        },
    })
}

fn tool_output(value: &ThreadToolOutput) -> Result<BridgeThreadToolOutput> {
    Ok(BridgeThreadToolOutput {
        result: value.result().to_string(),
        attachments: value.attachments().iter().map(attachment).collect(),
        output_artifacts_json: value
            .output_artifacts()
            .iter()
            .map(|artifact| serde_json::to_string(artifact).map_err(Into::into))
            .collect::<Result<Vec<_>>>()?,
        exit_code: value.exit_code(),
    })
}

fn item_delta(value: ThreadItemDelta) -> BridgeThreadItemDelta {
    BridgeThreadItemDelta {
        item_id: value.item_id,
        revision: value.revision,
        delta: match value.delta {
            ThreadItemDeltaState::Text { delta } => BridgeThreadItemDeltaState::Text { delta },
            ThreadItemDeltaState::ThinkingSummary { chunk_index, delta } => {
                BridgeThreadItemDeltaState::ThinkingSummary { chunk_index, delta }
            }
            ThreadItemDeltaState::ThinkingContent { chunk_index, delta } => {
                BridgeThreadItemDeltaState::ThinkingContent { chunk_index, delta }
            }
            ThreadItemDeltaState::ToolArguments { delta } => {
                BridgeThreadItemDeltaState::ToolArguments { delta }
            }
            ThreadItemDeltaState::ToolResult { delta } => {
                BridgeThreadItemDeltaState::ToolResult { delta }
            }
        },
    }
}

fn runtime_snapshot(value: ThreadRuntimeSnapshot) -> BridgeThreadRuntimeSnapshot {
    BridgeThreadRuntimeSnapshot {
        thread_id: value.thread_id,
        usage: runtime_usage(value.usage),
        turn_completion_tokens: value.turn_completion_tokens,
        turn_decode_millis: value.turn_decode_millis,
        todo: value.todo.map(todo),
        active_skills: value.active_skills,
        active_mcp_servers: value.active_mcp_servers,
        active_lsp_servers: value.active_lsp_servers,
        progress: value.progress,
        mcp_health: value.mcp_health.map(mcp_health),
        workflow: value.workflow.map(workflow_runtime_snapshot),
        updated_at: value.updated_at,
    }
}

fn workflow_runtime_snapshot(value: WorkflowRuntimeSnapshot) -> BridgeWorkflowRuntimeSnapshot {
    BridgeWorkflowRuntimeSnapshot {
        revision: value.revision,
        current_run: value.current_run.map(workflow_run),
    }
}

fn workflow_run(value: WorkflowRuntimeRunSnapshot) -> BridgeWorkflowRun {
    BridgeWorkflowRun {
        lineage_id: value.lineage_id,
        run_id: value.run_id,
        mode_id: value.mode_id.to_string(),
        graph_revision: value.graph_revision,
        graph_hash: value.graph_hash,
        lifecycle: match value.lifecycle {
            WorkflowRunLifecycle::Active => BridgeWorkflowRunLifecycle::Active,
            WorkflowRunLifecycle::Terminal => BridgeWorkflowRunLifecycle::Terminal,
        },
        current_state_id: value.current_state_id,
        started_at: value.started_at,
        updated_at: value.updated_at,
    }
}

fn runtime_usage(value: ThreadRuntimeUsage) -> BridgeThreadRuntimeUsage {
    BridgeThreadRuntimeUsage {
        model: value.model,
        context_window: value.context_window,
        latest_context_tokens: value.latest_context_tokens,
        prompt_tokens: value.prompt_tokens,
        completion_tokens: value.completion_tokens,
        cached_prompt_tokens: value.cached_prompt_tokens,
        cache_write_tokens: value.cache_write_tokens,
        cache_miss_tokens: value.cache_miss_tokens,
        reasoning_tokens: value.reasoning_tokens,
        inference_count: value.inference_count,
        total_tokens: value.total_tokens,
        cache_hit_rate: value.cache_hit_rate,
        estimated_costs: value
            .estimated_costs
            .into_iter()
            .map(runtime_cost)
            .collect(),
        estimated_cache_savings: value
            .estimated_cache_savings
            .into_iter()
            .map(runtime_cost)
            .collect(),
        has_unpriced_usage: value.has_unpriced_usage,
        prompt_generation: value.prompt_generation,
        prompt_cache_policy: value.prompt_cache_policy,
        prefix_changed_reason: value
            .prefix_changed_reason
            .map(prompt_prefix_changed_reason),
        updated_at: value.updated_at,
    }
}

fn prompt_prefix_changed_reason(
    value: PromptPrefixChangedReason,
) -> BridgePromptPrefixChangedReason {
    match value {
        PromptPrefixChangedReason::Initial => BridgePromptPrefixChangedReason::Initial,
        PromptPrefixChangedReason::PromptScopeChanged => {
            BridgePromptPrefixChangedReason::PromptScopeChanged
        }
        PromptPrefixChangedReason::ProviderChanged => {
            BridgePromptPrefixChangedReason::ProviderChanged
        }
        PromptPrefixChangedReason::ModelChanged => BridgePromptPrefixChangedReason::ModelChanged,
        PromptPrefixChangedReason::BaseInstructionsChanged => {
            BridgePromptPrefixChangedReason::BaseInstructionsChanged
        }
        PromptPrefixChangedReason::GlobalInstructionsChanged => {
            BridgePromptPrefixChangedReason::GlobalInstructionsChanged
        }
        PromptPrefixChangedReason::ModeRoleChanged => {
            BridgePromptPrefixChangedReason::ModeRoleChanged
        }
        PromptPrefixChangedReason::SkillCatalogChanged => {
            BridgePromptPrefixChangedReason::SkillCatalogChanged
        }
        PromptPrefixChangedReason::WorkspaceInstructionsChanged => {
            BridgePromptPrefixChangedReason::WorkspaceInstructionsChanged
        }
        PromptPrefixChangedReason::RequestPropertiesChanged => {
            BridgePromptPrefixChangedReason::RequestPropertiesChanged
        }
        PromptPrefixChangedReason::FixedPrefixChanged => {
            BridgePromptPrefixChangedReason::FixedPrefixChanged
        }
        PromptPrefixChangedReason::ToolSchemaChanged => {
            BridgePromptPrefixChangedReason::ToolSchemaChanged
        }
        PromptPrefixChangedReason::ContextCompacted => {
            BridgePromptPrefixChangedReason::ContextCompacted
        }
        PromptPrefixChangedReason::ContextAppended => {
            BridgePromptPrefixChangedReason::ContextAppended
        }
        PromptPrefixChangedReason::ContextRecovered => {
            BridgePromptPrefixChangedReason::ContextRecovered
        }
    }
}

fn runtime_cost(value: RuntimeCostAmount) -> BridgeRuntimeCostAmount {
    BridgeRuntimeCostAmount {
        currency: value.currency,
        amount: value.amount,
    }
}

fn token_usage(value: TokenUsageSnapshot) -> BridgeTokenUsageSnapshot {
    BridgeTokenUsageSnapshot {
        prompt_tokens: value.prompt_tokens,
        completion_tokens: value.completion_tokens,
        cached_prompt_tokens: value.cached_prompt_tokens,
        total_tokens: value.total_tokens,
    }
}

fn todo(value: TodoListSnapshot) -> BridgeTodoListSnapshot {
    BridgeTodoListSnapshot {
        call_id: value.call_id,
        agent_id: value.agent_id,
        path: value.path,
        parent_path: value.parent_path,
        explanation: value.explanation,
        items: value.items.into_iter().map(todo_item).collect(),
    }
}

fn todo_item(value: TodoItem) -> BridgeTodoItem {
    BridgeTodoItem {
        step: value.step,
        status: match value.status {
            TodoStatus::Pending => BridgeTodoStatus::Pending,
            TodoStatus::InProgress => BridgeTodoStatus::InProgress,
            TodoStatus::Completed => BridgeTodoStatus::Completed,
        },
    }
}

fn mcp_health(value: McpHealthSnapshot) -> BridgeThreadMcpHealthSnapshot {
    BridgeThreadMcpHealthSnapshot {
        generation: value.generation,
        servers: value.servers.into_iter().map(mcp_availability).collect(),
    }
}

fn mcp_availability(value: McpAvailabilityDescriptor) -> BridgeThreadMcpAvailabilityDescriptor {
    BridgeThreadMcpAvailabilityDescriptor {
        server: mcp_server(value.server),
        availability: value.availability,
        message: value.message,
        last_checked_at: value.last_checked_at,
        tool_count: value.tool_count.and_then(|count| u64::try_from(count).ok()),
    }
}

fn mcp_server(value: McpServerDescriptor) -> BridgeThreadMcpServerDescriptor {
    BridgeThreadMcpServerDescriptor {
        id: value.id,
        source: value.source,
        transport: value.transport,
        endpoint: value.endpoint,
        built_in: value.built_in,
    }
}

#[cfg(test)]
mod tests {
    use pl_protocol::{
        RunningTurnState, SkillActivation, ThreadContextCompactionItem, ThreadSkillItem,
        ThreadTextItem, TurnPhase, TurnState,
    };

    use super::*;

    #[test]
    fn running_turn_phases_map_losslessly() {
        for (phase, bridged_phase) in [
            (TurnPhase::Preparing, BridgeTurnPhase::Preparing),
            (TurnPhase::Thinking, BridgeTurnPhase::Thinking),
            (TurnPhase::Responding, BridgeTurnPhase::Responding),
            (TurnPhase::Planning, BridgeTurnPhase::Planning),
            (TurnPhase::RunningTool, BridgeTurnPhase::RunningTool),
            (TurnPhase::Persisting, BridgeTurnPhase::Persisting),
        ] {
            let bridged = bridge_turn(running_turn("turn-1", phase));
            // 完整比较 BridgeTurn，确保六种 canonical phase 逐个无损映射，
            // 未来枚举变更仍由穷尽 match 暴露。
            assert_eq!(bridged, expected_bridge_turn("turn-1", phase));
            assert_eq!(
                bridged.state,
                BridgeTurnState::Running {
                    started_at: 1,
                    phase: bridged_phase,
                }
            );
        }
    }

    fn running_turn(id: &str, phase: TurnPhase) -> Turn {
        Turn {
            id: id.to_string(),
            thread_id: "thread-1".to_string(),
            revision: 1,
            state: TurnState::Running(RunningTurnState::new(1, phase)),
            updated_at: 1,
        }
    }

    fn expected_bridge_turn(id: &str, phase: TurnPhase) -> BridgeTurn {
        BridgeTurn {
            id: id.to_string(),
            thread_id: "thread-1".to_string(),
            revision: 1,
            state: BridgeTurnState::Running {
                started_at: 1,
                phase: turn_phase(phase),
            },
            updated_at: 1,
        }
    }

    #[test]
    fn internal_context_items_never_cross_the_bridge_boundary() {
        let context_compaction = item(ThreadItemState::ContextCompaction(
            ThreadContextCompactionItem::new(100, 25, 1),
        ));

        assert!(
            bridge_thread_item(context_compaction.clone())
                .unwrap()
                .is_none()
        );
        assert!(
            thread_notification(ThreadNotificationEnvelope {
                thread_id: "thread-1".to_string(),
                revision: 1,
                emitted_at: 1,
                notification: ThreadNotification::ItemCompleted {
                    item: Box::new(context_compaction.clone()),
                },
            })
            .unwrap()
            .is_none()
        );

        let mut snapshot = ThreadSnapshot::empty("thread-1");
        snapshot.items = vec![
            context_compaction,
            item(ThreadItemState::Text(ThreadTextItem::new(
                ThreadTextChannel::User,
                "visible".to_string(),
                Vec::new(),
                ThreadContentLifecycle::completed(1),
            ))),
        ];
        let bridged = bridge_thread_snapshot(snapshot).unwrap();
        assert_eq!(bridged.items.len(), 1);
    }

    #[test]
    fn snapshot_window_truncates_on_turn_boundaries_with_history_cursor() {
        let mut snapshot = ThreadSnapshot::empty("thread-1");
        snapshot.items = (0..500)
            .map(|ordinal| window_item(ordinal, format!("turn-{}", ordinal / 2)))
            .collect();
        let bridged = bridge_thread_snapshot(snapshot).unwrap();
        // 截断到窗口上限；锚点 Turn 的 items 完整保留，锚点即窗口首 Turn。
        assert_eq!(bridged.items.len(), 400);
        assert_eq!(bridged.history_cursor.as_deref(), Some("turn-50"));
        assert_eq!(bridged.items.first().unwrap().turn_id, "turn-50");
        assert_eq!(bridged.items.last().unwrap().turn_id, "turn-249");
    }

    #[test]
    fn small_snapshots_carry_no_history_cursor() {
        let mut snapshot = ThreadSnapshot::empty("thread-1");
        snapshot.items = vec![
            window_item(0, "turn-0".to_string()),
            window_item(1, "turn-1".to_string()),
        ];
        let bridged = bridge_thread_snapshot(snapshot).unwrap();
        assert_eq!(bridged.items.len(), 2);
        assert_eq!(bridged.history_cursor, None);
    }

    #[test]
    fn skill_item_crosses_the_bridge_as_typed_activation_data() {
        let bridged = bridge_thread_item(item(ThreadItemState::Skill(ThreadSkillItem::new(
            SkillActivation {
                name: "pdf".to_string(),
                source: "system".to_string(),
                provider_id: "local-filesystem".to_string(),
                resource_base: pl_protocol::SkillActivationResourceBase::Directory {
                    path: "/skills/pdf".to_string(),
                },
                turn_id: "turn-1".to_string(),
                cause: pl_protocol::SkillActivationCause::Tool {
                    tool_call_id: "tool-1".to_string(),
                },
                activated_at: 7,
            },
        ))))
        .unwrap()
        .unwrap();

        assert!(matches!(
            bridged.state,
            BridgeThreadItemState::Skill {
                name,
                source,
                cause: BridgeSkillActivationCause::Tool { tool_call_id },
                activated_at: 7,
                ..
            } if name == "pdf" && source == "system" && tool_call_id == "tool-1"
        ));
    }

    fn window_item(ordinal: u64, turn_id: String) -> ThreadItem {
        ThreadItem::new(
            format!("item-{ordinal}"),
            "thread-1".to_string(),
            turn_id,
            ordinal,
            1,
            1,
            1,
            ThreadItemState::Text(ThreadTextItem::new(
                ThreadTextChannel::User,
                format!("message {ordinal}"),
                Vec::new(),
                ThreadContentLifecycle::completed(1),
            )),
        )
    }

    fn item(state: ThreadItemState) -> ThreadItem {
        ThreadItem::new(
            "item-1".to_string(),
            "thread-1".to_string(),
            "turn-1".to_string(),
            1,
            1,
            1,
            1,
            state,
        )
    }
}
