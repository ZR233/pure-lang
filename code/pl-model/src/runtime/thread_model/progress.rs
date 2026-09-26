//! Live observation of provider parts, independent of canonical completion accumulation.
//!
//! The source events already carry increments: `BlockDelta` is one chunk and `PresentationItem` is
//! the provider's authoritative text for an item that just closed. Neither is accumulated into a
//! full-text preview: every update is attached to a stable observation identity, advances that
//! identity's content version and shares the immutable block observed before it.
//!
//! A block event carries the provider item/part identity the adapter read from the source event, so
//! a delta that arrives *after* an earlier item closed is still attributed to its own item part
//! instead of being dropped or mixed into another channel. Raw reasoning is a part like any other:
//! an adapter that knows its item streams `ReasoningText` for it, and one that reports no provider
//! boundaries at all (chat-style `reasoning_content`) streams the request-scoped reasoning channel
//! the terminal item is keyed by. An aggregate is only ever that fallback, never a stand-in for a
//! provider item part.
//!
//! The part list itself lives in the published [`ModelProgress`] snapshot, not in a second copy the
//! projection would have to clone on every event. The projection owns the only live snapshot, so it
//! edits the stored list in place while the watch still holds its only reference; the moment a
//! consumer has cloned a snapshot the shared list reproduces only the chunk the edit lands in (plus
//! the small chunk index when it edits an already-filled chunk), which is what keeps that snapshot
//! immutable. A response with many parts therefore appends one chunk per event without ever copying
//! the whole list; a snapshot a consumer still holds makes the producer copy at most the chunk it
//! shares, so this removes the per-event whole-list copy — it does not promise that no copying ever
//! happens.
use crate::completion::CompletionPresentationPartKind;
use crate::completion::stream::ToolStream;
use crate::completion::stream::event::{
    ModelBlockContent, ModelBlockKind, ModelStreamEvent, ProviderBlockIdentity,
};
use pl_core::chat::PresentationPart;
use pl_core::model::{
    AggregateChannel, ModelParts, ModelProgress, ModelProgressSender, ModelTextChannel,
    ObservedItemKind, ObservedPart, ObservedPartIdentity, ObservedPartKind, ProviderPartIdentity,
};
use pl_protocol::PureError;
use pl_protocol::trace::TraceTextChannel;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Identity of one observation inside the current preview.
///
/// A provider row is keyed by exactly what `chat::presentation_item_id` reserves — item id, part
/// kind and content index — so a late channel/output-index correction can never duplicate the same
/// terminal identity into two observations.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum PartKey {
    Aggregate(AggregateChannel),
    Provider {
        item_id: Arc<str>,
        part: ObservedPartKind,
        content_index: u32,
    },
}

/// Bookkeeping the projection keeps in step with the stored part list while it edits in place.
///
/// The list itself is not mirrored here: it is the published [`ModelProgress`] snapshot, so an edit
/// borrows it next to this bookkeeping instead of copying it into a second buffer.
#[derive(Default)]
struct Book {
    /// Index of each reserved identity's row in the stored part list.
    slots: BTreeMap<PartKey, usize>,
    /// Bytes the current observation retains for its presentation parts, charged against the call's
    /// reliable quota.
    parts_bytes: u64,
    /// Set while a channel aggregate observation exists; cleared when provider rows replace it.
    aggregates_present: bool,
}

