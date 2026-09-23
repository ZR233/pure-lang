//! Live projection of the committed effect window for realtime notifications.
//!
//! A subscription cannot replay the durable checkpoint: it is deliberately compact and no longer
//! holds finished attempts, turns or deliveries. Instead each committed `ThreadEffectBatch` is
//! folded into a runnable owner snapshot, and the canonical `project_effect_items` turns
//! that effect into items. Live notifications and durable history therefore stay one projection of
//! the same effect instead of a second, drifting fact source.

use std::collections::{BTreeMap, BTreeSet};

use pl_core::{
    chat::Session,
    context::ContextSnapshot,
    thread::{
        RequestAttempt, ThreadEffectBatch, ThreadSnapshot, TurnRecord, TurnState,
        input::InputChange, journal::AttemptUpdate, journal::ContextChange,
    },
};
use pl_protocol::{
    InteractionRequest, ThreadContentLifecycle, ThreadItem, ThreadItemDelta, ThreadItemDeltaState,
    ThreadItemState, ThreadRuntimeSnapshot, ThreadTextChannel, ThreadTextItem, ThreadThinkingItem,
    ThreadToolState, Turn,
};

use super::ProjectionError;

/// Upper bounds for the short replay window a live subscription retains.
///
/// A subscription is a bounded observation channel: it keeps only the facts an in-flight effect
/// can still reference, the newest terminal items it must classify for `itemDelta` continuity, and
/// the streaming preview identity. Anything older is read from history on reconnect, so these
/// windows never grow with the Thread's history.
const LIVE_TURN_WINDOW: usize = 1024;
const LIVE_ITEM_WINDOW: usize = 1024;
const LIVE_CONTEXT_RECORD_WINDOW: usize = 4096;

/// Which lifecycle notification a projected Turn produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::studio) enum TurnEvent {
    Started,
    Updated,
    Completed,
}

/// One live change projected from a single committed effect.
pub(in crate::studio) enum LiveEvent {
    Turn {
        turn: Turn,
        event: TurnEvent,
    },
    /// 权威条目按 identity 整体替换；条目本身较大（含完整正文/载荷），装箱后与其它变体保持
    /// 同一量级，避免每个有界实时帧都为最大变体付出整块栈/队列内存。
    Item(Box<ThreadItem>),
    Delta(ThreadItemDelta),
    Interaction(Box<InteractionRequest>),
    Runtime(Box<ThreadRuntimeSnapshot>),
}

/// Folds committed effects into a runnable owner state and yields the live changes each one made.
pub(in crate::studio) struct LiveProjection {
    state: ThreadSnapshot,
    items: BTreeMap<String, ThreadItem>,
    turns_seen: BTreeSet<String>,
    preview_attempt: Option<String>,
    preview_payloads: BTreeSet<String>,
}

impl LiveProjection {
    /// Seeds the projection from the current owner snapshot.
    ///
    /// The shared session assigns new orders before either preview or persistence.
    pub(in crate::studio) fn seed(state: ThreadSnapshot) -> Self {
        Self {
            state,
            items: BTreeMap::new(),
            turns_seen: BTreeSet::new(),
            preview_attempt: None,
            preview_payloads: BTreeSet::new(),
        }
    }

