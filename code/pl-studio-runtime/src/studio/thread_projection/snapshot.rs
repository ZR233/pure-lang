//! Authoritative current snapshots and independently persisted history projections.
use super::{ProjectionError, project_active_turn, project_items};
use pl_core::thread::{
    ThreadEffectBatch, ThreadLifecycle, ThreadSnapshot, TurnState, input::InputState,
};
use pl_protocol::{Thread, ThreadStatus};
use std::sync::Arc;

pub(in crate::studio) fn project_snapshot(
    mut thread: Thread,
    state: &ThreadSnapshot,
    usage: &pl_core::thread::UsageSummary,
) -> Result<pl_protocol::ThreadSnapshot, ProjectionError> {
    let active_turn = project_active_turn(&thread.id, state, thread.updated_at)?;
    let mut interactions = Vec::new();
    for id in state.permissions.keys().chain(state.interactions.keys()) {
        if let Ok(Some(interaction)) =
            crate::thread_assembler::project_thread_interaction(&thread.id, id, state)
        {
            interactions.push(interaction);
        }
    }
    thread.status = status(state);
    if let Ok(Some(mode)) = saved_mode(state) {
        thread.mode = mode;
    }
    let runtime = super::runtime::project_runtime(&thread.id, state, thread.updated_at, usage).ok();
    Ok(pl_protocol::ThreadSnapshot {
        schema_version: pl_protocol::THREAD_SCHEMA_VERSION,
        revision: state.commit_sequence,
        runtime,
        thread,
        active_turn,
        interactions,
    })
}

/// Independently persisted history items for one Thread.
///
/// This is the one-way legacy-history projection: `state` must be a durable session state (the
/// migration export replays the old journal into one) and `journal` the committed effect window
/// that state was captured with. Live product reads take their items from the durable history
/// database, so this projection is not part of the normal observation path.
pub(in crate::studio) fn project_history_items(
    thread: &Thread,
    state: &ThreadSnapshot,
    journal: &[Arc<ThreadEffectBatch>],
) -> Result<Vec<pl_protocol::ThreadItem>, ProjectionError> {
    let mut items = project_items(
        &thread.id,
        thread.parent_thread_id.as_deref(),
        state,
        journal,
    )?;
    let recorded_at = journal.last().map_or(0, |commit| commit.committed_at);
    for id in state.permissions.keys().chain(state.interactions.keys()) {
        if let Err(error) =
            crate::thread_assembler::project_thread_interaction(&thread.id, id, state)
        {
            let payloads = state
                .permissions
                .get(id)
                .map(|record| &record.payload)
                .or_else(|| {
                    state
                        .interactions
                        .get(id)
                        .map(|record| &record.request.payload)
                })
                .into_iter()
                .map(super::raw_payload)
                .collect();
            append_raw_notice(
                &mut items,
                thread,
                RawNotice {
                    id: format!("raw-interaction:{id}"),
                    payloads,
                    notice: error.to_string(),
                    revision: state.commit_sequence,
                    recorded_at,
                },
            );
        }
    }
    if let Err(error) = saved_mode(state) {
        append_raw_notice(
            &mut items,
            thread,
            RawNotice {
                id: "raw-mode".into(),
                payloads: state
                    .extensions
                    .get("studio.mode")
                    .map(|record| super::raw_payload(&record.payload))
                    .into_iter()
                    .collect(),
                notice: error.to_string(),
                revision: state.commit_sequence,
                recorded_at,
            },
        );
    }
    // 历史投影只把 runtime 投影当作可解码性检查；累计使用量由 writer 折叠进 checkpoint 摘要，
    // 这里读同一份持久载体，不重新聚合历史集合。
    if let Err(error) =
        super::runtime::project_runtime(&thread.id, state, recorded_at, &state.usage_summary)
    {
        let mut payloads = state
            .extensions
            .values()
            .map(|record| super::raw_payload(&record.payload))
            .collect::<Vec<_>>();
        for attempt in state.attempts.iter() {
            if let Some(payload) = &attempt.request_metadata {
                payloads.push(super::raw_payload(payload));
            }
        }
        append_raw_notice(
            &mut items,
            thread,
            RawNotice {
                id: format!("raw-runtime:{}", state.commit_sequence),
                payloads,
                notice: error.to_string(),
                revision: state.commit_sequence,
                recorded_at: journal
                    .last()
                    .map_or(thread.updated_at, |commit| commit.committed_at),
            },
        );
    }
    Ok(items)
}