pub(crate) struct ProgressProjection {
    sender: Option<ModelProgressSender>,
    /// Bookkeeping paired with the stored part list, which lives in the published snapshot.
    book: Book,
    /// Charged high-water of each tool call's argument bytes, keyed by the accumulator identity the
    /// stream resolves its input events to.
    ///
    /// Tool arguments are not presentation parts, but they are still output this call may have to
    /// retain, so they are charged against the same reliable quota — before they are copied into the
    /// canonical accumulator. The high-water is per call, so an equal-length authoritative
    /// `completed` never charges twice and a later append after a shorter replacement still advances
    /// the total honestly.
    tool_bytes: BTreeMap<String, u64>,
    /// Identity resolver shared with the canonical accumulator, used only to key `tool_bytes`.
    ///
    /// The projection charges argument bytes *before* the accumulator copies them, so it resolves
    /// each event through the same identity rules the accumulator applies: a start that exposes the
    /// item and call id followed by a `delta` that omits the call id, then a `completed` that only
    /// carries the call id, all land on one key instead of charging one call as several. Keeping the
    /// resolver here instead of a second key derivation is what stops the two consumers from
    /// disagreeing about which call an event belongs to.
    tool_keys: ToolStream,
    /// Set by the stream's terminal event; observations arriving after it are rejected.
    terminal: bool,
}

/// One in-place edit of the stored observation.
///
/// An edit borrows the published part list together with the bookkeeping and the quota that list is
/// charged against, so a single event appends, authorizes, or adds parts without the projection
/// holding or copying the list. Everything here is synchronous and bounded: charging and identity
/// reservation are in-memory, so an edit never performs I/O while the watch is held.
struct Edit<'a> {
    parts: &'a mut ModelParts,
    book: &'a mut Book,
    sender: &'a ModelProgressSender,
    /// Bytes the call's tool arguments are charged, fixed for the duration of one part edit.
    tool_total: u64,
}

impl ProgressProjection {
    pub(crate) fn new(sender: Option<ModelProgressSender>) -> Self {
        if let Some(sender) = &sender {
            sender.publish(ModelProgress::default());
        }
        Self {
            sender,
            book: Book::default(),
            tool_bytes: BTreeMap::new(),
            tool_keys: ToolStream::new(),
            terminal: false,
        }
    }