    /// Applies one committed effect and returns the live changes it produced.
    ///
    /// Identity alignment is two-pass, exactly like the history writer: the effect is projected
    /// once to discover the item identities it touches, those identities are resolved against the
    /// durable phase and reserved in the shared session, and the effect is projected again with
    /// that phase. A live item therefore carries the ordinal the durable item will have, so window
    /// eviction, resubscription, lagged replay and multiple subscribers never renumber it.
    ///
    /// # Errors
    /// Returns a projection failure when the effect references facts the folded window lost; the
    /// caller turns that into `lagged` so the client resynchronizes instead of splicing a hole.
    pub(in crate::studio) async fn advance(
        &mut self,
        history: &crate::studio::storage::history::HistoryStore,
        chat: &Session,
        usage: &pl_core::thread::UsageSummary,
        thread: &pl_protocol::Thread,
        effect: &ThreadEffectBatch,
    ) -> Result<Vec<LiveEvent>, ProjectionError> {
        self.apply(effect);
        let provisional = super::project_effect_items(
            thread,
            &self.state,
            effect,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )?;
        let (mut existing, mut reserved) = self
            .resolve_phase(
                history,
                chat,
                provisional.items.iter().map(|item| item.id.clone()),
            )
            .await?;
        // A Turn can reference the input id its consuming commit pruned from the folded live state.
        // Only its already durable item can complete that reference, and reading it must not reserve
        // an ordinal for an identity this effect does not project.
        if !provisional.unresolved_inputs.is_empty() {
            let mut missing = Vec::new();
            for id in &provisional.unresolved_inputs {
                match chat
                    .read_item(id)
                    .await
                    .map_err(|error| ProjectionError::History(error.to_string()))?
                {
                    Some(item) => {
                        let decoded = serde_json::from_str(&item.body)
                            .map_err(|error| ProjectionError::History(error.to_string()))?;
                        existing.entry(id.clone()).or_insert(decoded);
                    }
                    None => missing.push(id.clone()),
                }
            }
            for (id, item) in history
                .existing_items(missing)
                .await
                .map_err(|error| ProjectionError::History(error.to_string()))?
            {
                existing.entry(id).or_insert(item);
            }
        }
        // Only already-started channels may be finalized by this effect.
        if let Some(attempt) = &effect.attempt {
            let channels = ["reasoning", "text"]
                .map(|channel| super::attempt_channel_id(&attempt.attempt_id, channel));
            for id in channels {
                if let Some(ordinal) = chat.assigned_order(&id) {
                    reserved.entry(id).or_insert(ordinal);
                }
            }
        }
        let projected =
            super::project_effect_items(thread, &self.state, effect, &existing, &reserved)?;
        // 实时帧同样不允许带着无法补全的 identity 继续：投影失败由调用方转成 lagged 重同步。
        projected.ensure_complete()?;
        let mut events = Vec::new();
        for item in projected.items {
            chat.publish(
                crate::studio::storage::history::chat_item(item.clone(), false)
                    .map_err(|error| ProjectionError::History(error.to_string()))?,
            )
            .map_err(|error| ProjectionError::History(error.to_string()))?;
            events.extend(self.record(item)?);
        }
        if let Some(record) = &effect.turn {
            let event = if record.state == TurnState::Running {
                if self.turns_seen.insert(record.turn_id.clone()) {
                    TurnEvent::Started
                } else {
                    TurnEvent::Updated
                }
            } else {
                self.turns_seen.insert(record.turn_id.clone());
                TurnEvent::Completed
            };
            let turn = super::turns::project_turn(
                &thread.id,
                &self.state,
                record,
                effect.committed_at,
                effect.committed_at,
                effect.sequence,
            )?;
            events.push(LiveEvent::Turn { turn, event });
        }
        for record in effect.interactions.iter() {
            if let Ok(Some(interaction)) = crate::thread_assembler::project_thread_interaction(
                &thread.id,
                &record.request.id,
                &self.state,
            ) {
                events.push(LiveEvent::Interaction(Box::new(interaction)));
            }
        }
        for permission in effect.permissions.iter() {
            if let Ok(Some(interaction)) = crate::thread_assembler::project_thread_interaction(
                &thread.id,
                &permission.id,
                &self.state,
            ) {
                events.push(LiveEvent::Interaction(Box::new(interaction)));
            }
        }
        if effect.attempt.is_some() || effect.runtime_facts.is_some() || effect.lifecycle.is_some()
        {
            // 该 effect 自己的累计事实也在本地折叠一次，使实时 runtime 帧与 writer 落库后的
            // checkpoint 摘要完全一致，而不是等 writer 追上才更新。
            let mut summary = usage.clone();
            if super::runtime::fold_effect_accounting(&mut summary, effect).is_ok()
                && let Ok(runtime) = super::runtime::project_runtime(
                    &thread.id,
                    &self.state,
                    effect.committed_at,
                    &summary,
                )
            {
                events.push(LiveEvent::Runtime(Box::new(runtime)));
            }
        }
        // 该 effect 自己的事实已经投影完，不再需要由它消费/收束的历史记录。
        self.prune_consumed_facts();
        Ok(events)
    }