struct RawNotice {
    id: String,
    payloads: Vec<pl_protocol::ThreadRawPayload>,
    notice: String,
    revision: u64,
    recorded_at: i64,
}
fn append_raw_notice(items: &mut Vec<pl_protocol::ThreadItem>, thread: &Thread, record: RawNotice) {
    items.push(pl_protocol::ThreadItem::new(
        record.id,
        thread.id.clone(),
        String::new(),
        items
            .last()
            .map_or(1, |item| item.ordinal.saturating_add(1)),
        record.revision,
        record.recorded_at,
        record.recorded_at,
        pl_protocol::ThreadItemState::Raw(pl_protocol::ThreadRawItem {
            payloads: record.payloads,
            notice: record.notice,
            recorded_at: record.recorded_at,
        }),
    ));
}

pub(in crate::studio) fn status(state: &ThreadSnapshot) -> ThreadStatus {
    match state.lifecycle {
        ThreadLifecycle::Closing => ThreadStatus::Closing,
        ThreadLifecycle::Closed => ThreadStatus::Closed,
        ThreadLifecycle::Open => {
            if matches!(
                state.input_execution,
                pl_core::thread::input::InputExecution::Interrupting { .. }
            ) {
                return ThreadStatus::Cancelling;
            }
            if matches!(
                state.input_execution,
                pl_core::thread::input::InputExecution::Failed { .. }
            ) {
                return ThreadStatus::Faulted;
            }
            if state.interactions.values().any(|record| {
                record.state == pl_core::thread::interactions::InteractionState::Pending
            }) || state.permissions.values().any(|record| {
                record.state == pl_core::thread::permissions::PermissionState::Pending
            }) {
                ThreadStatus::WaitingInteraction
            } else if state
                .turns
                .iter()
                .any(|turn| turn.state == TurnState::Running)
            {
                ThreadStatus::Running
            } else if state
                .inputs
                .iter()
                .any(|input| input.state == InputState::Pending)
            {
                ThreadStatus::Queued
            } else {
                ThreadStatus::Idle
            }
        }
    }
}