    /// Runs one event's edit against the published part list, notifying observers once.
    ///
    /// The watch holds the shared part list, so while no observer has cloned a snapshot the edit
    /// applies in place without copying; a snapshot a consumer really holds forces copy-on-write of
    /// at most the chunk the edit lands in, plus the small chunk index when that chunk is already
    /// filled, which keeps that snapshot immutable. The copy therefore happens only when a frame is
    /// really shared, and never for the whole list — but a consumer that keeps a clone of every frame
    /// still forces a copy of the chunk it shares, so this bounds copying, it does not remove it.
    fn edit<R>(&mut self, work: impl FnOnce(&mut Edit<'_>) -> R) -> R {
        let sender = self
            .sender
            .clone()
            .expect("a progress edit requires the live observation channel");
        let tool_total = self.tool_bytes.values().copied().sum::<u64>();
        let book = &mut self.book;
        let sender_ref: &ModelProgressSender = &sender;
        sender.edit_parts(move |parts| {
            work(&mut Edit {
                parts,
                book,
                sender: sender_ref,
                tool_total,
            })
        })
    }

    /// Charges the call's whole accepted output — presentation parts plus tool arguments — against
    /// the reliable quota.
    ///
    /// The charge is monotonic on the combined total, so an increment that only moves bytes between
    /// the two domains, or a repeated authoritative body, never double-charges. A refusal leaves the
    /// accepted parts exactly as they were: the caller only installs the new observation after this
    /// succeeds.
    fn charge_total(&self) -> Result<(), PureError> {
        let Some(sender) = &self.sender else {
            return Ok(());
        };
        let tool = self.tool_bytes.values().copied().sum::<u64>();
        sender
            .charge_output(self.book.parts_bytes.saturating_add(tool))
            .map_err(|error| PureError::MemoryError(error.to_string()))
    }

    /// Charges one tool call's argument high-water against the reliable quota.
    fn charge_tool_output(&mut self, key: &str, next: u64) -> Result<(), PureError> {
        let previous = self.tool_bytes.get(key).copied().unwrap_or_default();
        if next <= previous {
            return Ok(());
        }
        self.tool_bytes.insert(key.to_owned(), next);
        self.charge_total()
    }

    pub(crate) fn observe(&mut self, event: &ModelStreamEvent) -> Result<(), PureError> {
        if self.sender.is_none() || self.terminal {
            return Ok(());
        }
        match event {
            ModelStreamEvent::BlockOpened { .. } => {
                // Identity is recorded when the block first produces content; an empty open would
                // only publish an empty part.
            }
            ModelStreamEvent::ReasoningRawDelta {
                delta, provider, ..
            } => {
                self.edit(|edit| edit.observe_raw_reasoning(provider.as_ref(), delta))?;
            }
            ModelStreamEvent::BlockDelta {
                kind,
                delta,
                provider,
                ..
            } => {
                self.edit(|edit| edit.extend(*kind, provider.as_ref(), delta))?;
            }
            ModelStreamEvent::BlockClosed {
                kind,
                authoritative_content,
                provider,
                ..
            } => match authoritative_content {
                Some(ModelBlockContent::Text(text)) => {
                    self.edit(|edit| edit.authorize(*kind, provider.as_ref(), text))?;
                }
                Some(ModelBlockContent::ReasoningSummary(parts)) => {
                    self.edit(|edit| edit.authorize_summary(provider.as_ref(), parts))?;
                }
                None => {}
            },
            ModelStreamEvent::PresentationItem { item } => {
                self.edit(|edit| edit.observe_item(item))?;
            }
            ModelStreamEvent::Completed { .. } | ModelStreamEvent::Failed { .. } => {
                self.terminal = true;
            }
            ModelStreamEvent::ResponseStarted { .. }
            | ModelStreamEvent::ResponseModelObserved { .. }
            | ModelStreamEvent::ToolCallCaller { .. }
            | ModelStreamEvent::ResponsesContextItem { .. }
            | ModelStreamEvent::WebSearchStarted { .. }
            | ModelStreamEvent::WebSearchCompleted { .. }
            | ModelStreamEvent::Usage(_) => {}
            ModelStreamEvent::ToolInputStarted {
                stream_id,
                item_id,
                call_id,
                ..
            } => {
                // A start reserves the identity but carries no argument bytes, so nothing is charged
                // until the first delta. Reserving it through the shared resolver lets the following
                // deltas, which may omit the call id, resolve onto this same call.
                self.tool_keys
                    .reserve_identity(stream_id.as_ref(), call_id.as_ref(), item_id);
            }
            ModelStreamEvent::ToolInputDelta {
                stream_id,
                item_id,
                call_id,
                payload_delta,
                ..
            } => {
                let key =
                    self.tool_keys
                        .reserve_identity(stream_id.as_ref(), call_id.as_ref(), item_id);
                let next = self
                    .tool_bytes
                    .get(&key)
                    .copied()
                    .unwrap_or_default()
                    .saturating_add(payload_delta.text().len() as u64);
                self.charge_tool_output(&key, next)?;
            }
            ModelStreamEvent::ToolInputCompleted {
                stream_id,
                item_id,
                call_id,
                payload,
                ..
            }
            | ModelStreamEvent::ToolCallReady {
                stream_id,
                item_id,
                call_id,
                payload,
                ..
            } => {
                let Some(payload) = payload else {
                    return Ok(());
                };
                let key =
                    self.tool_keys
                        .reserve_identity(stream_id.as_ref(), call_id.as_ref(), item_id);
                let previous = self.tool_bytes.get(&key).copied().unwrap_or_default();
                let next = previous.max(payload.text().len() as u64);
                self.charge_tool_output(&key, next)?;
            }
        }
        Ok(())
    }
}

impl Edit<'_> {
    /// Charges one prospective increment against the call's reserved reliable output quota.
    ///
    /// The charge happens *before* the increment becomes part of the observation: a piece the quota
    /// cannot hold is refused, so the parts keep exactly the bytes this process really accepted and
    /// the terminal receipt reports those instead of a body that was never retained. The producer
    /// stays bounded by the reservation rather than by the provider's own output limit.
    fn accept(&mut self, previous_len: usize, next_len: usize) -> Result<(), PureError> {
        self.book.parts_bytes = self
            .book
            .parts_bytes
            .saturating_sub(previous_len as u64)
            .saturating_add(next_len as u64);
        self.charge_total()
    }