    /// Resolves the authoritative ordinal phase for one effect's item identities.
    ///
    /// Already written identities keep the durable payload they were assigned; identities this
    /// effect introduces are allocated by the shared session, and the writer uses the same order.
    async fn resolve_phase(
        &self,
        history: &crate::studio::storage::history::HistoryStore,
        chat: &Session,
        ids: impl IntoIterator<Item = String>,
    ) -> Result<(BTreeMap<String, ThreadItem>, BTreeMap<String, u64>), ProjectionError> {
        let ids = ids.into_iter().collect::<Vec<_>>();
        // Once every identity is owned by this session, streaming updates need no
        // database round trip. The durable lookup below remains necessary for an
        // identity encountered for the first time after reopening a session.
        let assigned = ids
            .iter()
            .map(|id| {
                self.items
                    .contains_key(id)
                    .then(|| chat.assigned_order(id))
                    .flatten()
                    .map(|order| (id.clone(), order))
            })
            .collect::<Option<Vec<_>>>();
        if let Some(assigned) = assigned {
            let existing = ids
                .into_iter()
                .filter_map(|id| self.items.get(&id).cloned().map(|item| (id, item)))
                .collect();
            return Ok((existing, assigned.into_iter().collect()));
        }
        let mut existing = history
            .existing_items(ids.clone())
            .await
            .map_err(|error| ProjectionError::History(error.to_string()))?;
        let mut reserved = BTreeMap::new();
        let missing = ids
            .into_iter()
            .filter(|id| !reserved.contains_key(id) && !existing.contains_key(id))
            .collect::<Vec<_>>();
        for id in missing {
            let order = chat
                .reserve_order(&id)
                .await
                .map_err(|error| ProjectionError::History(error.to_string()))?;
            reserved.insert(id, order);
        }
        for (id, item) in &self.items {
            existing.entry(id.clone()).or_insert_with(|| item.clone());
        }
        Ok((existing, reserved))
    }

    /// Drops the facts this effect already consumed from the folded live state.
    ///
    /// Consumed messages, resolved interactions, finished tasks and their permissions are durable
    /// history facts owned by the writers; a subscription that kept them would grow with the
    /// Thread's history. Pruning runs only after the effect that consumed them was projected,
    /// because that same effect is the one that still needed them (a consumed message is reported
    /// through the state entry it just left).
    fn prune_consumed_facts(&mut self) {
        let consumed = self.state.consumed_messages;
        if self
            .state
            .inbox
            .iter()
            .any(|record| record.sequence <= consumed)
        {
            self.state.inbox = self
                .state
                .inbox
                .iter()
                .filter(|record| record.sequence > consumed)
                .cloned()
                .collect::<Vec<_>>()
                .into();
        }
        self.state.interactions.retain(|_, record| {
            record.state == pl_core::thread::interactions::InteractionState::Pending
        });
        // 未交付的调用结果、仍在运行的 Turn 与仍在运行的任务是折叠状态里唯一还需要保留的
        // 执行事实；其余（含其许可）都是历史。
        let live_turns = self
            .state
            .turns
            .iter()
            .map(|turn| turn.turn_id.as_str())
            .collect::<BTreeSet<_>>();
        let undelivered = self
            .state
            .deliveries
            .iter()
            .map(|delivery| delivery.call_id.as_str())
            .collect::<BTreeSet<_>>();
        self.state.tasks.retain(|_, record| {
            record.status == pl_core::thread::task::TaskStatus::Running
                || undelivered.contains(record.call_id.as_str())
                || live_turns.contains(record.turn_id.as_str())
        });
        let live_tasks = self
            .state
            .tasks
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        self.state
            .permissions
            .retain(|_, record| live_tasks.contains(record.task_id.as_str()));
        // 上下文替换只属于写者的历史投影；折叠状态不再保存它们。
        self.state.context_replacements = Default::default();
    }