pub(in crate::studio) fn saved_mode(
    state: &ThreadSnapshot,
) -> Result<Option<pl_protocol::ThreadModeId>, ProjectionError> {
    state
        .extensions
        .get("studio.mode")
        .map(|record| {
            if record.payload.format() != "pl.studio.mode" || record.payload.version() != 1 {
                return Err(ProjectionError::UnsupportedOutput(
                    "unsupported saved Mode".into(),
                ));
            }
            Ok(serde_json::from_str(record.payload.content())?)
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::thread_projection::runtime;
    use pl_core::{
        context::{ContextContent, OpaquePayload},
        model::{
            DynModelSession, ModelError, ModelRequest, ModelSession, ModelStepOutput,
            PreparedModelCall,
        },
        thread::{
            ThreadHandle,
            input::{QueuedTurn, ThreadInput},
        },
    };
    use pretty_assertions::assert_eq;

    /// Cumulative summary the projection must consume for a raw core snapshot.
    ///
    /// The owner keeps only unfinished attempts, so the summary is folded from the durable effects
    /// the writer would have folded; the tests use the same fold instead of re-aggregating
    /// `state.attempts` the way the projection no longer does.
    fn summary_of(state: &ThreadSnapshot) -> pl_core::thread::UsageSummary {
        let mut summary = pl_core::thread::UsageSummary::default();
        runtime::fold_state_for_test(&mut summary, state).expect("fold succeeds");
        summary
    }

    /// Folds committed effects into the durable state consumed by history projection.
    ///
    /// History projection intentionally consumes durable session facts plus the effect window, not a
    /// pruned live snapshot. The fold below mirrors the current effect contract directly; it does not
    /// enter the legacy one-way replay path.
    fn durable_state(effects: &[Arc<ThreadEffectBatch>]) -> ThreadSnapshot {
        let mut state = ThreadSnapshot::default();
        for effect in effects {
            apply_effect(&mut state, effect);
        }
        state
    }

    fn apply_effect(state: &mut ThreadSnapshot, effect: &ThreadEffectBatch) {
        use pl_core::thread::{RequestAttempt, input::InputChange, journal};

        if let Some(change) = &effect.context {
            state.context = match change {
                journal::ContextChange::Append { revision, records } => {
                    let mut next = state.context.records.to_vec();
                    next.extend(records.iter().cloned());
                    pl_core::context::ContextSnapshot {
                        revision: *revision,
                        records: next.into(),
                    }
                }
                journal::ContextChange::Replace(snapshot) => snapshot.clone(),
            };
        }
        if let Some(change) = &effect.private_context {
            state.private_context = match change {
                journal::PrivateContextChange::Set(payload) => Some(payload.clone()),
                journal::PrivateContextChange::Clear => None,
            };
        }
        if let Some(update) = &effect.attempt {
            let attempt = RequestAttempt {
                request_metadata: update.request_metadata.clone(),
                tool_projection: update.tool_projection.clone(),
                turn_id: update.turn_id.clone(),
                attempt_id: update.attempt_id.clone(),
                retry_of: update.retry_of.clone(),
                input: pl_core::context::ContextSnapshot {
                    revision: update.input_revision,
                    records: state.context.records.clone(),
                },
                tools: update.tools.clone(),
                outcome: update.outcome.clone(),
                input_estimate: update.input_estimate,
            };
            let mut attempts = state.attempts.to_vec();
            match attempts
                .iter()
                .position(|previous| previous.attempt_id == attempt.attempt_id)
            {
                Some(index) => attempts[index] = attempt,
                None => attempts.push(attempt),
            }
            state.attempts = attempts.into();
        }
        if let Some(turn) = &effect.turn {
            let mut turns = state.turns.to_vec();
            match turns
                .iter()
                .position(|previous| previous.turn_id == turn.turn_id)
            {
                Some(index) => turns[index] = turn.clone(),
                None => turns.push(turn.clone()),
            }
            state.turns = turns.into();
        }
        for change in effect.inputs.iter() {
            match change {
                InputChange::Accepted(record) => {
                    let mut inputs = state.inputs.to_vec();
                    match inputs
                        .iter()
                        .position(|previous| previous.input.id == record.input.id)
                    {
                        Some(index) => inputs[index] = record.clone(),
                        None => inputs.push(record.clone()),
                    }
                    state.inputs = inputs.into();
                }
                InputChange::Transition {
                    id,
                    revision,
                    state: input_state,
                } => {
                    let mut inputs = state.inputs.to_vec();
                    if let Some(index) = inputs.iter().position(|previous| &previous.input.id == id)
                    {
                        inputs[index].revision = *revision;
                        inputs[index].state = input_state.clone();
                    }
                    state.inputs = inputs.into();
                }
            }
        }
        for record in effect.inbox.iter() {
            let mut inbox = state.inbox.to_vec();
            if !inbox
                .iter()
                .any(|previous| previous.message.id == record.message.id)
            {
                inbox.push(record.clone());
            }
            state.inbox = inbox.into();
            // The admission sequence stays monotonic across consumption pruning, so the durable
            // fold keeps the same high watermark the live snapshot carries.
            state.inbox_sequence = state.inbox_sequence.max(record.sequence);
        }
        for task in effect.tasks.iter() {
            state.tasks.insert(task.id.clone(), task.clone());
        }
        for permission in effect.permissions.iter() {
            state
                .permissions
                .insert(permission.id.clone(), permission.clone());
        }
        for interaction in effect.interactions.iter() {
            state
                .interactions
                .insert(interaction.request.id.clone(), interaction.clone());
        }
        for delivery in effect.deliveries.iter() {
            let mut deliveries = state.deliveries.to_vec();
            match deliveries
                .iter()
                .position(|previous| previous.call_id == delivery.call_id)
            {
                Some(index) => deliveries[index] = delivery.clone(),
                None => deliveries.push(delivery.clone()),
            }
            state.deliveries = deliveries.into();
        }
        if let Some(facts) = &effect.runtime_facts {
            state.runtime_facts = facts.clone();
        }
        if let Some(tools) = &effect.discovered_tools {
            state.discovered_tools = tools.clone();
        }
        for change in effect.extensions.iter() {
            match change {
                pl_core::thread::extensions::ExtensionChange::Put { id, record } => {
                    state.extensions.insert(id.clone(), record.clone());
                }
                pl_core::thread::extensions::ExtensionChange::Delete { id, .. } => {
                    state.extensions.remove(id);
                }
            }
            state.extension_sequence = state.extension_sequence.saturating_add(1);
        }
        for replacement in effect.replacements.iter() {
            let mut replacements = state.context_replacements.to_vec();
            replacements.push(replacement.clone());
            state.context_replacements = replacements.into();
        }
        if let Some(through) = effect.consumed_messages {
            state.consumed_messages = through;
        }
        if let Some(through) = effect.wake_messages_through {
            state.wake_messages_through = through;
        }
        if let Some(lifecycle) = effect.lifecycle {
            state.lifecycle = lifecycle;
        }
        state.commit_sequence = effect.sequence;
    }

    struct Reply;
    impl ModelSession for Reply {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            Ok(PreparedModelCall::new(async move {
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: vec![ContextContent::Text {
                        text: Arc::from("  answer\r\n"),
                    }],
                    tool_calls: Vec::new(),
                    private_context: None,
                    usage: pl_core::model::ModelUsage {
                        input_tokens: Some(10),
                        output_tokens: Some(4),
                        cache_read_tokens: Some(8),
                        ..Default::default()
                    },
                })
            }))
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn input_turn_and_response_projection_matches_effect_fold_without_calling_current_services()
     {
        let handle = ThreadHandle::start("thread".into(), DynModelSession::new(Reply)).unwrap();
        handle.submit_input(ThreadInput {
            id: "input".into(),
            payload: OpaquePayload::new("pl.studio.prompt", 1, serde_json::json!({"text":"  question\r\n", "presentation":"visible", "attachments":[]}).to_string()).unwrap(),
            context: vec![ContextContent::Text { text: Arc::from("  question\r\n") }],
        }).await.unwrap();
        handle
            .run_next_input(QueuedTurn {
                turn_id: "turn".into(),
                attempt_prefix: "request".into(),
                max_model_steps: pl_core::thread::ModelStepLimit::Limited(
                    std::num::NonZeroU32::new(2).unwrap(),
                ),
                cancellation: Default::default(),
            })
            .await
            .unwrap();
        let effects = handle.effects().await.unwrap();
        let state = durable_state(&effects);
        let summary = summary_of(&state);
        let product = project_snapshot(Thread::placeholder("thread"), &state, &summary).unwrap();
        let items =
            project_history_items(&Thread::placeholder("thread"), &state, &effects).unwrap();
        assert_eq!(product.thread.status, ThreadStatus::Idle);
        assert!(product.active_turn.is_none());
        // 活动 owner 的当前快照要给出同样的产品结构：实时与重建来源不同，终态一致。
        let live =
            project_snapshot(Thread::placeholder("thread"), &handle.snapshot(), &summary).unwrap();
        assert_eq!(live.thread.status, product.thread.status);
        assert_eq!(live.active_turn, product.active_turn);
        assert_eq!(
            items.len(),
            4,
            "input, Turn, inference and response all remain visible"
        );
        assert_eq!(items[0].id, "input");
        assert_eq!(items[0].turn_id, "turn");
        let pl_protocol::ThreadItemState::Turn(turn) = items[1].state() else {
            panic!("Turn item");
        };
        assert_eq!(turn.input_id(), Some("input"));
        assert!(turn.state().is_terminal());
        let pl_protocol::ThreadItemState::Text(text) = items[3].state() else {
            panic!("answer item");
        };
        assert_eq!(text.text(), "  answer\r\n");
        let usage = &product.runtime.as_ref().unwrap().usage;
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.total_tokens, 14);
        assert_eq!(usage.cache_usage.input_tokens, 10);
        assert_eq!(usage.cache_usage.cache_read_tokens, 8);
        assert_eq!(usage.cache_usage.hit_rate, Some(0.8));
        assert!(usage.has_unpriced_usage);
        handle.close().await.unwrap();
        let closed_state = handle.snapshot();
        let closed = project_snapshot(
            Thread::placeholder("thread"),
            &closed_state,
            &summary_of(&closed_state),
        )
        .unwrap();
        assert_eq!(closed.thread.status, ThreadStatus::Closed);
    }
    #[tokio::test]
    async fn parent_inbox_messages_remain_visible_before_consumption_and_after_effect_fold() {
        use pl_core::thread::{TurnInput, inbox::ThreadMessage};
        use pl_protocol::{ThreadItemState, ThreadTextChannel};
        use tokio::sync::Semaphore;

        struct GatedReply {
            entered: Arc<Semaphore>,
            release: Arc<Semaphore>,
        }
        impl ModelSession for GatedReply {
            async fn prepare(
                &mut self,
                request: ModelRequest,
            ) -> Result<PreparedModelCall, ModelError> {
                let reply = Reply.prepare(request).await?;
                let entered = self.entered.clone();
                let release = self.release.clone();
                Ok(PreparedModelCall::new(async move {
                    entered.add_permits(1);
                    release.acquire().await.unwrap().forget();
                    reply.execute().await
                }))
            }
            async fn close(&mut self) -> Result<(), ModelError> {
                Ok(())
            }
        }
        async fn product(
            handle: &ThreadHandle,
        ) -> (pl_protocol::ThreadSnapshot, Vec<pl_protocol::ThreadItem>) {
            let mut thread = Thread::placeholder("child");
            thread.parent_thread_id = Some("parent".into());
            let effects = handle.effects().await.unwrap();
            // 父对话与历史项都来自持久事实；实时快照只用于产品结构断言。
            let state = durable_state(&effects);
            (
                project_snapshot(thread.clone(), &state, &summary_of(&state)).unwrap(),
                project_history_items(&thread, &state, &effects).unwrap(),
            )
        }
        fn messages(items: &[pl_protocol::ThreadItem]) -> Vec<pl_protocol::ThreadItem> {
            items.iter().filter(|item| matches!(item.state(),
                ThreadItemState::Text(text) if text.channel() == ThreadTextChannel::ParentAgent
            )).cloned().collect()
        }
        let entered = Arc::new(Semaphore::new(0));
        let release = Arc::new(Semaphore::new(0));
        let handle = ThreadHandle::start(
            "child".into(),
            DynModelSession::new(GatedReply {
                entered: entered.clone(),
                release: release.clone(),
            }),
        )
        .unwrap();
        let message = |id: &str, source: &str, text: &str| ThreadMessage {
            id: id.into(),
            source_id: source.into(),
            payload: OpaquePayload::text(text),
            context: vec![ContextContent::Text {
                text: Arc::from(text),
            }],
        };
        let initial = message("initial", "agent:parent", "  初始任务\r\n完整正文  ");
        let sequence = handle.send_message(initial.clone()).await.unwrap();
        assert_eq!(handle.send_message(initial).await.unwrap(), sequence);
        for (id, source) in [("notice", "studio.notification"), ("other", "agent:other")] {
            handle
                .send_message(message(id, source, "内部消息"))
                .await
                .unwrap();
        }
        let pending = messages(&product(&handle).await.1);
        assert_eq!(
            pending.len(),
            1,
            "accepted parent message must be visible before any Turn"
        );
        assert_eq!(
            pending[0].text().map(|text| text.text()),
            Some("  初始任务\r\n完整正文  ")
        );
        assert_eq!(pending[0].turn_id, "");

        for index in 0..3 {
            let running = handle.clone();
            let turn_id = format!("turn-{index}");
            let expected_turn = turn_id.clone();
            let task = tokio::spawn(async move {
                running
                    .run_turn(TurnInput {
                        turn_id,
                        attempt_prefix: format!("attempt-{index}"),
                        content: Vec::new(),
                        max_model_steps: pl_core::thread::ModelStepLimit::Unlimited,
                        cancellation: Default::default(),
                    })
                    .await
                    .unwrap()
            });
            tokio::time::timeout(std::time::Duration::from_secs(5), entered.acquire())
                .await
                .unwrap()
                .unwrap()
                .forget();
            let consumed = messages(&product(&handle).await.1);
            assert_eq!(consumed[index].turn_id, expected_turn);
            if index == 0 {
                assert_eq!(consumed[0].id, pending[0].id);
                assert_eq!(consumed[0].ordinal, pending[0].ordinal);
                assert_eq!(consumed[0].created_at, pending[0].created_at);
                handle
                    .send_message(message("steer", "agent:parent", "运行中补充"))
                    .await
                    .unwrap();
                let current = messages(&product(&handle).await.1);
                assert_eq!(current.len(), 2);
                assert_eq!(current[1].turn_id, "");
            }
            release.add_permits(1);
            task.await.unwrap();
            if index == 1 {
                handle
                    .send_message(message("resume", "agent:parent", "完成后续接"))
                    .await
                    .unwrap();
                assert_eq!(messages(&product(&handle).await.1).len(), 3);
            }
        }
        handle.close().await.unwrap();
        let (closed, closed_items) = product(&handle).await;
        let visible = messages(&closed_items);
        assert_eq!(
            visible
                .iter()
                .map(|item| item.text().unwrap().text())
                .collect::<Vec<_>>(),
            ["  初始任务\r\n完整正文  ", "运行中补充", "完成后续接"]
        );
        assert!(
            visible
                .windows(2)
                .all(|pair| pair[0].ordinal < pair[1].ordinal)
        );
        assert_eq!(closed.thread.status, ThreadStatus::Closed);
        assert!(closed.active_turn.is_none());
        // 没有父 agent 身份的根视图不能投影出父对话。
        let effects = handle.effects().await.unwrap();
        let durable = durable_state(&effects);
        let root = Thread::placeholder("child");
        let root_items = project_history_items(&root, &durable, &effects).unwrap();
        assert!(
            messages(&root_items).is_empty(),
            "a root cannot have parent dialogue"
        );
    }

    #[tokio::test]
    async fn unknown_input_codec_remains_exactly_readable_after_effect_fold() {
        let handle =
            ThreadHandle::start("raw-history".into(), DynModelSession::new(Reply)).unwrap();
        let original = "  {\"n\":9007199254740993123456789}\r\n原文\0<script>literal</script>  ";
        handle
            .submit_input(ThreadInput {
                id: "unknown-input".into(),
                payload: OpaquePayload::new("future.studio.prompt", 99, original).unwrap(),
                context: Vec::new(),
            })
            .await
            .unwrap();
        handle.close().await.unwrap();
        let effects = handle.effects().await.unwrap();
        let durable = durable_state(&effects);
        let items =
            project_history_items(&Thread::placeholder("raw-history"), &durable, &effects).unwrap();
        assert_eq!(items.len(), 1);
        let pl_protocol::ThreadItemState::Raw(raw) = items[0].state() else {
            panic!("unknown codecs must remain readable as raw data");
        };
        assert_eq!(
            raw.payloads,
            vec![pl_protocol::ThreadRawPayload {
                format: "future.studio.prompt".into(),
                version: 99,
                content: original.into(),
            }]
        );
        assert!(
            durable.context.records.is_empty(),
            "opaque input must not become model context"
        );
    }
}