    fn charge_total(&self) -> Result<(), PureError> {
        let total = self.book.parts_bytes.saturating_add(self.tool_total);
        self.sender
            .charge_output(total)
            .map_err(|error| PureError::MemoryError(error.to_string()))
    }

    /// Recomputes the retained total after observations were dropped (aggregate → provider rows).
    fn settle_accepted(&mut self) {
        self.book.parts_bytes = self
            .parts
            .iter()
            .fold(0_u64, |total, part| total.saturating_add(part.len() as u64));
    }

    fn extend(
        &mut self,
        kind: ModelBlockKind,
        provider: Option<&ProviderBlockIdentity>,
        delta: &str,
    ) -> Result<(), PureError> {
        let (key, identity) = observation(kind, provider)?;
        let index = self.slot_for(key, identity)?;
        let previous = self.parts[index].len();
        let next = self.parts[index].appended(delta);
        self.accept(previous, next.len())?;
        self.parts[index] = next;
        Ok(())
    }

    /// Records one raw reasoning delta as its own provider part, or as the request-scoped raw
    /// reasoning channel when the adapter reports no provider item boundary.
    ///
    /// Raw reasoning never becomes a summary: an adapter that knows the item streams its own
    /// `ReasoningText` part (the identity the terminal item finalizes), and one that does not
    /// streams the stable aggregate channel the terminal reasoning item is keyed by.
    fn observe_raw_reasoning(
        &mut self,
        provider: Option<&ProviderBlockIdentity>,
        delta: &str,
    ) -> Result<(), PureError> {
        let (key, identity) = match provider {
            Some(provider) => provider_observation(
                &provider.item_id,
                ObservedItemKind::Reasoning,
                ObservedPartKind::ReasoningText,
                provider.content_index,
            ),
            None => {
                let channel = AggregateChannel::Reasoning;
                (
                    PartKey::Aggregate(channel),
                    ObservedPartIdentity::Aggregate { channel },
                )
            }
        };
        let index = self.slot_for(key, identity)?;
        let previous = self.parts[index].len();
        let next = self.parts[index].appended(delta);
        self.accept(previous, next.len())?;
        self.parts[index] = next;
        Ok(())
    }

    fn authorize(
        &mut self,
        kind: ModelBlockKind,
        provider: Option<&ProviderBlockIdentity>,
        text: &str,
    ) -> Result<(), PureError> {
        let (key, identity) = observation(kind, provider)?;
        let index = self.slot_for(key, identity)?;
        let previous = self.parts[index].len();
        let next = self.parts[index].authorized(text);
        self.accept(previous, next.len())?;
        self.parts[index] = next;
        Ok(())
    }

    /// Authorizes each authoritative reasoning summary part under its own stable index.
    ///
    /// The provider reports one summary index per part and the terminal item finalizes the same
    /// `SummaryText(index)` parts the stream produced, so every part is written at its own index.
    /// Joining them into index zero would lose or overwrite the other summaries and would break the
    /// stable identity the streamed deltas already published.
    fn authorize_summary(
        &mut self,
        provider: Option<&ProviderBlockIdentity>,
        parts: &[String],
    ) -> Result<(), PureError> {
        let Some(provider) = provider else {
            return Err(PureError::LlmError(
                "provider stream protocol error: reasoning summary content carries no provider item identity"
                    .to_owned(),
            ));
        };
        for (index, text) in parts.iter().enumerate() {
            let content_index = u32::try_from(index).unwrap_or(u32::MAX);
            let (key, identity) = provider_observation(
                &provider.item_id,
                ObservedItemKind::Reasoning,
                ObservedPartKind::SummaryText,
                content_index,
            );
            let slot = self.slot_for(key, identity)?;
            let previous = self.parts[slot].len();
            let next = self.parts[slot].authorized(text);
            self.accept(previous, next.len())?;
            self.parts[slot] = next;
        }
        Ok(())
    }