    /// Projects the ephemeral streaming preview the owner publishes between committed effects.
    ///
    /// Streaming text and reasoning never appear in an effect batch, so their `delta` frames come
    /// from the owner's live overlay. The projection keeps following the same item identity the
    /// committed effect later finalizes.
    ///
    /// `applied` is the subscription's canonical committed-effect watermark, owned by the live
    /// subscription that drives this projection; `LiveProjection` never keeps a second copy of it.
    /// A preview therefore carries the revision of the last committed effect it was projected
    /// behind — strictly below the revision of the effect that will finalize the same identity — so
    /// it can never block the terminal payload and never invents a tick-local counter the durable
    /// item does not share.
    pub(in crate::studio) async fn stream(
        &mut self,
        history: &crate::studio::storage::history::HistoryStore,
        chat: &Session,
        thread: &pl_protocol::Thread,
        state: &ThreadSnapshot,
        applied: u64,
    ) -> Result<Vec<LiveEvent>, ProjectionError> {
        let Some(preview) = state.model_progress.as_ref() else {
            self.preview_attempt = None;
            self.preview_payloads.clear();
            return Ok(Vec::new());
        };
        let Some(attempt) = state
            .attempts
            .iter()
            .find(|attempt| attempt.attempt_id == preview.attempt_id)
        else {
            return Ok(Vec::new());
        };
        let at = crate::studio::unix_seconds();
        let mut events = Vec::new();
        if !preview.progress.presentation.is_empty() {
            if self.preview_attempt.as_deref() != Some(preview.attempt_id.as_str()) {
                self.preview_attempt = Some(preview.attempt_id.clone());
                self.preview_payloads.clear();
            }
            let previous_payloads = std::mem::take(&mut self.preview_payloads);
            for channel in ["text", "reasoning"] {
                let id = super::order::response_id(&attempt.attempt_id, channel);
                if self
                    .items
                    .get(&id)
                    .is_some_and(|item| !item.state().is_terminal())
                {
                    self.items.remove(&id);
                    chat.drop_preview(&id);
                }
            }
            for payload in &preview.progress.presentation {
                let content = payload.content();
                self.preview_payloads.insert(content.to_owned());
                if previous_payloads.contains(content) {
                    continue;
                }
                if payload.format() != "pl.model.presentation-item" || payload.version() != 1 {
                    return Err(ProjectionError::UnsupportedOutput(
                        "unsupported model presentation preview".into(),
                    ));
                }
                let item: pl_model::completion::CompletionPresentationItem =
                    serde_json::from_str(payload.content())
                        .map_err(|error| ProjectionError::UnsupportedOutput(error.to_string()))?;
                for part in item
                    .parts
                    .iter()
                    .map(Some)
                    .chain(item.parts.is_empty().then_some(None))
                {
                    let id = super::order::presentation_id(
                        &attempt.attempt_id,
                        &item.provider_item_id,
                        part.map(super::order::presentation_part),
                    );
                    if self
                        .items
                        .get(&id)
                        .is_some_and(|previous| previous.state().is_terminal())
                    {
                        continue;
                    }
                    let state = match (item.kind, part.map(|part| part.kind)) {
                        (
                            pl_model::completion::CompletionPresentationItemKind::Text(channel),
                            None,
                        )
                        | (
                            pl_model::completion::CompletionPresentationItemKind::Text(channel),
                            Some(pl_model::completion::CompletionPresentationPartKind::OutputText),
                        ) => {
                            let channel = match channel {
                                pl_protocol::trace::TraceTextChannel::User => {
                                    ThreadTextChannel::User
                                }
                                pl_protocol::trace::TraceTextChannel::Commentary => {
                                    ThreadTextChannel::Commentary
                                }
                                pl_protocol::trace::TraceTextChannel::Final => {
                                    ThreadTextChannel::Final
                                }
                            };
                            ThreadItemState::Text(ThreadTextItem::new(
                                channel,
                                part.map_or_else(String::new, |part| part.text.clone()),
                                Vec::new(),
                                ThreadContentLifecycle::streaming(),
                            ))
                        }
                        (pl_model::completion::CompletionPresentationItemKind::Reasoning, None)
                        | (
                            pl_model::completion::CompletionPresentationItemKind::Reasoning,
                            Some(
                                pl_model::completion::CompletionPresentationPartKind::ReasoningText,
                            ),
                        ) => ThreadItemState::Thinking(ThreadThinkingItem::new(
                            Vec::new(),
                            part.map_or_else(Vec::new, |part| vec![part.text.clone()]),
                            ThreadContentLifecycle::streaming(),
                        )),
                        (
                            pl_model::completion::CompletionPresentationItemKind::Reasoning,
                            Some(pl_model::completion::CompletionPresentationPartKind::SummaryText),
                        ) => ThreadItemState::Thinking(ThreadThinkingItem::new(
                            part.map_or_else(Vec::new, |part| vec![part.text.clone()]),
                            Vec::new(),
                            ThreadContentLifecycle::streaming(),
                        )),
                        _ => {
                            return Err(ProjectionError::UnsupportedOutput(
                                "provider presentation part does not match its item".into(),
                            ));
                        }
                    };
                    if self
                        .items
                        .get(&id)
                        .is_some_and(|previous| previous.state() == &state)
                    {
                        continue;
                    }
                    let order = chat
                        .reserve_order(&id)
                        .await
                        .map_err(|error| ProjectionError::History(error.to_string()))?;
                    let revision = self.preview_revision(&id, applied);
                    let created_at = self.created_at(&id, at);
                    let projected = ThreadItem::new(
                        id,
                        thread.id.clone(),
                        attempt.turn_id.clone(),
                        order,
                        revision,
                        created_at,
                        at,
                        state,
                    );
                    chat.publish_preview(
                        crate::studio::storage::history::chat_item(projected.clone(), false)
                            .map_err(|error| ProjectionError::History(error.to_string()))?,
                    )
                    .map_err(|error| ProjectionError::History(error.to_string()))?;
                    events.extend(self.record(projected)?);
                }
            }
            return Ok(events);
        }
        let reasoning = preview.progress.reasoning.as_ref().and_then(|payload| {
            (payload.format() == "text/plain" && payload.version() == 1)
                .then(|| payload.content().to_owned())
        });
        let text = super::content::text_content(&preview.progress.content);
        let reasoning = reasoning.filter(|text| !text.is_empty());
        // The streaming preview and its terminal item share an in-memory identity assignment.
        let reasoning_id = super::order::response_id(&attempt.attempt_id, "reasoning");
        let text_id = super::order::response_id(&attempt.attempt_id, "text");
        let mut ids = Vec::new();
        if reasoning.is_some() {
            ids.push(reasoning_id.clone());
        }
        if !text.is_empty() {
            ids.push(text_id.clone());
        }
        let (existing, reserved) = self.resolve_phase(history, chat, ids).await?;
        if let Some(reasoning) = reasoning {
            let id = reasoning_id;
            let revision = self.preview_revision(&id, applied);
            let created_at = self.created_at(&id, at);
            let ordinal = phase_ordinal(&existing, &reserved, &id);
            let item = ThreadItem::new(
                id,
                thread.id.clone(),
                attempt.turn_id.clone(),
                ordinal,
                revision,
                created_at,
                at,
                ThreadItemState::Thinking(ThreadThinkingItem::new(
                    Vec::new(),
                    vec![reasoning],
                    ThreadContentLifecycle::streaming(),
                )),
            );
            chat.publish_preview(
                crate::studio::storage::history::chat_item(item.clone(), false)
                    .map_err(|error| ProjectionError::History(error.to_string()))?,
            )
            .map_err(|error| ProjectionError::History(error.to_string()))?;
            events.extend(self.record(item)?);
        }
        if !text.is_empty() {
            let id = text_id;
            let revision = self.preview_revision(&id, applied);
            let created_at = self.created_at(&id, at);
            let ordinal = phase_ordinal(&existing, &reserved, &id);
            let item = ThreadItem::new(
                id,
                thread.id.clone(),
                attempt.turn_id.clone(),
                ordinal,
                revision,
                created_at,
                at,
                ThreadItemState::Text(ThreadTextItem::new(
                    ThreadTextChannel::Commentary,
                    text,
                    Vec::new(),
                    ThreadContentLifecycle::streaming(),
                )),
            );
            chat.publish_preview(
                crate::studio::storage::history::chat_item(item.clone(), false)
                    .map_err(|error| ProjectionError::History(error.to_string()))?,
            )
            .map_err(|error| ProjectionError::History(error.to_string()))?;
            events.extend(self.record(item)?);
        }
        Ok(events)
    }

