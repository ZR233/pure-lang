mod interaction;

pub(crate) use interaction::interaction as bridge_interaction;

use anyhow::Result;
use pl_protocol::{
    AgentMessageChannel, McpAvailabilityDescriptor, McpHealthSnapshot, McpServerDescriptor,
    PromptPrefixChangedReason, RuntimeCostAmount, Thread, ThreadAttachment, ThreadItem,
    ThreadItemContent, ThreadItemDelta, ThreadItemDeltaField, ThreadItemStatus, ThreadMode,
    ThreadNotification, ThreadNotificationEnvelope, ThreadRuntimeSnapshot, ThreadRuntimeUsage,
    ThreadSnapshot, ThreadStatus, ThreadSubscriptionUpdate, ThreadToolCall, TodoItem,
    TodoListSnapshot, TodoStatus, TokenUsageSnapshot, Turn, TurnPhase, TurnState,
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
            turn: bridge_turn(value),
        },
        ThreadNotification::TurnUpdated { turn: value } => BridgeThreadNotification::TurnUpdated {
            turn: bridge_turn(value),
        },
        ThreadNotification::TurnCompleted { turn: value } => {
            BridgeThreadNotification::TurnCompleted {
                turn: bridge_turn(value),
            }
        }
        ThreadNotification::ItemStarted { item: value } => {
            let Some(item) = bridge_thread_item(*value)? else {
                return Ok(None);
            };
            BridgeThreadNotification::ItemStarted { item }
        }
        ThreadNotification::ItemDelta { delta } => BridgeThreadNotification::ItemDelta {
            delta: item_delta(delta),
        },
        ThreadNotification::ItemCompleted { item: value } => {
            let Some(item) = bridge_thread_item(*value)? else {
                return Ok(None);
            };
            BridgeThreadNotification::ItemCompleted { item }
        }
        ThreadNotification::InteractionChanged { interaction: value } => {
            BridgeThreadNotification::InteractionChanged {
                interaction: interaction::interaction(*value)?,
            }
        }
        ThreadNotification::ThreadRuntimeUpdated { runtime: value } => {
            BridgeThreadNotification::ThreadRuntimeUpdated {
                runtime: runtime_snapshot(*value),
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
        mode: match value.mode {
            ThreadMode::Simple => BridgeThreadMode::Simple,
            ThreadMode::Task => BridgeThreadMode::Task,
        },
        root_thread_id: value.root_thread_id,
        parent_thread_id: value.parent_thread_id,
        role: value.role,
        agent_path: value.agent_path,
        status: match value.status {
            ThreadStatus::Idle => BridgeThreadStatus::Idle,
            ThreadStatus::Running => BridgeThreadStatus::Running,
            ThreadStatus::Waiting => BridgeThreadStatus::Waiting,
            ThreadStatus::Completed => BridgeThreadStatus::Completed,
            ThreadStatus::Failed => BridgeThreadStatus::Failed,
            ThreadStatus::Closed => BridgeThreadStatus::Closed,
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
        state: match value.state {
            TurnState::Queued => BridgeTurnState::Queued,
            TurnState::InProgress { phase } => BridgeTurnState::InProgress {
                phase: turn_phase(phase),
            },
            TurnState::Completed => BridgeTurnState::Completed,
            TurnState::Failed { reason } => BridgeTurnState::Failed { reason },
            TurnState::Interrupted { reason } => BridgeTurnState::Interrupted { reason },
        },
        failure: value.failure.map(bridge_turn_failure),
        started_at: value.started_at,
        updated_at: value.updated_at,
        completed_at: value.completed_at,
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
    let Some(content) = item_content(value.content)? else {
        return Ok(None);
    };
    Ok(Some(BridgeThreadItem {
        id: value.id,
        thread_id: value.thread_id,
        turn_id: value.turn_id,
        ordinal: value.ordinal,
        revision: value.revision,
        status: item_status(value.status),
        created_at: value.created_at,
        updated_at: value.updated_at,
        completed_at: value.completed_at,
        error: value.error,
        content,
        usage: value.usage.map(token_usage),
    }))
}

fn item_content(value: ThreadItemContent) -> Result<Option<BridgeThreadItemContent>> {
    Ok(match value {
        ThreadItemContent::UserMessage { text, attachments } => {
            Some(BridgeThreadItemContent::UserMessage {
                text,
                attachments: attachments.into_iter().map(attachment).collect(),
            })
        }
        ThreadItemContent::AgentMessage { channel, text } => {
            Some(BridgeThreadItemContent::AgentMessage {
                channel: match channel {
                    AgentMessageChannel::Commentary => BridgeAgentMessageChannel::Commentary,
                    AgentMessageChannel::Final => BridgeAgentMessageChannel::Final,
                },
                text,
            })
        }
        ThreadItemContent::Reasoning { summary, content } => {
            Some(BridgeThreadItemContent::Reasoning { summary, content })
        }
        ThreadItemContent::Plan { content } => Some(BridgeThreadItemContent::Plan { content }),
        ThreadItemContent::ToolCall { tool } => Some(BridgeThreadItemContent::ToolCall {
            tool: tool_call(tool)?,
        }),
        ThreadItemContent::File { path, media_type } => {
            Some(BridgeThreadItemContent::File { path, media_type })
        }
        ThreadItemContent::ContextCompaction { .. } => None,
    })
}

fn attachment(value: ThreadAttachment) -> BridgeThreadAttachment {
    BridgeThreadAttachment {
        id: value.id,
        media_type: value.media_type,
        filename: value.filename,
        width: value.width,
        height: value.height,
        byte_size: value.byte_size,
        data_url: value.data_url,
    }
}

fn tool_call(value: ThreadToolCall) -> Result<BridgeThreadToolCall> {
    Ok(BridgeThreadToolCall {
        tool_call_id: value.tool_call_id,
        call_id: value.call_id,
        provider_item_id: value.provider_item_id,
        name: value.name,
        arguments: value.arguments,
        result: value.result,
        output_artifacts_json: value
            .output_artifacts
            .into_iter()
            .map(|artifact| serde_json::to_string(&artifact).map_err(Into::into))
            .collect::<Result<Vec<_>>>()?,
        exit_code: value.exit_code,
        timed_out: value.timed_out,
        working_directory: value.working_directory,
        denial_reason: value.denial_reason,
    })
}

fn item_status(value: ThreadItemStatus) -> BridgeThreadItemStatus {
    match value {
        ThreadItemStatus::Started => BridgeThreadItemStatus::Started,
        ThreadItemStatus::Streaming => BridgeThreadItemStatus::Streaming,
        ThreadItemStatus::AwaitingApproval => BridgeThreadItemStatus::AwaitingApproval,
        ThreadItemStatus::Approved => BridgeThreadItemStatus::Approved,
        ThreadItemStatus::Denied => BridgeThreadItemStatus::Denied,
        ThreadItemStatus::Running => BridgeThreadItemStatus::Running,
        ThreadItemStatus::Completed => BridgeThreadItemStatus::Completed,
        ThreadItemStatus::Failed => BridgeThreadItemStatus::Failed,
        ThreadItemStatus::Interrupted => BridgeThreadItemStatus::Interrupted,
        ThreadItemStatus::BudgetLimited => BridgeThreadItemStatus::BudgetLimited,
    }
}

fn item_delta(value: ThreadItemDelta) -> BridgeThreadItemDelta {
    BridgeThreadItemDelta {
        item_id: value.item_id,
        revision: value.revision,
        field: match value.field {
            ThreadItemDeltaField::Text => BridgeThreadItemDeltaField::Text,
            ThreadItemDeltaField::ReasoningSummary => BridgeThreadItemDeltaField::ReasoningSummary,
            ThreadItemDeltaField::ReasoningContent => BridgeThreadItemDeltaField::ReasoningContent,
            ThreadItemDeltaField::PlanContent => BridgeThreadItemDeltaField::PlanContent,
            ThreadItemDeltaField::ToolArguments => BridgeThreadItemDeltaField::ToolArguments,
            ThreadItemDeltaField::ToolResult => BridgeThreadItemDeltaField::ToolResult,
        },
        delta: value.delta,
        chunk_index: value.chunk_index,
    }
}

fn runtime_snapshot(value: ThreadRuntimeSnapshot) -> BridgeThreadRuntimeSnapshot {
    BridgeThreadRuntimeSnapshot {
        thread_id: value.thread_id,
        usage: runtime_usage(value.usage),
        todo: value.todo.map(todo),
        active_skills: value.active_skills,
        active_mcp_servers: value.active_mcp_servers,
        active_lsp_servers: value.active_lsp_servers,
        progress: value.progress,
        mcp_health: value.mcp_health.map(mcp_health),
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
    use super::*;

    #[test]
    fn internal_context_items_never_cross_the_bridge_boundary() {
        let context_compaction = item(ThreadItemContent::ContextCompaction {
            before_tokens: 100,
            after_tokens: 25,
            compacted_at: 1,
        });

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
            item(ThreadItemContent::UserMessage {
                text: "visible".to_string(),
                attachments: Vec::new(),
            }),
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

    fn window_item(ordinal: u64, turn_id: String) -> ThreadItem {
        ThreadItem {
            id: format!("item-{ordinal}"),
            thread_id: "thread-1".to_string(),
            turn_id,
            ordinal,
            revision: 1,
            status: ThreadItemStatus::Completed,
            created_at: 1,
            updated_at: 1,
            completed_at: Some(1),
            error: None,
            content: ThreadItemContent::UserMessage {
                text: format!("message {ordinal}"),
                attachments: Vec::new(),
            },
            usage: None,
        }
    }

    fn item(content: ThreadItemContent) -> ThreadItem {
        ThreadItem {
            id: "item-1".to_string(),
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            ordinal: 1,
            revision: 1,
            status: ThreadItemStatus::Completed,
            created_at: 1,
            updated_at: 1,
            completed_at: Some(1),
            error: None,
            content,
            usage: None,
        }
    }
}