    /// Records one provider item as its own stable parts, reserving every identity once.
    ///
    /// The presentation item may arrive after deltas already accumulated the same part; the body
    /// observed so far is kept and only the late metadata (output index) is upgraded, then the
    /// provider's authoritative text is folded in once. It never silently replaces the part of
    /// another item kind or content index, because the slot is keyed by the full identity.
    fn observe_item(
        &mut self,
        item: &crate::completion::CompletionPresentationItem,
    ) -> Result<(), PureError> {
        let item_kind = match item.kind {
            crate::completion::CompletionPresentationItemKind::Text(channel) => {
                ObservedItemKind::Text(text_channel(channel))
            }
            crate::completion::CompletionPresentationItemKind::Reasoning => {
                ObservedItemKind::Reasoning
            }
        };
        let sender = self.sender;
        let provider_item_id = item.provider_item_id.as_str();
        let reserve = |part: Option<PresentationPart>| {
            sender
                .reserve_observed_item(provider_item_id, part)
                .map_err(|error| {
                    PureError::MemoryError(format!("presentation item reservation failed: {error}"))
                })
        };
        if item.parts.is_empty() {
            reserve(None)?;
        }
        for part in &item.parts {
            reserve(Some(presentation_part(part.kind, part.content_index)))?;
        }
        let item_id: Arc<str> = Arc::from(provider_item_id);
        for part in &item.parts {
            let observed_kind = observed_part_kind(part.kind);
            let identity = ObservedPartIdentity::Provider(ProviderPartIdentity {
                item_id: item_id.clone(),
                output_index: item.output_index,
                item_kind,
                part: observed_kind,
                content_index: part.content_index,
            });
            let key = PartKey::Provider {
                item_id: item_id.clone(),
                part: observed_kind,
                content_index: part.content_index,
            };
            let index = self.slot_for(key, identity)?;
            let previous = self.parts[index].len();
            let next = self.parts[index]
                .authorized(&part.text)
                .with_metadata(item.output_index, Some(item_kind));
            self.accept(previous, next.len())?;
            self.parts[index] = next;
        }
        Ok(())
    }

    /// Returns the slot of `key`, creating it and reserving its provider identity when new.
    fn slot_for(
        &mut self,
        key: PartKey,
        identity: ObservedPartIdentity,
    ) -> Result<usize, PureError> {
        if let Some(index) = self.book.slots.get(&key) {
            return Ok(*index);
        }
        match &identity {
            ObservedPartIdentity::Provider(provider) => {
                // Provider observations replace the channel aggregate instead of keeping two sources.
                self.drop_aggregate_observations();
                self.reserve(provider)?;
            }
            ObservedPartIdentity::Aggregate { .. } => self.book.aggregates_present = true,
        }
        let index = self.parts.len();
        self.parts.push(ObservedPart::new(identity));
        self.book.slots.insert(key, index);
        Ok(index)
    }

    fn reserve(&self, provider: &ProviderPartIdentity) -> Result<(), PureError> {
        self.sender
            .reserve_observed_item(&provider.item_id, Some(provider.presentation_part()))
            .map_err(|error| {
                PureError::MemoryError(format!("presentation item reservation failed: {error}"))
            })
    }

    /// Drops every aggregate observation once the adapter reports real provider boundaries.
    fn drop_aggregate_observations(&mut self) {
        if !self.book.aggregates_present {
            return;
        }
        self.book.aggregates_present = false;
        self.parts
            .retain(|part| !matches!(part.identity(), ObservedPartIdentity::Aggregate { .. }));
        self.book.slots.clear();
        for (index, part) in self.parts.iter().enumerate() {
            self.book.slots.insert(part_key(part.identity()), index);
        }
        self.settle_accepted();
    }
}