    /// Records one projected payload and keeps its revision strictly monotonic.
    ///
    /// The ordinal and the revision both come from the canonical projection: the ordinal is the
    /// durable assignment resolved by the caller, and the revision is the effect sequence that
    /// produced this payload, which is exactly what the history writer stores. Live frames,
    /// resubscriptions, `lagged` replay and the final SQL item therefore agree on the same
    /// revision instead of a second, tick-local counter. A stale payload is ignored rather than
    /// renumbered.
    ///
    /// # Errors
    /// Returns [`ProjectionError::ItemOrder`] when the caller did not resolve an ordinal.
    fn record(&mut self, item: ThreadItem) -> Result<Vec<LiveEvent>, ProjectionError> {
        if item.ordinal == 0 {
            return Err(ProjectionError::ItemOrder(item.id));
        }
        let previous = self.items.get(&item.id).cloned();
        if let Some(previous) = &previous
            && previous.revision > item.revision
        {
            return Ok(Vec::new());
        }
        let events = classify(previous.as_ref(), &item);
        self.items.insert(item.id.clone(), item);
        Ok(events)
    }

    /// Canonical revision of an uncommitted streaming preview.
    ///
    /// The preview is not yet a committed effect, so its revision must stay at or below the
    /// revision the finalizing effect will store. Using the newest applied commit sequence (and
    /// never a tick-local counter) keeps every preview frame `<=` the durable terminal revision, so
    /// the client's `incoming.revision >= existing.revision` merge always accepts the terminal.
    fn preview_revision(&self, id: &str, applied: u64) -> u64 {
        self.items
            .get(id)
            .map_or(applied, |item| item.revision.max(applied))
    }

    fn created_at(&self, id: &str, fallback: i64) -> i64 {
        self.items.get(id).map_or(fallback, |item| item.created_at)
    }

    /// Folds the effect's exported facts into the runnable state so later effects can reference
    /// attempts, Turns, inputs and deliveries that the compact checkpoint already dropped.
    fn apply(&mut self, effect: &ThreadEffectBatch) {
        if let Some(change) = &effect.context {
            match change {
                ContextChange::Append { revision, records } => {
                    let mut next = self.state.context.records.to_vec();
                    next.extend(records.iter().cloned());
                    self.state.context = ContextSnapshot {
                        revision: *revision,
                        records: next.into(),
                    };
                }
                ContextChange::Replace(snapshot) => self.state.context = snapshot.clone(),
            }
        }
        if let Some(update) = &effect.attempt {
            let attempt = attempt_from(update, &self.state.context);
            let mut attempts = self.state.attempts.to_vec();
            match attempts
                .iter()
                .position(|previous| previous.attempt_id == attempt.attempt_id)
            {
                Some(index) => attempts[index] = attempt,
                None => attempts.push(attempt),
            }
            self.state.attempts = attempts.into();
        }
        if let Some(turn) = &effect.turn {
            upsert_turn(&mut self.state.turns, turn);
        }
        for change in effect.inputs.iter() {
            match change {
                InputChange::Accepted(record) => {
                    let mut inputs = self.state.inputs.to_vec();
                    match inputs
                        .iter()
                        .position(|previous| previous.input.id == record.input.id)
                    {
                        Some(index) => inputs[index] = record.clone(),
                        None => inputs.push(record.clone()),
                    }
                    self.state.inputs = inputs.into();
                }
                InputChange::Transition {
                    id,
                    revision,
                    state,
                } => {
                    let mut inputs = self.state.inputs.to_vec();
                    if let Some(index) = inputs.iter().position(|previous| &previous.input.id == id)
                    {
                        inputs[index].revision = *revision;
                        inputs[index].state = state.clone();
                    }
                    self.state.inputs = inputs.into();
                }
            }
        }
        for record in effect.inbox.iter() {
            let mut inbox = self.state.inbox.to_vec();
            if !inbox
                .iter()
                .any(|previous| previous.message.id == record.message.id)
            {
                inbox.push(record.clone());
            }
            self.state.inbox = inbox.into();
        }
        for task in effect.tasks.iter() {
            self.state.tasks.insert(task.id.clone(), task.clone());
        }
        for permission in effect.permissions.iter() {
            self.state
                .permissions
                .insert(permission.id.clone(), permission.clone());
        }
        for interaction in effect.interactions.iter() {
            self.state
                .interactions
                .insert(interaction.request.id.clone(), interaction.clone());
        }
        for delivery in effect.deliveries.iter() {
            let mut deliveries = self.state.deliveries.to_vec();
            match deliveries
                .iter()
                .position(|previous| previous.call_id == delivery.call_id)
            {
                Some(index) => deliveries[index] = delivery.clone(),
                None => deliveries.push(delivery.clone()),
            }
            self.state.deliveries = deliveries.into();
        }
        if let Some(facts) = &effect.runtime_facts {
            self.state.runtime_facts = facts.clone();
        }
        for change in effect.extensions.iter() {
            match change {
                pl_core::thread::extensions::ExtensionChange::Put { id, record } => {
                    self.state.extensions.insert(id.clone(), record.clone());
                }
                pl_core::thread::extensions::ExtensionChange::Delete { id, .. } => {
                    self.state.extensions.remove(id);
                }
            }
        }
        for replacement in effect.replacements.iter() {
            let mut next = self.state.context_replacements.to_vec();
            next.push(replacement.clone());
            self.state.context_replacements = next.into();
        }
        if let Some(through) = effect.consumed_messages {
            self.state.consumed_messages = through;
        }
        if let Some(through) = effect.wake_messages_through {
            self.state.wake_messages_through = through;
        }
        self.retain_live();
    }