/// Observation identity of one block event, from its kind and (if reported) provider item part.
///
/// A summary block that reports no provider identity is a protocol error rather than an aggregate:
/// summaries are always one part of a provider item, so aggregating one would merge it into the raw
/// reasoning channel and lose the summary/raw distinction the terminal items keep.
fn observation(
    kind: ModelBlockKind,
    provider: Option<&ProviderBlockIdentity>,
) -> Result<(PartKey, ObservedPartIdentity), PureError> {
    match provider {
        Some(provider) => Ok(provider_observation(
            &provider.item_id,
            item_kind(kind),
            observed_block_part(kind),
            provider.content_index,
        )),
        None => match aggregate_channel(kind) {
            Some(channel) => Ok((
                PartKey::Aggregate(channel),
                ObservedPartIdentity::Aggregate { channel },
            )),
            None => Err(PureError::LlmError(format!(
                "provider stream protocol error: {kind:?} content carries no provider item identity"
            ))),
        },
    }
}

/// Observation identity of one provider item part, shared by block deltas and raw reasoning deltas.
fn provider_observation(
    item_id: &str,
    item_kind: ObservedItemKind,
    part: ObservedPartKind,
    content_index: u32,
) -> (PartKey, ObservedPartIdentity) {
    let item_id: Arc<str> = Arc::from(item_id);
    (
        PartKey::Provider {
            item_id: item_id.clone(),
            part,
            content_index,
        },
        ObservedPartIdentity::Provider(ProviderPartIdentity {
            item_id,
            output_index: None,
            item_kind,
            part,
            content_index,
        }),
    )
}

fn part_key(identity: &ObservedPartIdentity) -> PartKey {
    match identity {
        ObservedPartIdentity::Aggregate { channel } => PartKey::Aggregate(*channel),
        ObservedPartIdentity::Provider(provider) => PartKey::Provider {
            item_id: provider.item_id.clone(),
            part: provider.part,
            content_index: provider.content_index,
        },
    }
}

fn item_kind(kind: ModelBlockKind) -> ObservedItemKind {
    match kind {
        ModelBlockKind::Text { channel } => ObservedItemKind::Text(text_channel(channel)),
        ModelBlockKind::ReasoningSummary => ObservedItemKind::Reasoning,
    }
}

fn observed_block_part(kind: ModelBlockKind) -> ObservedPartKind {
    match kind {
        ModelBlockKind::Text { .. } => ObservedPartKind::OutputText,
        ModelBlockKind::ReasoningSummary => ObservedPartKind::SummaryText,
    }
}

/// Aggregate channel of a block kind streamed without provider item boundaries.
///
/// Only assistant text has such an aggregate: reasoning summaries are always itemized, while raw
/// reasoning without item identity is observed through [`AggregateChannel::Reasoning`] instead of
/// through a block kind.
fn aggregate_channel(kind: ModelBlockKind) -> Option<AggregateChannel> {
    match kind {
        ModelBlockKind::Text { .. } => Some(AggregateChannel::Text),
        ModelBlockKind::ReasoningSummary => None,
    }
}

fn observed_part_kind(kind: CompletionPresentationPartKind) -> ObservedPartKind {
    match kind {
        CompletionPresentationPartKind::OutputText => ObservedPartKind::OutputText,
        CompletionPresentationPartKind::ReasoningText => ObservedPartKind::ReasoningText,
        CompletionPresentationPartKind::SummaryText => ObservedPartKind::SummaryText,
    }
}

fn presentation_part(kind: CompletionPresentationPartKind, content_index: u32) -> PresentationPart {
    match kind {
        CompletionPresentationPartKind::OutputText => PresentationPart::OutputText(content_index),
        CompletionPresentationPartKind::ReasoningText => {
            PresentationPart::ReasoningText(content_index)
        }
        CompletionPresentationPartKind::SummaryText => PresentationPart::SummaryText(content_index),
    }
}

fn text_channel(channel: TraceTextChannel) -> ModelTextChannel {
    match channel {
        TraceTextChannel::User => ModelTextChannel::User,
        TraceTextChannel::Commentary => ModelTextChannel::Commentary,
        TraceTextChannel::Final => ModelTextChannel::Final,
    }
}