    /// Bounds the folded facts to the window a later live effect can still reference.
    ///
    /// A subscription outlives many Turns, so without this the folded snapshot would keep every
    /// attempt, delivery, context record and item the Thread ever produced. Running facts and the
    /// newest terminal tail stay; anything older is a durable history fact a reconnect reads from
    /// SQL, never an entity kept resident for the whole session.
    fn retain_live(&mut self) {
        let keep_turns = self
            .state
            .turns
            .iter()
            .rev()
            .take(LIVE_TURN_WINDOW)
            .map(|turn| turn.turn_id.clone())
            .collect::<BTreeSet<_>>();
        if self.state.turns.len() > keep_turns.len() {
            self.state.turns = self
                .state
                .turns
                .iter()
                .filter(|turn| keep_turns.contains(&turn.turn_id))
                .cloned()
                .collect::<Vec<_>>()
                .into();
        }
        let running_turns = self
            .state
            .turns
            .iter()
            .map(|turn| turn.turn_id.as_str())
            .collect::<BTreeSet<_>>();
        if self.state.attempts.len() > LIVE_TURN_WINDOW {
            self.state.attempts = self
                .state
                .attempts
                .iter()
                .filter(|attempt| running_turns.contains(attempt.turn_id.as_str()))
                .cloned()
                .collect::<Vec<_>>()
                .into();
        }
        if self.state.attempts.len() > LIVE_TURN_WINDOW {
            let mut newest = self
                .state
                .attempts
                .iter()
                .rev()
                .take(LIVE_TURN_WINDOW)
                .cloned()
                .collect::<Vec<_>>();
            newest.reverse();
            self.state.attempts = newest.into();
        }
        if self.state.inputs.len() > LIVE_TURN_WINDOW {
            self.state.inputs = self
                .state
                .inputs
                .iter()
                .filter(|record| record.state == pl_core::thread::input::InputState::Pending)
                .cloned()
                .collect::<Vec<_>>()
                .into();
        }
        if self.state.deliveries.len() > LIVE_ITEM_WINDOW {
            self.state.deliveries = self
                .state
                .deliveries
                .iter()
                .rev()
                .take(LIVE_ITEM_WINDOW)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .into();
        }
        if self.state.context.records.len() > LIVE_CONTEXT_RECORD_WINDOW {
            let excess = self.state.context.records.len() - LIVE_CONTEXT_RECORD_WINDOW;
            self.state.context.records = self.state.context.records[excess..].to_vec().into();
        }
        // Turn lifecycle memory is part of the same bounded window: an identity whose Turn left
        // the retained window cannot be classified again, so it is not kept either.
        self.turns_seen
            .retain(|turn_id| keep_turns.contains(turn_id));
        let item_count = self.items.len();
        if item_count > LIVE_ITEM_WINDOW {
            let keep = self
                .items
                .values()
                .filter(|item| !item.state().is_terminal())
                .map(|item| item.id.clone())
                .collect::<BTreeSet<_>>();
            let newest = self
                .items
                .values()
                .rev()
                .take(LIVE_ITEM_WINDOW)
                .map(|item| item.id.clone())
                .collect::<BTreeSet<_>>();
            self.items
                .retain(|id, _| keep.contains(id) || newest.contains(id));
        }
    }
}

/// Ordinal of one identity: the durable item's ordinal when it exists, else the reservation.
fn phase_ordinal(
    existing: &BTreeMap<String, ThreadItem>,
    reserved: &BTreeMap<String, u64>,
    id: &str,
) -> u64 {
    existing.get(id).map_or_else(
        || reserved.get(id).copied().unwrap_or(0),
        |item| item.ordinal,
    )
}

fn attempt_from(update: &AttemptUpdate, context: &ContextSnapshot) -> RequestAttempt {
    RequestAttempt {
        request_metadata: update.request_metadata.clone(),
        tool_projection: update.tool_projection.clone(),
        turn_id: update.turn_id.clone(),
        attempt_id: update.attempt_id.clone(),
        retry_of: update.retry_of.clone(),
        // Projection never reads the request context; only its revision is carried forward.
        input: ContextSnapshot {
            revision: update.input_revision,
            records: context.records.clone(),
        },
        tools: update.tools.clone(),
        outcome: update.outcome.clone(),
        input_estimate: update.input_estimate,
    }
}

fn upsert_turn(turns: &mut std::sync::Arc<[TurnRecord]>, turn: &TurnRecord) {
    let mut next = turns.to_vec();
    match next
        .iter()
        .position(|previous| previous.turn_id == turn.turn_id)
    {
        Some(index) => next[index] = turn.clone(),
        None => next.push(turn.clone()),
    }
    *turns = next.into();
}

/// Classifies one re-projected item against the last payload already handed to the client.
///
/// A `delta` is only emitted when the canonical revision advanced by exactly one, which is what
/// makes `itemDelta` frames provably continuous for the client. Any other change (a preview at the
/// same revision, or a jump caused by an effect the client did not project) is sent as the full
/// authoritative non-terminal payload, which the client merges by identity and revision.
fn classify(previous: Option<&ThreadItem>, next: &ThreadItem) -> Vec<LiveEvent> {
    let Some(previous) = previous else {
        return vec![LiveEvent::Item(Box::new(next.clone()))];
    };
    if previous.kind() != next.kind() || previous.turn_id != next.turn_id {
        return vec![LiveEvent::Item(Box::new(next.clone()))];
    }
    if next.state().is_terminal() {
        if previous.state().is_terminal() && previous.state() == next.state() {
            return Vec::new();
        }
        return vec![LiveEvent::Item(Box::new(next.clone()))];
    }
    if previous.state().is_terminal() {
        // A terminal item is never regressed by a stale non-terminal payload.
        return Vec::new();
    }
    if previous.state() == next.state() {
        return Vec::new();
    }
    if next.revision == previous.revision.saturating_add(1) {
        let deltas = append_deltas(previous, next);
        if !deltas.is_empty() {
            return deltas.into_iter().map(LiveEvent::Delta).collect();
        }
    }
    // Same-revision preview or a revision jump: the authoritative payload replaces the overlay.
    vec![LiveEvent::Item(Box::new(next.clone()))]
}

/// A single appended field of one item, or several when more than one field grew at once.
///
/// Exactly one field may be sent as a `delta`; a same-item payload cannot carry two `delta`
/// frames at one revision, so any multi-field growth falls back to a full non-terminal upsert.
fn append_deltas(previous: &ThreadItem, next: &ThreadItem) -> Vec<ThreadItemDelta> {
    let mut states = Vec::new();
    match (previous.state(), next.state()) {
        (ThreadItemState::Text(previous), ThreadItemState::Text(next)) => {
            if let Some(delta) = appended(previous.text(), next.text()) {
                states.push(ThreadItemDeltaState::Text {
                    delta: delta.to_owned(),
                });
            }
        }
        (ThreadItemState::Thinking(previous), ThreadItemState::Thinking(next)) => {
            for (chunk_index, delta) in chunk_appends(previous.summary(), next.summary()) {
                states.push(ThreadItemDeltaState::ThinkingSummary { chunk_index, delta });
            }
            for (chunk_index, delta) in chunk_appends(previous.content(), next.content()) {
                states.push(ThreadItemDeltaState::ThinkingContent { chunk_index, delta });
            }
        }
        (ThreadItemState::Tool(previous), ThreadItemState::Tool(next)) => {
            if let Some(delta) = appended(
                previous.invocation().arguments(),
                next.invocation().arguments(),
            ) {
                states.push(ThreadItemDeltaState::ToolArguments {
                    delta: delta.to_owned(),
                });
            }
            if let Some(delta) =
                appended(tool_streamed(previous.state()), tool_streamed(next.state()))
            {
                states.push(ThreadItemDeltaState::ToolResult {
                    delta: delta.to_owned(),
                });
            }
        }
        _ => {}
    }
    let [delta] = states.as_slice() else {
        return Vec::new();
    };
    vec![ThreadItemDelta {
        item_id: next.id.clone(),
        revision: next.revision,
        delta: delta.clone(),
    }]
}

/// Appended bytes when `next` strictly extends `previous`; otherwise no growth.
fn appended<'a>(previous: &str, next: &'a str) -> Option<&'a str> {
    (next.len() > previous.len() && next.starts_with(previous)).then(|| &next[previous.len()..])
}

/// Appended chunk content per index for a chunk vector that only grows forward.
fn chunk_appends(previous: &[String], next: &[String]) -> Vec<(u32, String)> {
    let mut appends = Vec::new();
    for (index, chunk) in next.iter().enumerate() {
        let base = previous.get(index).map_or("", String::as_str);
        if let Some(delta) = appended(base, chunk) {
            appends.push((index as u32, delta.to_owned()));
        }
    }
    appends
}

fn tool_streamed(state: &ThreadToolState) -> &str {
    match state {
        ThreadToolState::Running(value) => value.streamed_output(),
        ThreadToolState::Cancelling(value) => value.streamed_output(),
        _ => "",
    }
}
