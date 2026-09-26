//! Live projection of a Thread's admitted writes for realtime notifications.
//!
//! The Thread's observation worker owns the only instance of this projection. It consumes the
//! **admitted** writes of the Thread's reliable handoff queue in admission order, projects each
//! immutable `ThreadWrite` exactly once, publishes the result into the shared chat session and
//! broadcasts the same typed changes. The history writer consumes that same prepared batch, so live
//! and durable history stay one projection of the same fact (`design/15` §15.3).
//!
//! Everything the projection resolves while it runs comes from its own in-memory tables: the
//! canonical payloads of the identities it may still have to update, the ordinal/revision of every
//! identity in the retained window, and the report facts of the newest Turn. Storage is only read
//! once at install, when the projection is seeded with the durable facts of work that started in an
//! earlier owner or process — never on the commit or preview path.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

use pl_core::{
    chat::{ChatField, ChatItem, ChatLifecycle, PresentationPart, Session},
    model::{
        AggregateChannel, ContentBlock, ModelTextChannel, ObservedItemKind, ObservedPart,
        ObservedPartIdentity, ObservedPartKind,
    },
    thread::{ThreadEffectBatch, ThreadSnapshot, TurnRecord, TurnState},
};
use pl_protocol::{
    InteractionRequest, SkillActivationCause, SkillActivationResourceBase, ThreadAgentItem,
    ThreadAgentState, ThreadAttachment, ThreadContentLifecycle, ThreadInferenceItem,
    ThreadInferenceState, ThreadItem, ThreadItemState, ThreadRawItem, ThreadRuntimeSnapshot,
    ThreadSkillItem, ThreadTextChannel, ThreadTextItem, ThreadThinkingItem, ThreadToolItem,
    ThreadToolOutput, ThreadToolState, Turn, TurnPhase,
};

use super::ProjectionError;
use crate::studio::runtime::chat_item::{TOOL_RESULT_FIELD, static_meta, static_state};

/// What one effect's identities resolve to.
///
/// The first map holds the canonical payloads this projection already knows for the identities
/// (from its retained window or as a durable placeholder), and the second the orders a *new*
/// identity just took from the session's in-memory allocator. Both are in-memory facts: resolving
/// never reads durable history on the commit path.
type ResolvedIdentities = (BTreeMap<String, ThreadItem>, BTreeMap<String, u64>);

/// Upper bounds for the short replay window a live projection retains.
///
/// The projection is a bounded observation owner, not a second history: the committed facts it
/// keeps are the ones a running Turn can still reference, and anything older is read from durable
/// history on reconnect. These windows never grow with the Thread's history.
pub(in crate::studio) const LIVE_ITEM_WINDOW: usize = 1024;

/// Commentary parts kept as the fallback text of a Turn that committed no final summary.
///
/// The report's result is the committed final text, which is kept in full. A Turn that stopped
/// without one is described by the tail of its visible commentary; this bound keeps that diagnostic
/// from turning the report accumulator into the Turn's whole process log.
const TURN_COMMENTARY_PARTS: usize = 16;

/// Fixed per-identity charge of the retained-size estimate.
///
/// The estimate sums every variable-length body of an item, so this constant only covers the
/// identity, ordinals and payload shells around them. It never serializes an item.
const RETAINED_ITEM_OVERHEAD: u64 = 512;

/// Fixed per-entry charge of one map or set node whose key is measured separately.
///
/// Placement metadata (`Fact`) and the report accumulator are keyed collections; the key strings
/// themselves are summed by the estimator, and this constant covers the node and ordering overhead
/// around them.
const RETAINED_ENTRY_OVERHEAD: u64 = 64;

/// Which lifecycle notification a projected Turn produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::studio) enum TurnEvent {
    Started,
    Updated,
    Completed,
}

/// One non-content change projected from a single committed effect.
///
/// Content is deliberately not part of this vocabulary. The single projection owner writes every
/// committed body and streaming preview into the shared chat session, and the GUI reads them from
/// that one content window; a second content frame on the status feed would be the same bytes on a
/// parallel carrier. The status feed therefore only carries the facts the window does not own —
/// Turn lifecycle, interactions and runtime snapshots — and no subscriber ever projects content.
#[derive(Debug, Clone)]
pub(in crate::studio) enum LiveEvent {
    Turn { turn: Turn, event: TurnEvent },
    Interaction(Box<InteractionRequest>),
    Runtime(Box<ThreadRuntimeSnapshot>),
}

/// One in-flight streaming preview's resident fact.
///
/// The content blocks are shared with the producer (cloning them only clones the pointer), the
/// static structure is encoded once per identity, and `observed_version` is the observation's own
/// monotonic counter (model content version / tool accumulated length) — only when it advances does
/// the preview re-emit a frame. Nothing here materializes the whole body per token.
///
/// The `revision` delivered to the window comes from [`LiveProjection::revisions`], this
/// projection's own item content version: the preview and the terminal item share one counter, so
/// the preview can neither pass nor collide with the commit.
#[derive(Clone)]
struct LivePreview {
    /// All field blocks currently delivered; a model observation is one field, a tool preview is
    /// the arguments plus the streamed result.
    fields: BTreeMap<ChatField, Arc<ContentBlock>>,
    /// Cheap stable identity of one provider/aggregate observation, absent for a tool-output
    /// preview.
    ///
    /// The preview map itself is keyed by the projection id because the shared session contract is
    /// defined on it; this short key is what the streaming hot path compares, so an already
    /// delivered part never has to rebuild that long identity to be recognized. Its index entry is
    /// charged to the same retained budget ([`observed_index_bytes`]) as the rest of the projection.
    observed: Option<ObservedKey>,
    /// Static-structure-only meta, encoded once per identity.
    meta: Arc<[u8]>,
    /// Delivered structure / terminal shape (dynamic bodies cleared, see
    /// [`crate::studio::runtime::chat_item::static_state`]). The content version compares it, so a
    /// tool state transition or an execution terminal advances the version while a volatile
    /// timestamp that only the projection re-stamps does not.
    shape: ThreadItemState,
    order: u64,
    /// Newest **observation** version this identity delivered (model content version / tool
    /// accumulated length), used only for de-duplication; it is not the window item's content
    /// version, which [`LiveProjection::revisions`] owns.
    observed_version: u64,
}

/// Cheap stable identity of one provider/aggregate streaming observation.
///
/// It carries exactly the facts [`crate::chat::presentation_item_id`] derives the projection id
/// from — the provider item id and the part discriminator, never the late output index or part id —
/// so two observations of one identity always agree on it, while an observation that only learned
/// late metadata keeps its key.
///
/// The streaming hot path compares one of these instead of the projection id: the id has a long
/// common prefix across every part of a response, so ordering and comparing it per part per frame is
/// what turns a mostly-unchanged preview into work proportional to the whole response.
#[derive(Clone, PartialEq, Eq, Hash)]
enum ObservedKey {
    /// One request-scoped aggregate channel, published before the adapter itemizes the response.
    Aggregate(AggregateChannel),
    /// One provider item part: the item identity plus its part discriminator.
    Provider(Arc<str>, PresentationPart),
}

/// Cheap key of one observation identity, derived without building its projection id.
fn observed_key(identity: &ObservedPartIdentity) -> ObservedKey {
    match identity {
        ObservedPartIdentity::Aggregate { channel } => ObservedKey::Aggregate(*channel),
        ObservedPartIdentity::Provider(provider) => {
            ObservedKey::Provider(provider.item_id.clone(), provider.presentation_part())
        }
    }
}

/// Retained bytes of one placement fact keyed by its identity, its fixed entry shell included.
fn fact_bytes(id: &str, fact: &Fact) -> u64 {
    (id.len() as u64)
        .saturating_add(fact.turn_id.len() as u64)
        .saturating_add(RETAINED_ENTRY_OVERHEAD)
}

/// Retained bytes of one in-flight preview keyed by its identity.
///
/// The resident bodies are the shared content blocks, each counted at its observed length, while the
/// static meta is held once per identity.
fn preview_bytes(id: &str, preview: &LivePreview) -> u64 {
    preview_entry_bytes(id.len() as u64, preview)
}

/// Retained bytes of one preview entry with its identity length supplied separately.
///
/// This is the one formula every site scores a preview entry with — insert, replacement, release and
/// the re-derivation after the bounded window — so a replacement can never keep a hand-copied older
/// estimate that omits a term the projection added since (for example the short-key observation
/// index). `BTreeMap::insert` hands back the replaced entry without its key, so the caller passes the
/// key length it just used; the replaced key is the very same identity.
///
/// The delivered content blocks are shared pointers: each is charged once at its observed length and
/// never copied here.
fn preview_entry_bytes(id_len: u64, preview: &LivePreview) -> u64 {
    let mut bytes = id_len
        .saturating_add(preview.meta.len() as u64)
        .saturating_add(RETAINED_ENTRY_OVERHEAD)
        .saturating_add(preview_fields_bytes(preview));
    // The short-key observation index is resident metadata of the same projection, so its entry is
    // part of the same budget instead of living outside it.
    if let Some(observed) = &preview.observed {
        bytes = bytes.saturating_add(observed_index_bytes(observed));
    }
    bytes
}

/// Retained bytes of one short-key observation-index entry.
///
/// A provider key carries the provider item identity (its `Arc<str>` is shared with the observation,
/// so only the identity's own bytes are resident) and one entry shell for the key, the delivered
/// version and the map slot; an aggregate key carries no heap body at all. Charging one shell keeps
/// the estimate conservative without materializing anything.
fn observed_index_bytes(observed: &ObservedKey) -> u64 {
    match observed {
        ObservedKey::Aggregate(_) => RETAINED_ENTRY_OVERHEAD,
        ObservedKey::Provider(item_id, _) => {
            (item_id.len() as u64).saturating_add(RETAINED_ENTRY_OVERHEAD)
        }
    }
}

/// Retained bytes of one preview's delivered field blocks.
fn preview_fields_bytes(preview: &LivePreview) -> u64 {
    preview.fields.values().fold(0_u64, |total, block| {
        total.saturating_add(block.len() as u64)
    })
}

/// Retained bytes of one content-version entry keyed by its identity.
fn revision_bytes(id: &str) -> u64 {
    (id.len() as u64).saturating_add(RETAINED_ENTRY_OVERHEAD)
}

/// The product content one committed effect contributes, projected exactly once.
///
/// It is the output of the Thread's single live projection owner: the same projection that
/// publishes the effect into the shared session and broadcasts its typed changes. The reliable
/// admission queue carries it to the history writer, which commits this exact identity/revision
/// set instead of projecting the effect a second time.
///
/// Every item carries its own **content version** (never the effect sequence that happens to
/// commit it). The writer stores that exact version and confirms it back by identity/version, so a
/// save receipt can never acknowledge a payload it did not write.
#[derive(Debug)]
pub(in crate::studio) struct PreparedEffect {
    /// Effect sequence this batch belongs to; the writer matches it against its admission queue.
    pub(in crate::studio) sequence: u64,
    /// The canonical items the effect committed, in the order the durable rows are written.
    pub(in crate::studio) items: Vec<ThreadItem>,
}

/// Cheap retained-memory estimate of one prepared batch.
///
/// Every variable-length body a retained item can carry is summed — text, reasoning, tool
/// arguments and streamed output and results, attachment metadata, opaque JSON payloads, agent and
/// inference diagnostics, Skill and file facts — and every identity is charged a fixed overhead for
/// its ordinals and payload shell. Nothing is serialized, so the estimate can be taken before the
/// reliable queue's lock. An attachment's blob is not resident, so only its metadata is charged.
pub(in crate::studio) fn retained_bytes(items: &[ThreadItem]) -> u64 {
    items.iter().fold(0_u64, |total, item| {
        total.saturating_add(charged_item_bytes(item))
    })
}

/// Resident bytes one retained item holds, its fixed identity overhead included.
fn charged_item_bytes(item: &ThreadItem) -> u64 {
    item_body_bytes(item).saturating_add(RETAINED_ITEM_OVERHEAD)
}

/// Sums every resident body of one item's category payload.
fn item_body_bytes(item: &ThreadItem) -> u64 {
    match item.state() {
        ThreadItemState::Raw(raw) => raw
            .payloads
            .iter()
            .fold(0_u64, |total, payload| {
                total.saturating_add(payload.content.len() as u64)
            })
            .saturating_add(raw.notice.len() as u64),
        ThreadItemState::Text(text) => (text.text().len() as u64)
            .saturating_add(attachments_bytes(text.attachments()))
            .saturating_add(lifecycle_bytes(text.lifecycle())),
        ThreadItemState::Thinking(thinking) => thinking
            .summary()
            .iter()
            .chain(thinking.content())
            .fold(0_u64, |total, chunk| {
                total.saturating_add(chunk.len() as u64)
            })
            .saturating_add(lifecycle_bytes(thinking.lifecycle())),
        ThreadItemState::Tool(tool) => tool_bytes(tool),
        ThreadItemState::Agent(agent) => agent_bytes(agent),
        ThreadItemState::Turn(turn) => optional_bytes(turn.input_id()),
        ThreadItemState::Inference(inference) => inference_bytes(inference),
        ThreadItemState::Skill(skill) => skill_bytes(skill),
        ThreadItemState::File(file) => {
            (file.path().len() as u64).saturating_add(optional_bytes(file.media_type()))
        }
        ThreadItemState::ContextCompaction(_) => 0,
    }
}

fn optional_bytes(value: Option<&str>) -> u64 {
    value.map_or(0, |text| text.len() as u64)
}

/// Diagnostic bodies a text or reasoning lifecycle can carry beyond its content.
fn lifecycle_bytes(lifecycle: &ThreadContentLifecycle) -> u64 {
    optional_bytes(lifecycle.failure())
        .saturating_add(optional_bytes(lifecycle.cancellation_reason()))
}

/// Metadata of one attachment list; the referenced blobs are not resident.
fn attachments_bytes(attachments: &[ThreadAttachment]) -> u64 {
    attachments.iter().fold(0_u64, |total, attachment| {
        total
            .saturating_add(attachment.id.len() as u64)
            .saturating_add(attachment.media_type.len() as u64)
            .saturating_add(optional_bytes(attachment.filename.as_deref()))
            .saturating_add(RETAINED_ENTRY_OVERHEAD)
    })
}

/// Arguments and state bodies of one tool item.
fn tool_bytes(tool: &ThreadToolItem) -> u64 {
    let invocation = tool.invocation();
    let mut bytes = 0_u64;
    for text in [
        invocation.tool_call_id(),
        invocation.name(),
        invocation.arguments(),
    ] {
        bytes = bytes.saturating_add(text.len() as u64);
    }
    for text in [
        invocation.call_id(),
        invocation.provider_item_id(),
        invocation.working_directory(),
        invocation.task_id(),
    ] {
        bytes = bytes.saturating_add(optional_bytes(text));
    }
    match tool.state() {
        ThreadToolState::Queued(_)
        | ThreadToolState::Started(_)
        | ThreadToolState::Streaming(_)
        | ThreadToolState::AwaitingApproval(_)
        | ThreadToolState::Approved(_) => {}
        ThreadToolState::Cancelling(state) => {
            bytes = bytes.saturating_add(state.streamed_output().len() as u64);
        }
        ThreadToolState::Interrupted(state) => {
            bytes = bytes.saturating_add(state.reason().len() as u64);
        }
        ThreadToolState::Running(state) => {
            bytes = bytes.saturating_add(state.streamed_output().len() as u64);
        }
        ThreadToolState::Succeeded(state) => {
            bytes = bytes.saturating_add(tool_output_bytes(state.output()));
        }
        ThreadToolState::Failed(state) => {
            bytes = bytes
                .saturating_add(state.failure().message().len() as u64)
                .saturating_add(state.output().map_or(0, tool_output_bytes));
        }
        ThreadToolState::Denied(state) => {
            bytes = bytes.saturating_add(state.reason().len() as u64);
        }
        ThreadToolState::Cancelled(state) => {
            bytes = bytes.saturating_add(state.reason().len() as u64);
        }
    }
    bytes
}

/// Result, attachment metadata and opaque artifacts of one terminal tool output.
fn tool_output_bytes(output: &ThreadToolOutput) -> u64 {
    (output.result().len() as u64)
        .saturating_add(attachments_bytes(output.attachments()))
        .saturating_add(
            output
                .output_artifacts()
                .iter()
                .fold(0_u64, |total, artifact| {
                    total.saturating_add(json_bytes(artifact))
                }),
        )
}

/// Resident size of one opaque JSON body, walked in place instead of re-serialized.
fn json_bytes(value: &serde_json::Value) -> u64 {
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) => 8,
        serde_json::Value::Number(_) => 16,
        serde_json::Value::String(text) => text.len() as u64,
        serde_json::Value::Array(values) => values.iter().fold(0_u64, |total, value| {
            total.saturating_add(json_bytes(value))
        }),
        serde_json::Value::Object(entries) => entries.iter().fold(0_u64, |total, (key, value)| {
            total
                .saturating_add(key.len() as u64)
                .saturating_add(json_bytes(value))
        }),
    }
}

/// Identity and state bodies of one agent item.
fn agent_bytes(agent: &ThreadAgentItem) -> u64 {
    let identity = agent.identity();
    let mut bytes = 0_u64;
    for text in [
        identity.id(),
        identity.path(),
        identity.role(),
        identity.task(),
    ] {
        bytes = bytes.saturating_add(text.len() as u64);
    }
    bytes = bytes.saturating_add(optional_bytes(identity.parent_path()));
    match agent.state() {
        ThreadAgentState::Queued(_) | ThreadAgentState::Running(_) => {}
        ThreadAgentState::Succeeded(state) => {
            bytes = bytes.saturating_add(state.summary().len() as u64);
        }
        ThreadAgentState::Denied(state) => {
            bytes = bytes.saturating_add(state.reason().len() as u64);
        }
        ThreadAgentState::Cancelled(state) => {
            bytes = bytes.saturating_add(state.reason().len() as u64);
        }
        ThreadAgentState::Failed(state) => {
            bytes = bytes.saturating_add(state.error().len() as u64);
        }
    }
    bytes
}

/// Identity and diagnostic bodies of one inference item.
fn inference_bytes(inference: &ThreadInferenceItem) -> u64 {
    let mut bytes =
        (inference.inference_id().len() as u64).saturating_add(inference.model().len() as u64);
    match inference.state() {
        ThreadInferenceState::Running(_) | ThreadInferenceState::Completed(_) => {}
        ThreadInferenceState::Failed(state) => {
            bytes = bytes.saturating_add(state.error().len() as u64);
        }
        ThreadInferenceState::Cancelled(state) => {
            bytes = bytes.saturating_add(state.reason().len() as u64);
        }
    }
    bytes
}

/// Bodies of one Skill activation item.
fn skill_bytes(skill: &ThreadSkillItem) -> u64 {
    let activation = skill.activation();
    let mut bytes = 0_u64;
    for text in [
        activation.name.as_str(),
        activation.source.as_str(),
        activation.provider_id.as_str(),
        activation.turn_id.as_str(),
    ] {
        bytes = bytes.saturating_add(text.len() as u64);
    }
    bytes = bytes.saturating_add(match &activation.cause {
        SkillActivationCause::Tool { tool_call_id } => tool_call_id.len() as u64,
        SkillActivationCause::UserGesture { invocation_id } => invocation_id.len() as u64,
    });
    bytes.saturating_add(match &activation.resource_base {
        SkillActivationResourceBase::Directory { path } => path.len() as u64,
        SkillActivationResourceBase::Url { url } => url.len() as u64,
        SkillActivationResourceBase::Opaque { description } => description.len() as u64,
    })
}

/// The facts a parent/child report and the directory summary need from one Turn.
///
/// The report is the committed **result** plus the stable identities it belongs to — not the
/// Turn's process bodies. Keeping this accumulator instead of the Turn's whole item set is what
/// bounds the live projection's memory: the final text, a bounded commentary fallback, the newest
/// tool item and every identity the Turn produced.
///
/// Text facts are keyed by identity and ordinal, so a later revision of the same body replaces its
/// entry instead of appending a second copy: an uncommitted streaming preview and the committed
/// item it finalizes share one key, a Turn that rewrote its own summary keeps only the latest text,
/// and the report never grows with the number of revisions a long Turn committed.
#[derive(Debug, Default, Clone)]
pub(in crate::studio) struct TurnResult {
    identities: BTreeSet<String>,
    finals: BTreeMap<(u64, String), String>,
    /// Tail of the visible commentary, used only when the Turn committed no final text.
    commentary: BTreeMap<(u64, String), String>,
    last_tool: Option<(u64, ThreadToolItem)>,
}

impl TurnResult {
    /// Every identity the Turn committed, in no particular order.
    pub(in crate::studio) fn identities(&self) -> impl Iterator<Item = &str> {
        self.identities.iter().map(String::as_str)
    }

    /// The committed final text, or `None` when the Turn committed none.
    pub(in crate::studio) fn final_text(&self) -> Option<String> {
        let text = self
            .finals
            .values()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n\n");
        (!text.is_empty()).then_some(text)
    }

    /// The bounded commentary tail, joined; the fallback description of a Turn without a summary.
    pub(in crate::studio) fn commentary(&self) -> String {
        self.commentary
            .values()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// The newest committed tool item, used when the Turn produced no final text.
    pub(in crate::studio) fn last_tool(&self) -> Option<&ThreadToolItem> {
        self.last_tool.as_ref().map(|(_, tool)| tool)
    }

    /// Bytes this Turn's report facts retain: the texts kept in full plus one shell per entry.
    ///
    /// The report needs these texts, so a Turn under storage pressure keeps them instead of losing
    /// its result: the value is what the Thread's reliable budget charges, which is how a Turn that
    /// would otherwise grow its accumulator without bound becomes visible pressure.
    pub(in crate::studio) fn retained_bytes(&self) -> u64 {
        let mut bytes = 0_u64;
        for (key, text) in self.finals.iter().chain(self.commentary.iter()) {
            bytes = bytes
                .saturating_add(key.1.len() as u64)
                .saturating_add(text.len() as u64)
                .saturating_add(RETAINED_ENTRY_OVERHEAD);
        }
        for id in &self.identities {
            bytes = bytes
                .saturating_add(id.len() as u64)
                .saturating_add(RETAINED_ENTRY_OVERHEAD);
        }
        if let Some((_, tool)) = &self.last_tool {
            bytes = bytes.saturating_add(tool_bytes(tool));
        }
        bytes
    }

    /// Adds one committed payload to the Turn's result facts.
    fn observe(&mut self, item: &ThreadItem) {
        self.identities.insert(item.id.clone());
        if let Some(tool) = item.tool() {
            let newest = self
                .last_tool
                .as_ref()
                .is_none_or(|(ordinal, _)| item.ordinal >= *ordinal);
            if newest {
                self.last_tool = Some((item.ordinal, tool.clone()));
            }
        }
        if let Some(text) = item.text() {
            let key = (item.ordinal, item.id.clone());
            match text.channel() {
                ThreadTextChannel::Final => {
                    self.finals.insert(key, text.text().to_owned());
                }
                ThreadTextChannel::Commentary => {
                    self.commentary.insert(key, text.text().to_owned());
                    // The tail is a diagnostic fallback, not a second process log: keep only the
                    // newest parts and drop the oldest by their canonical placement.
                    while self.commentary.len() > TURN_COMMENTARY_PARTS {
                        let oldest = self
                            .commentary
                            .keys()
                            .next()
                            .cloned()
                            .expect("commentary is non-empty");
                        self.commentary.remove(&oldest);
                    }
                }
                _ => {}
            }
        }
    }

    /// Result facts of one durable Turn slice, used by the cold recovery paths.
    pub(in crate::studio) fn from_items(items: &[ThreadItem]) -> Self {
        let mut result = Self::default();
        for item in items {
            result.observe(item);
        }
        result
    }
}

/// Ordinal, revision and creation stamp of one identity the projection has already published.
///
/// A later revision of the same identity is stamped from this instead of a storage lookup. The body
/// itself is only kept while it can still change, so this metadata is what makes "release the
/// process body" safe without losing the identity's canonical placement.
#[derive(Debug, Clone)]
struct Fact {
    ordinal: u64,
    revision: u64,
    created_at: i64,
    turn_id: String,
}

impl Fact {
    /// Stand-in payload for an identity whose body left the live window.
    ///
    /// The projection only reads placement metadata and identity presence from it; a projection
    /// that needs the released body itself (re-projecting a pruned tool call) fails closed instead
    /// of inventing content.
    fn placeholder(&self, id: &str, thread_id: &str) -> ThreadItem {
        ThreadItem::new(
            id.to_owned(),
            thread_id.to_owned(),
            self.turn_id.clone(),
            self.ordinal,
            self.revision,
            self.created_at,
            self.created_at,
            ThreadItemState::Raw(ThreadRawItem {
                payloads: Vec::new(),
                notice: "This item's body is no longer resident in the live window.".to_owned(),
                recorded_at: self.created_at,
            }),
        )
    }
}

/// Folds admitted writes into the live product facts and yields the changes each one made.
pub(in crate::studio) struct LiveProjection {
    /// Canonical payloads of identities that can still change.
    ///
    /// A non-terminal item is updated by a later effect, and the newest Turn's tool items are
    /// re-projected by a late delivery, so their bodies stay. A terminal body that can no longer
    /// change is released: the shared session window and durable history own the presentation from
    /// then on.
    items: BTreeMap<String, ThreadItem>,
    /// Placement metadata of every identity inside the retained window.
    facts: BTreeMap<String, Fact>,
    /// Report facts of the newest Turn.
    results: BTreeMap<String, TurnResult>,
    /// Hidden input identities this projection has learned from effects or the cold seed.
    hidden_inputs: BTreeSet<String>,
    /// Turns already announced on the feed, so a repeat is reported as an update.
    turns_seen: BTreeSet<String>,
    /// Last phase already published for each running Turn.
    ///
    /// The Turn record only changes when a Turn starts or finishes, but the canonical phase is
    /// derived from running tasks and the newest attempt outcome. Keeping the published phase lets
    /// the projection emit a `turnUpdated` frame for each real phase transition instead of leaving
    /// the activity row at the phase captured when the Turn started.
    turn_phases: BTreeMap<String, TurnPhase>,
    /// Identity of the attempt the resident streaming previews belong to.
    ///
    /// A new attempt uses entirely new observation identities, so the caches of the previous one no
    /// longer apply.
    preview_attempt: Option<String>,
    /// In-flight streaming previews keyed by their stable identity.
    ///
    /// Each observation snapshot carries its own content version and only an identity whose version
    /// advanced is re-delivered; the hot path only clones the shared `Arc<ContentBlock>`, and the
    /// terminal commit replaces the same identity directly.
    previews: BTreeMap<String, LivePreview>,
    /// Newest observation version delivered per provider/aggregate identity.
    ///
    /// It mirrors exactly the `observed_version` of every provider/aggregate entry in
    /// [`LiveProjection::previews`], which is keyed by the long projection id the session contract
    /// needs; this short-keyed index is what the streaming hot path compares, so an unchanged part
    /// never rebuilds that id and never re-walks the long common prefix of every sibling key. The
    /// two can never disagree: every insert or removal of a provider preview moves both, and
    /// [`LiveProjection::retain`] re-derives this index from the entries that survived.
    observed: HashMap<ObservedKey, u64>,
    /// Content version of every window item, owned by this projection and independent of the
    /// execution commit sequence.
    ///
    /// It is the source of `ChatItem::revision`: it advances on first delivery and whenever the
    /// body, structure or terminal state really changed, and otherwise keeps its old value. A
    /// streaming preview and its terminal commit therefore converge on one counter; the effect
    /// sequence stays only the commit / reliable-save watermark and no longer bounds the item
    /// version. A save confirmation does not advance the version, but any transition of the
    /// **persistable payload** (body, tool state, execution terminal) does — otherwise an old
    /// receipt would confirm a payload it never wrote.
    ///
    /// The version is the identity's own monotonic fact, not a property of this cache: an identity
    /// that was evicted from the retained window, or re-encountered after the projection was
    /// re-seeded, recovers its baseline from the canonical / session fact
    /// ([`LiveProjection::adopt_revisions`] / [`LiveProjection::adopt_session_revision`]) instead of
    /// restarting at 1.
    revisions: BTreeMap<String, u64>,
    /// Running retained-byte totals of the four tables that grow with a response.
    ///
    /// `retained_bytes` is the projection's only budget input and is published on every preview
    /// frame, so recomputing these totals from the tables on that path costs O(parts) per frame
    /// while a real change moves a single entry. Each mutation keeps them exact, and
    /// [`LiveProjection::retain`] re-derives them from the pruned tables, so a running total can
    /// never drift from what the tables hold. The remaining tables are bounded by Turns and inputs
    /// rather than by streamed parts and stay directly summed.
    items_bytes: u64,
    facts_bytes: u64,
    previews_bytes: u64,
    revisions_bytes: u64,
}

impl LiveProjection {
    /// Creates an empty projection owner.
    ///
    /// The caller seeds it with the durable facts of unfinished work while installing the Thread's
    /// observation, once, before any commit is projected.
    pub(in crate::studio) fn new() -> Self {
        Self {
            items: BTreeMap::new(),
            facts: BTreeMap::new(),
            results: BTreeMap::new(),
            hidden_inputs: BTreeSet::new(),
            turns_seen: BTreeSet::new(),
            turn_phases: BTreeMap::new(),
            preview_attempt: None,
            previews: BTreeMap::new(),
            observed: HashMap::new(),
            revisions: BTreeMap::new(),
            items_bytes: 0,
            facts_bytes: 0,
            previews_bytes: 0,
            revisions_bytes: 0,
        }
    }

    /// Installs the durable facts of work that started before this projection existed.
    ///
    /// The caller reads the bounded slice once, while installing the observation: the newest Turn's
    /// committed items plus the hidden dispositions of the Thread's inputs. Everything the hot path
    /// resolves afterwards comes from these tables, so a commit or a preview never waits on a
    /// history reader — including for a Thread re-activated in the middle of a Turn.
    pub(in crate::studio) fn seed(&mut self, items: Vec<ThreadItem>, hidden: BTreeSet<String>) {
        // The seeded bodies already carry the content version durable history stored, so they are
        // the baseline of this projection's counter: re-encountering the same identity must never
        // restart at 1, because a lower version is silently dropped by the shared session and the
        // preview would stay stuck on the old body.
        for item in &items {
            self.raise_revision(&item.id, item.revision);
        }
        for item in items {
            self.fact(&item);
            self.observe_result(&item);
            self.committed_body(item);
        }
        self.hidden_inputs.extend(hidden);
    }

    /// Report facts of one Turn, if the projection still holds them.
    pub(in crate::studio) fn turn_result(&self, turn_id: &str) -> Option<&TurnResult> {
        self.results.get(turn_id)
    }

    /// Whether this projection still holds the committed terminal tool result for `id`.
    ///
    /// A reliable-output repair must know whether it can supplement the result in memory or has to
    /// read the already-committed body back first; only a resident terminal tool item is a body this
    /// owner can project without a durable read.
    pub(in crate::studio) fn holds_terminal_tool(&self, id: &str) -> bool {
        self.items.get(id).is_some_and(|item| {
            matches!(item.state(), ThreadItemState::Tool(tool) if tool.terminal_output().is_some())
        })
    }

    /// Bytes this projection currently retains in its own tables.
    ///
    /// The retained bodies, their placement metadata and the report accumulator are the projection's
    /// own resident memory: they outlive the prepared batch the writer has already written, so the
    /// Thread's reliable budget has to cover them too. The value is absolute, not cumulative, so
    /// releasing a body lowers the budget in the same step that released it.
    ///
    /// The value is read on **every** published frame, while a long response streams one part per
    /// identity; the four part-keyed tables that grow with the response therefore carry running
    /// totals that each real change moves by one entry ([`LiveProjection::store_item`],
    /// [`LiveProjection::store_fact`], [`LiveProjection::store_preview`],
    /// [`LiveProjection::raise_revision`]), so no frame walks them any more.
    ///
    /// The remaining terms are not all equally cheap, and this is deliberately not claimed to be
    /// O(turns): `hidden_inputs`, `turns_seen` and `turn_phases` are bounded by the Thread's inputs
    /// and Turns, but a Turn's report accumulator is still summed in full here
    /// ([`TurnResult::retained_bytes`] walks that Turn's kept final parts). A frame that runs while a
    /// large finished Turn is resident is therefore still proportional to that Turn's report facts —
    /// a bounded window rather than the whole response, and the next precise target for this path.
    pub(in crate::studio) fn retained_bytes(&self) -> u64 {
        let mut bytes = self
            .items_bytes
            .saturating_add(self.facts_bytes)
            .saturating_add(self.previews_bytes)
            .saturating_add(self.revisions_bytes);
        for (turn_id, result) in &self.results {
            bytes = bytes
                .saturating_add(turn_id.len() as u64)
                .saturating_add(result.retained_bytes());
        }
        for identity in &self.hidden_inputs {
            bytes = bytes
                .saturating_add(identity.len() as u64)
                .saturating_add(RETAINED_ENTRY_OVERHEAD);
        }
        for turn_id in &self.turns_seen {
            bytes = bytes
                .saturating_add(turn_id.len() as u64)
                .saturating_add(RETAINED_ENTRY_OVERHEAD);
        }
        for turn_id in self.turn_phases.keys() {
            bytes = bytes
                .saturating_add(turn_id.len() as u64)
                .saturating_add(RETAINED_ENTRY_OVERHEAD);
        }
        bytes
    }

    /// Projects one admitted write into the session and broadcasts its live changes.
    ///
    /// The batch is published into the shared session **before** the caller hands it to the reliable
    /// save queue, so "the writer confirmed this identity" always means "the session already holds
    /// it": there is no save-before-publish interleaving and no hidden confirmation table.
    ///
    /// # Errors
    /// Returns a projection failure when the effect references facts this projection cannot resolve
    /// from memory. The caller reports it, so the durable barrier fails closed with the real reason
    /// instead of writing a timeline with a hole.
    pub(in crate::studio) fn advance(
        &mut self,
        chat: &Session,
        thread: &pl_protocol::Thread,
        usage: &pl_core::thread::UsageSummary,
        effect: &ThreadEffectBatch,
        state: &ThreadSnapshot,
    ) -> Result<(Vec<LiveEvent>, Arc<PreparedEffect>), ProjectionError> {
        let prepared = self.project(chat, thread, effect, state)?;
        for item in &prepared.items {
            chat.publish(
                crate::studio::storage::history::chat_item(item.clone(), false)
                    .map_err(|error| ProjectionError::History(error.to_string()))?,
            )
            .map_err(|error| ProjectionError::History(error.to_string()))?;
        }
        let mut events = Vec::new();
        self.emit_frames(usage, thread, effect, state, &mut events)?;
        self.retain(state);
        Ok((events, prepared))
    }

    /// Projects one admitted write this owner installed *behind*: it folds the fact into memory and
    /// returns the canonical batch, but publishes nothing.
    ///
    /// A commit the owner had already folded into the snapshot this projection was seeded from is
    /// never replayed as a live frame — the subscriber's baseline already carries it — yet the
    /// durable writer still owes it a row, so the same single projection produces its content here.
    ///
    /// # Errors
    /// Same as [`LiveProjection::advance`].
    pub(in crate::studio) fn project_committed(
        &mut self,
        chat: &Session,
        thread: &pl_protocol::Thread,
        effect: &ThreadEffectBatch,
        state: &ThreadSnapshot,
    ) -> Result<Arc<PreparedEffect>, ProjectionError> {
        self.project(chat, thread, effect, state)
    }

    /// Projects one effect's canonical content exactly once.
    ///
    /// Every identity is resolved against this projection's own tables and the shared session's
    /// in-memory allocator; nothing here reads storage. An identity the projection has never seen is
    /// a new one and reserves its order in memory, which is exactly what the writer will commit. The
    /// projected bodies are folded into this projection in the same step, so a later effect of the
    /// same Turn resolves them without a second projection or a storage lookup.
    fn project(
        &mut self,
        chat: &Session,
        thread: &pl_protocol::Thread,
        effect: &ThreadEffectBatch,
        state: &ThreadSnapshot,
    ) -> Result<Arc<PreparedEffect>, ProjectionError> {
        for change in effect.inputs.iter() {
            if let pl_core::thread::input::InputChange::Accepted(record) = change
                && super::input_presentation(record) == pl_protocol::MessagePresentation::Hidden
            {
                self.hidden_inputs.insert(record.input.id.clone());
            }
        }
        let provisional = super::project_effect_items(
            thread,
            state,
            effect,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeSet::new(),
        )?;
        // The same identity set the durable writer enumerates: the items this effect produces,
        // the pruned inputs it still references, and the calls whose producing attempt left the
        // state. All three are resolved from the projection's own facts.
        let mut ids = provisional
            .items
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        ids.extend(provisional.unresolved_inputs.iter().cloned());
        ids.extend(provisional.unresolved_calls.iter().cloned());
        // Only a channel this projection already started may be finalized by this effect. A channel
        // the response never produced must not be reserved here: reserving a name is not starting a
        // row, and an effect mentioning a channel would otherwise fabricate an empty aggregate item
        // (an empty `reasoning`/`text` row) the provider never emitted. A provider part the window
        // already showed is a started identity too: `finalize` has to name it even when the outcome's
        // own receipt cannot carry it, so this commit closes the identity the streaming frames used
        // instead of inventing a second aggregate one.
        if let Some(attempt) = &effect.attempt {
            for channel in ["reasoning", "text"] {
                let id = super::attempt_channel_id(&attempt.attempt_id, channel);
                if self.identity_is_started(chat, &id) {
                    ids.push(id);
                }
            }
            ids.extend(self.started_provider_ids(&attempt.attempt_id));
        }
        let (mut existing, reserved) = self.resolve(chat, thread, ids)?;
        let hidden_inputs = self.hidden_inputs.clone();
        // Only a bounded reception may take its committed body from the preview the window already
        // showed: the stream stopped before the provider's authoritative item, so the prefix that did
        // arrive stays on the identity it was shown on instead of being lost. A normally received
        // response always carries its complete provider parts in its own receipt, so its committed
        // content never depends on what a subscriber happened to observe.
        if let Some(attempt) = &effect.attempt
            && super::responses::keeps_bounded_preview(&attempt.outcome)
        {
            for (id, item) in self.attempt_preview_items(&attempt.attempt_id, &attempt.turn_id)? {
                // The provider part the window is showing is the body this commit must close. A
                // placement fact alone resolves to a release placeholder, so it must never beat the
                // preview that still holds the streamed text; a canonical committed payload (this
                // projection's own body) stays authoritative.
                if !self.items.contains_key(&id) {
                    existing.insert(id, item);
                }
            }
        }
        let projected = super::project_effect_items(
            thread,
            state,
            effect,
            &existing,
            &reserved,
            &hidden_inputs,
        )?;
        // 实时帧同样不允许带着无法补全的 identity 继续：投影失败由调用方转成报告并 fail-closed。
        projected.ensure_complete()?;
        let mut items = projected.items;
        // 内容版本基线：把这里解析到的 canonical 事实的 revision 抬成本投影的单调下限。已在内存里的
        // 身份不复查（`adopt_revisions` 跳过 `self.items` 持有的 id），只有冷恢复/被淘汰后重现的身份才
        // 由放置元数据恢复；同一条目因此绝不因为缓存淘汰而从 1 重编、被会话当成旧帧丢弃。
        self.adopt_revisions(&existing);
        // 每一条目的窗口版本由本投影自己的内容版本给出，和 effect 序号（提交 / 可靠保存水位）无关：
        // Turn 绑定、正文、结构或终态状态真的变了才递增，否则沿用旧版本（例如只推进 saved 的保存确认，
        // 或与上次完全相同的重新投影）。该版本同时被流式预览使用，因此同一身份的预览与终态沿同一个
        // 计数器收束，终态不会被预览版本挡住，旧保存回执也无法确认它没写过的新载荷。
        for item in items.iter_mut() {
            // The payload this revision was last delivered with, when this projection still knows
            // it: the deterministic comparison partner for a repeat projection.
            let previous = self
                .items
                .get(item.id.as_str())
                .or_else(|| existing.get(item.id.as_str()))
                .map(|previous| {
                    (
                        previous.ordinal,
                        previous.turn_id.clone(),
                        previous.created_at,
                        previous.updated_at,
                    )
                });
            let changed = self.content_changed(item.id.as_str(), item, &existing);
            item.revision = match (self.revisions.get(item.id.as_str()).copied(), changed) {
                (Some(current), false) => current,
                _ => self.bump_content_revision(&item.id),
            };
            // One `(identity, revision)` pair is one canonical payload. A repeat projection whose
            // body, structure and terminal shape are unchanged therefore keeps the placement and the
            // timestamps of the payload it was last delivered with instead of restamping the
            // projection clock: otherwise the same revision would be written twice with two
            // different payloads, and the durable row's own revision fence rejects that as a real
            // conflict. A body that did change takes the fresh stamp together with its new version.
            if !changed && let Some((ordinal, turn_id, created_at, updated_at)) = previous {
                item.ordinal = ordinal;
                item.turn_id = turn_id;
                item.created_at = created_at;
                item.updated_at = updated_at;
            }
        }
        for item in &items {
            self.record(item.clone())?;
        }
        Ok(Arc::new(PreparedEffect {
            sequence: effect.sequence,
            items,
        }))
    }

    /// Projects the ephemeral streaming preview the owner publishes between committed effects.
    ///
    /// Streaming text, reasoning and tool output never appear in an effect batch, so the overlay is
    /// the only projection of them. It consumes typed observations — a stable identity per provider
    /// item part, or a whole channel before the adapter itemizes the response — so it never decodes a
    /// producer payload again. The hot path writes the observation's **shared** `Arc<ContentBlock>`
    /// straight into the window's fields: it never calls `part.text()`, never JSON-encodes the whole
    /// item and never copies the full body again. An identity whose observation version did not
    /// advance neither re-materializes its body nor re-emits a frame, and its static `meta` is
    /// encoded once per stable identity.
    ///
    /// The preview is written into the shared session, the one content window the GUI reads, and
    /// emits no content frame of its own. `applied` is the subscriber's committed-effect watermark;
    /// it is only the commit / reliable-save watermark and **not** the item content version, which
    /// comes from this projection's own [`LiveProjection::revisions`] — the same counter the terminal
    /// commit uses, so multi-token text is never dropped merely because the commit watermark did not
    /// move.
    pub(in crate::studio) async fn stream(
        &mut self,
        chat: &Session,
        thread: &pl_protocol::Thread,
        state: &ThreadSnapshot,
        _applied: u64,
    ) -> Result<(), ProjectionError> {
        let Some(preview) = state.model_progress.as_ref() else {
            self.preview_attempt = None;
            self.clear_previews();
            return Ok(());
        };
        let Some(attempt) = state
            .attempts
            .iter()
            .find(|attempt| attempt.attempt_id == preview.attempt_id)
        else {
            return Ok(());
        };
        let at = crate::studio::unix_seconds();
        // A new attempt uses entirely new observation identities, so the previous attempt's caches
        // no longer apply.
        if self.preview_attempt.as_deref() != Some(preview.attempt_id.as_str()) {
            self.preview_attempt = Some(preview.attempt_id.clone());
            self.clear_previews();
        }
        let progress = &preview.progress;
        // The producer retires the whole-channel aggregate observation the moment it itemizes the
        // response, yet the aggregate preview rows it published before still have to leave the window
        // and this projection, otherwise the same text would show up twice.
        if progress
            .parts()
            .iter()
            .any(|part| matches!(part.identity(), ObservedPartIdentity::Provider(_)))
        {
            for channel in [AggregateChannel::Text, AggregateChannel::Reasoning] {
                let id = aggregate_id(&attempt.attempt_id, channel);
                let streaming = self
                    .items
                    .get(&id)
                    .is_some_and(|item| !item.state().is_terminal());
                if streaming || self.previews.contains_key(&id) {
                    self.release_item(&id);
                    self.release_preview(&id);
                    chat.drop_preview(&id);
                }
            }
        }
        // Per-observation consumption: every observation carries a stable identity and its own content
        // version, and only an identity whose version advanced is delivered. An unchanged identity
        // neither re-materializes its body nor re-emits a frame.
        for part in progress.parts() {
            // An observation with no body yet is not a displayable preview; wait for its first byte.
            if part.is_empty() {
                continue;
            }
            // An identity whose observation version did not advance is not re-delivered, and this is
            // the one condition a long response hits for every already-delivered part on every
            // preview frame. It is decided on the observation's own short identity rather than on the
            // projection id: rebuilding that id and order-comparing its long sibling prefix for every
            // part every frame is what would make this loop cost O(parts) per frame for parts that
            // already delivered their newest body. [`LiveProjection::observed`] mirrors exactly the
            // `observed_version` of the identity's resident preview, so this skips the same set the
            // preview lookup would.
            //
            // It runs before the terminal test below because that test takes the shared session lock
            // and looks the identity up in the committed window — pure repetition for a part that
            // already delivered its newest body. Both checks skip the same way (neither touches a
            // delivered body or frame), so reordering cannot pull a terminal identity back in: an
            // advanced version still reaches, and is rejected by, the terminal test.
            let key = observed_key(part.identity());
            if self.observed.get(&key) == Some(&part.version()) {
                continue;
            }
            let Some((id, field, static_item)) = observation_preview(&attempt.attempt_id, part)
            else {
                return Err(ProjectionError::UnsupportedOutput(
                    "provider presentation part does not match its item".into(),
                ));
            };
            // A terminal identity is never pulled back by a late streaming observation, even after
            // this projection released its body from the retained window: the shared session owns
            // that fact, and a preview for it would let a stale frame replace (and a later preview
            // release erase) a row durable history already committed.
            if chat.is_committed(&id)
                || self
                    .items
                    .get(&id)
                    .is_some_and(|previous| previous.state().is_terminal())
            {
                continue;
            }
            // On the first encounter inside this projection the identity's baseline is recovered from
            // the shared session when that session already holds it — an in-memory read for a resident
            // preview, so a re-seeded projection never restarts an in-flight identity at 1 and its
            // frames are never dropped as stale. A genuinely new identity pays no read.
            if !self.previews.contains_key(&id) {
                self.adopt_session_revision(chat, &id).await?;
            }
            // The observation's own content version advanced, so this is a real body change: advance
            // this projection's content version and deliver the frame. It is never skipped because
            // "the committed revision equals the commit watermark" — the content version and the
            // commit watermark are two different facts.
            let revision = self.bump_content_revision(&id);
            let mut entry = match self.previews.get(&id).cloned() {
                Some(known) => known,
                None => {
                    // A preview's ordinal is allocated by the shared session in memory: the preview
                    // only ever lives in memory, and once this session assigned the stable identity its
                    // ordinal is reused, so the terminal projection resolves the same ordinal from the
                    // same identity without a history read.
                    let order = chat
                        .reserve_order(&id)
                        .await
                        .map_err(|error| ProjectionError::History(error.to_string()))?;
                    let created_at = self.created_at(&id, at);
                    let shape = static_state(&static_item);
                    let meta = static_meta(&ThreadItem::new(
                        id.clone(),
                        thread.id.clone(),
                        attempt.turn_id.clone(),
                        order,
                        0,
                        created_at,
                        at,
                        static_item,
                    ))
                    .map_err(|error| ProjectionError::History(error.to_string()))?;
                    // Placement metadata, so the retained window bounds this identity like any other
                    // fact instead of keeping its body alive for the whole Turn.
                    self.store_fact(
                        id.clone(),
                        Fact {
                            ordinal: order,
                            revision,
                            created_at,
                            turn_id: attempt.turn_id.clone(),
                        },
                    );
                    LivePreview {
                        fields: BTreeMap::new(),
                        // Filled in below with the key this part already resolved, so the identity is
                        // built once per delivered part instead of once per part per frame.
                        observed: None,
                        meta,
                        shape,
                        order,
                        observed_version: part.version(),
                    }
                }
            };
            entry.observed = Some(key);
            entry.observed_version = part.version();
            entry.fields.insert(field, part.content().clone());
            self.publish_preview(chat, &id, &attempt.turn_id, revision, &entry)?;
            self.store_preview(id, entry);
        }
        self.stream_tool_progress(state, chat)?;
        Ok(())
    }

    /// Streaming output preview of a running task: the producer's own shared content block,
    /// delivered in the `tool.result` field and keyed by the stable call identity.
    ///
    /// The Thread publishes each opaque output identity with its immutable content block and its
    /// own observation version, so this shares the newest block instead of rebuilding it from an
    /// accumulated string: an unchanged identity is recognized by version (and by pointer inside the
    /// block comparison), a changed one carries a new block, and a bounded window that rolled over
    /// arrives as a block the consumer's baseline no longer extends. No JSON is encoded and no
    /// canonical `ThreadItem` is materialized per chunk.
    fn stream_tool_progress(
        &mut self,
        state: &ThreadSnapshot,
        chat: &Session,
    ) -> Result<(), ProjectionError> {
        for (task_id, progress) in state.tool_progress.iter() {
            let Some(task) = state.tasks.get(task_id) else {
                continue;
            };
            if task.status != pl_core::thread::task::TaskStatus::Running {
                continue;
            }
            // An observation with no body yet is not a displayable preview; wait for its first byte.
            if progress.bytes() == 0 {
                continue;
            }
            let id = super::order::tool_id(&task.call_id);
            // The tool identity and its static structure come from the committed canonical fact: no
            // identity is invented here that the commit does not have, and no coarser meta is
            // hand-assembled.
            let Some(committed) = self.items.get(&id).cloned() else {
                continue;
            };
            // A terminal item is never pulled back by a late streaming observation.
            if committed.state().is_terminal() {
                continue;
            }
            // Only an identity whose own version advanced is delivered: an unchanged observation
            // neither re-materializes its body nor re-emits a frame.
            if self
                .previews
                .get(&id)
                .is_some_and(|known| known.observed_version == progress.version())
            {
                continue;
            }
            let result_field = ChatField::host(TOOL_RESULT_FIELD);
            // One shared block per identity: an unchanged window is the same `Arc`, and a rollover is
            // a different block, so the typed field diff reports a whole replacement instead of
            // appending onto a body that changed identity.
            let block = progress.content();
            let revision = self.bump_content_revision(&id);
            let mut entry = match self.previews.get(&id).cloned() {
                Some(known) => known,
                None => {
                    // Reuse the canonical item's static meta and body partner fields, swapping only
                    // the streamed result for a shared block.
                    let meta = static_meta(&committed)
                        .map_err(|error| ProjectionError::History(error.to_string()))?;
                    LivePreview {
                        fields: crate::studio::runtime::chat_item::content_fields(
                            committed.state(),
                        ),
                        // A tool-output preview is keyed by the call identity, not by a provider
                        // observation, so it never enters the provider observation index.
                        observed: None,
                        meta,
                        shape: static_state(committed.state()),
                        order: committed.ordinal,
                        observed_version: progress.version(),
                    }
                }
            };
            entry.observed_version = progress.version();
            entry.fields.insert(result_field, block);
            let turn_id = committed.turn_id.clone();
            self.publish_preview(chat, &id, &turn_id, revision, &entry)?;
            self.store_preview(id, entry);
        }
        Ok(())
    }

    /// Publishes one preview fact into the shared window.
    ///
    /// A failed `publish_preview` keeps the baseline already published instead of pretending success.
    fn publish_preview(
        &self,
        chat: &Session,
        id: &str,
        turn_id: &str,
        revision: u64,
        entry: &LivePreview,
    ) -> Result<(), ProjectionError> {
        chat.publish_preview(ChatItem {
            item_id: id.to_owned(),
            turn_id: turn_id.to_owned(),
            order: entry.order,
            revision,
            fields: entry.fields.clone(),
            meta: entry.meta.clone(),
            omitted_bytes: 0,
            saved: false,
            lifecycle: ChatLifecycle::Streaming,
        })
        .map_err(|error| ProjectionError::History(error.to_string()))
    }

    /// Raises the content-version baseline to the lower bound of the known canonical facts.
    ///
    /// The content version is each identity's own monotonic fact, not a property of this cache: when
    /// an identity was evicted from the retained window, or the projection was re-seeded and meets it
    /// again, the canonical payload resolved back already carries its version and this projection must
    /// continue from it. Restarting at 1 would be lower than the persisted old version, and the shared
    /// session would ignore the frame as stale.
    ///
    /// An identity already resident in `self.items` is left alone: while this projection holds it, its
    /// own counter is the session's authoritative baseline, and re-applying an external version would
    /// only jump it into another version domain. The ordinary path therefore never re-reads storage;
    /// only a cold identity resolved from a placement fact is raised here.
    fn adopt_revisions(&mut self, existing: &BTreeMap<String, ThreadItem>) {
        for (id, item) in existing {
            if item.revision == 0 || self.items.contains_key(id) {
                continue;
            }
            self.raise_revision(id, item.revision);
        }
    }

    /// Raises one identity's baseline to the shared session's current content version.
    ///
    /// The read happens only when the session already assigned this identity, so a brand-new identity
    /// pays nothing: the projection's own counter stays the authority for everything it published, and
    /// only a re-seeded projection has to recover the baseline of an in-flight preview the session
    /// already holds. This is the session's own resident fact, never a second content cache.
    async fn adopt_session_revision(
        &mut self,
        chat: &Session,
        id: &str,
    ) -> Result<(), ProjectionError> {
        if chat.assigned_order(id).is_none() {
            return Ok(());
        }
        let Some(item) = chat
            .read_item(id)
            .await
            .map_err(|error| ProjectionError::History(error.to_string()))?
        else {
            return Ok(());
        };
        self.raise_revision(id, item.revision);
        Ok(())
    }

    /// Advances and returns one identity's new content version.
    fn bump_content_revision(&mut self, id: &str) -> u64 {
        let next = self
            .revisions
            .get(id)
            .copied()
            .unwrap_or(0)
            .saturating_add(1);
        self.raise_revision(id, next);
        next
    }

    /// Raises one identity's content-version baseline, keeping the retained-byte estimate in step.
    ///
    /// Inserting an identity's first version also inserts its charged entry, so the running total
    /// follows the table exactly; raising a version the identity already had does not change the
    /// entry's size.
    fn raise_revision(&mut self, id: &str, version: u64) {
        match self.revisions.get_mut(id) {
            Some(known) => {
                if version > *known {
                    *known = version;
                }
            }
            None => {
                self.revisions.insert(id.to_owned(), version);
                self.revisions_bytes = self.revisions_bytes.saturating_add(revision_bytes(id));
            }
        }
    }

    /// Whether this projected item differs from the **persistable payload** last delivered for it.
    ///
    /// The payload is the turn binding (a queued input/item identity that is only later bound to the
    /// Turn that consumes it, so the binding is durable content and not placement metadata), the
    /// dynamic fields ([`crate::studio::runtime::chat_item::content_fields`], compared by shared
    /// chain without materializing a body) plus the structure / terminal shape
    /// ([`crate::studio::runtime::chat_item::static_state`], covering kind, attachments, lifecycle and
    /// tool state). None of them includes the `ThreadItem` timestamps that only this projection
    /// restamps, so a repeated projection keeps its version — and with it a byte-identical payload —
    /// while a real transition (turn binding, terminal admission, tool state) always reads as a
    /// change.
    fn content_changed(
        &self,
        id: &str,
        item: &ThreadItem,
        existing: &BTreeMap<String, ThreadItem>,
    ) -> bool {
        let candidate_fields = crate::studio::runtime::chat_item::content_fields(item.state());
        let candidate_shape = static_state(item.state());
        if let Some(preview) = self.previews.get(id) {
            return preview.shape != candidate_shape
                || !fields_equal(&candidate_fields, &preview.fields);
        }
        // A committed fact resident in memory wins; on cold recovery the canonical fact resolved by
        // this pass is compared the same way, so a payload identical to the stored one is not treated
        // as a first delivery and does not raise the version for nothing.
        match self.items.get(id).or_else(|| existing.get(id)) {
            Some(previous) => {
                previous.turn_id != item.turn_id
                    || static_state(previous.state()) != candidate_shape
                    || !fields_equal(
                        &candidate_fields,
                        &crate::studio::runtime::chat_item::content_fields(previous.state()),
                    )
            }
            None => true,
        }
    }

    /// Resolves one effect's identities: canonical payloads this projection holds, plus the orders a
    /// new identity takes from the session's in-memory allocator.
    fn resolve(
        &self,
        chat: &Session,
        thread: &pl_protocol::Thread,
        ids: impl IntoIterator<Item = String>,
    ) -> Result<ResolvedIdentities, ProjectionError> {
        let mut existing = BTreeMap::new();
        let mut reserved = BTreeMap::new();
        for id in ids {
            if existing.contains_key(&id) || reserved.contains_key(&id) {
                continue;
            }
            if let Some(item) = self.items.get(&id) {
                existing.insert(id, item.clone());
                continue;
            }
            if let Some(fact) = self.facts.get(&id) {
                existing.insert(id.clone(), fact.placeholder(&id, &thread.id));
                continue;
            }
            let order = self.reserve_identity_in(chat, &id)?;
            reserved.insert(id, order);
        }
        Ok((existing, reserved))
    }

    /// Whether one identity is a row this projection or the shared session already holds.
    ///
    /// Placement metadata alone does not make an identity started: a fact can outlive the preview
    /// it described (the window replaces an aggregate preview the moment the adapter itemizes the
    /// response), and reserving its name would fabricate a terminal row nothing ever showed.
    fn identity_is_started(&self, chat: &Session, id: &str) -> bool {
        self.items.contains_key(id)
            || self.previews.contains_key(id)
            || chat.assigned_order(id).is_some()
    }

    /// Provider part identities this projection already showed for one attempt.
    ///
    /// The terminal commit must close the identity the streaming frames used, so every started
    /// provider part is part of the finalize set even when the outcome's own receipt cannot carry it.
    fn started_provider_ids(&self, attempt_id: &str) -> Vec<String> {
        self.provider_previews(attempt_id)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Resident previews of one attempt's provider parts.
    ///
    /// A provider part's identity begins with its attempt's presentation prefix, and a `BTreeMap`
    /// keeps its keys in lexicographic order, so one attempt's identities are a single contiguous
    /// range: an effect that finalizes an attempt enumerates that attempt's parts instead of walking
    /// every part the response ever streamed. Only one attempt is ever resident (`stream` clears the
    /// table when the attempt changes), so the range is also bounded by the parts of that attempt.
    fn provider_previews<'a>(
        &'a self,
        attempt_id: &str,
    ) -> impl Iterator<Item = (&'a String, &'a LivePreview)> + 'a {
        let prefix = super::order::presentation_prefix(attempt_id);
        self.previews
            .range(prefix.clone()..)
            .take_while(move |(id, _)| id.starts_with(prefix.as_str()))
    }

    /// Canonical items for one attempt's in-flight provider previews.
    ///
    /// Read only for a **bounded** reception (see [`super::responses::keeps_bounded_preview`]): the
    /// shared window is one representation of an identity ([`crate::studio::runtime::chat_item`]), so
    /// the terminal projection reads the received prefix back through the same codec instead of
    /// keeping a second body copy, and [`super::responses::finalize_missing_presentation`] closes it
    /// on the same identity. A normally received response takes its body from its own receipt instead.
    /// Only provider-part identities qualify — an aggregate channel is not a provider part, and its own
    /// liveness is decided by [`LiveProjection::identity_is_started`].
    fn attempt_preview_items(
        &self,
        attempt_id: &str,
        turn_id: &str,
    ) -> Result<Vec<(String, ThreadItem)>, ProjectionError> {
        self.provider_previews(attempt_id)
            .map(|(id, preview)| {
                let item = ChatItem {
                    item_id: id.clone(),
                    turn_id: turn_id.to_owned(),
                    order: preview.order,
                    revision: self.revisions.get(id).copied().unwrap_or(0),
                    fields: preview.fields.clone(),
                    meta: preview.meta.clone(),
                    omitted_bytes: 0,
                    saved: false,
                    lifecycle: ChatLifecycle::Streaming,
                };
                crate::studio::runtime::chat_item::canonical_item(&item)
                    .map(|item| (id.clone(), item))
                    .map_err(|error| ProjectionError::History(error.to_string()))
            })
            .collect()
    }

    /// Order of a new identity: the session's own reservation first, else a new in-memory order.
    fn reserve_identity_in(&self, chat: &Session, id: &str) -> Result<u64, ProjectionError> {
        match chat.assigned_order(id) {
            Some(order) => Ok(order),
            None => chat
                .reserve_order_in_memory(id)
                .map_err(|error| ProjectionError::History(error.to_string())),
        }
    }

    /// Emits the Turn, interaction and runtime frames one committed effect produced.
    ///
    /// The frames are derived from the same effect-matched state the content was projected from, so
    /// a live subscriber and durable history describe one commit rather than two drifting views.
    fn emit_frames(
        &mut self,
        usage: &pl_core::thread::UsageSummary,
        thread: &pl_protocol::Thread,
        effect: &ThreadEffectBatch,
        state: &ThreadSnapshot,
        events: &mut Vec<LiveEvent>,
    ) -> Result<(), ProjectionError> {
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
                state,
                record,
                effect.committed_at,
                effect.committed_at,
                effect.sequence,
            )?;
            if record.state == TurnState::Running {
                self.turn_phases.insert(
                    record.turn_id.clone(),
                    super::turns::phase(state, &record.turn_id),
                );
            } else {
                self.turn_phases.remove(&record.turn_id);
            }
            events.push(LiveEvent::Turn { turn, event });
        }
        // A Turn record is only committed when a Turn starts or finishes, but a task start, a tool
        // completion or a model attempt commit changes the running Turn's derived phase. Publishing
        // each real phase transition keeps the activity row on the canonical phase instead of the
        // phase captured at Turn start. The frame carries the same revision as the item updates of
        // this effect, so a late busy frame can never outrank the terminal Turn.
        for record in self.phase_advances(state) {
            let event = if self.turns_seen.insert(record.turn_id.clone()) {
                TurnEvent::Started
            } else {
                TurnEvent::Updated
            };
            let turn = super::turns::project_turn(
                &thread.id,
                state,
                &record,
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
                state,
            ) {
                events.push(LiveEvent::Interaction(Box::new(interaction)));
            }
        }
        for permission in effect.permissions.iter() {
            if let Ok(Some(interaction)) = crate::thread_assembler::project_thread_interaction(
                &thread.id,
                &permission.id,
                state,
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
                    state,
                    effect.committed_at,
                    &summary,
                )
            {
                events.push(LiveEvent::Runtime(Box::new(runtime)));
            }
        }
        let mut active_inputs = state
            .turns
            .iter()
            .filter_map(|turn| {
                (turn.state == TurnState::Running)
                    .then_some(turn.input_id.as_deref())
                    .flatten()
            })
            .collect::<BTreeSet<_>>();
        active_inputs.extend(state.inputs.iter().map(|record| record.input.id.as_str()));
        self.hidden_inputs
            .retain(|id| active_inputs.contains(id.as_str()));
        Ok(())
    }

    /// Running Turns whose canonical phase changed since the last published Turn frame.
    fn phase_advances(&mut self, state: &ThreadSnapshot) -> Vec<TurnRecord> {
        let mut advanced = Vec::new();
        for record in state.turns.iter() {
            if record.state != TurnState::Running {
                continue;
            }
            let phase = super::turns::phase(state, &record.turn_id);
            if self.turn_phases.get(&record.turn_id) == Some(&phase) {
                continue;
            }
            self.turn_phases.insert(record.turn_id.clone(), phase);
            advanced.push(record.clone());
        }
        advanced
    }

    /// Records one projected payload and keeps its revision strictly monotonic.
    ///
    /// The ordinal is the placement this projection resolved and the revision is this projection's own
    /// content version ([`LiveProjection::revisions`]), never the effect sequence that happened to
    /// commit the payload. Live frames, resubscription, the window and the durable writer therefore
    /// agree on one content version per identity, while the effect sequence stays only the commit /
    /// reliable-save watermark. A stale payload is ignored rather than renumbered.
    fn record(&mut self, item: ThreadItem) -> Result<(), ProjectionError> {
        if item.ordinal == 0 {
            return Err(ProjectionError::ItemOrder(item.id));
        }
        if self
            .items
            .get(&item.id)
            .is_some_and(|previous| previous.revision > item.revision)
        {
            // A stale payload never regresses a newer body; it produces no frame either way.
            return Ok(());
        }
        // A committed fact replaces the same identity's preview: dropping the cache makes the next
        // streaming observation rebuild its static meta and argument partner fields from the freshly
        // committed canonical fact instead of reusing a stale preview.
        self.release_preview(&item.id);
        self.fact(&item);
        self.observe_result(&item);
        self.store_item(item);
        Ok(())
    }

    /// Keeps the placement metadata of one identity that the projection may update later.
    fn fact(&mut self, item: &ThreadItem) {
        self.store_fact(
            item.id.clone(),
            Fact {
                ordinal: item.ordinal,
                revision: item.revision,
                created_at: item.created_at,
                turn_id: item.turn_id.clone(),
            },
        );
    }

    /// Retains one committed body while it can still change or be re-projected.
    fn committed_body(&mut self, item: ThreadItem) {
        self.store_item(item);
    }

    /// Stores one canonical body, keeping the retained-byte estimate in step.
    ///
    /// Replacing a body charges the difference instead of double counting it, so the running total
    /// stays the exact value [`LiveProjection::retained_bytes`] would compute from the table.
    fn store_item(&mut self, item: ThreadItem) {
        let next = charged_item_bytes(&item);
        match self.items.insert(item.id.clone(), item) {
            Some(previous) => {
                self.items_bytes = self
                    .items_bytes
                    .saturating_sub(charged_item_bytes(&previous))
                    .saturating_add(next);
            }
            None => self.items_bytes = self.items_bytes.saturating_add(next),
        }
    }

    /// Releases one canonical body, keeping the retained-byte estimate in step.
    fn release_item(&mut self, id: &str) {
        if let Some(previous) = self.items.remove(id) {
            self.items_bytes = self
                .items_bytes
                .saturating_sub(charged_item_bytes(&previous));
        }
    }

    /// Stores one placement fact, keeping the retained-byte estimate in step.
    fn store_fact(&mut self, id: String, fact: Fact) {
        let next = fact_bytes(&id, &fact);
        let id_len = id.len() as u64;
        match self.facts.insert(id, fact) {
            Some(replaced) => {
                let replaced_bytes = id_len
                    .saturating_add(replaced.turn_id.len() as u64)
                    .saturating_add(RETAINED_ENTRY_OVERHEAD);
                self.facts_bytes = self
                    .facts_bytes
                    .saturating_sub(replaced_bytes)
                    .saturating_add(next);
            }
            None => self.facts_bytes = self.facts_bytes.saturating_add(next),
        }
    }

    /// Stores one in-flight preview by projection id, keeping the byte estimate and the short-key
    /// observation index in step.
    fn store_preview(&mut self, id: String, entry: LivePreview) {
        // One formula scores both sides of the replacement (`preview_entry_bytes`), so the running
        // total moves by the exact difference instead of re-deriving a smaller hand-copied estimate
        // that could omit the observation-index term.
        let next = preview_bytes(&id, &entry);
        let id_len = id.len() as u64;
        let observed = entry.observed.clone();
        let version = entry.observed_version;
        match self.previews.insert(id, entry) {
            Some(replaced) => {
                let replaced_bytes = preview_entry_bytes(id_len, &replaced);
                self.previews_bytes = self
                    .previews_bytes
                    .saturating_sub(replaced_bytes)
                    .saturating_add(next);
                // One projection id is a function of one observation identity, so a replacement under
                // it always carries the same short key and this branch is unreachable in the current
                // identity derivation. It is still handled: a replaced key whose mirror entry would
                // otherwise outlive its preview is dropped here instead of quietly making the hot
                // path skip an identity the preview map no longer holds.
                if replaced.observed != observed
                    && let Some(stale) = &replaced.observed
                {
                    self.observed.remove(stale);
                }
            }
            None => self.previews_bytes = self.previews_bytes.saturating_add(next),
        }
        // The index mirrors the delivered entry's own version, so the hot path can compare it
        // without rebuilding the projection id.
        if let Some(observed) = observed {
            self.observed.insert(observed, version);
        }
    }

    /// Releases one in-flight preview by projection id, keeping the byte estimate and the short-key
    /// observation index in step.
    fn release_preview(&mut self, id: &str) {
        let Some(removed) = self.previews.remove(id) else {
            return;
        };
        self.previews_bytes = self
            .previews_bytes
            .saturating_sub(preview_bytes(id, &removed));
        if let Some(observed) = &removed.observed {
            self.observed.remove(observed);
        }
    }

    /// Releases every in-flight preview, e.g. when the observed attempt changes.
    fn clear_previews(&mut self) {
        self.previews.clear();
        self.observed.clear();
        // The retained bytes of an empty table, by the same `preview_entry_bytes` formula.
        self.previews_bytes = 0;
    }

    /// Adds one committed payload to the report facts of its Turn.
    fn observe_result(&mut self, item: &ThreadItem) {
        if item.turn_id.is_empty() {
            return;
        }
        self.results
            .entry(item.turn_id.clone())
            .or_default()
            .observe(item);
    }

    fn created_at(&self, id: &str, fallback: i64) -> i64 {
        self.facts.get(id).map_or(fallback, |fact| fact.created_at)
    }

    /// Bounds the live tables to the facts a later effect can still reference.
    ///
    /// The projection outlives many Turns, so it keeps only the newest Turn's facts plus the
    /// identities the current owner state still references. Anything older is a durable history fact
    /// a reconnect reads from SQL, and a body released here is replaced by its placement metadata so
    /// the identity itself is never mistaken for a new item.
    fn retain(&mut self, state: &ThreadSnapshot) {
        let mut live = state
            .turns
            .iter()
            .filter(|turn| turn.state == TurnState::Running)
            .map(|turn| turn.turn_id.clone())
            .collect::<BTreeSet<_>>();
        if let Some(newest) = state.turns.last() {
            live.insert(newest.turn_id.clone());
        }
        let mut referenced = state
            .tasks
            .values()
            .filter(|task| task.status == pl_core::thread::task::TaskStatus::Running)
            .map(|task| super::order::tool_id(&task.call_id))
            .collect::<BTreeSet<_>>();
        referenced.extend(state.inputs.iter().map(|record| record.input.id.clone()));
        for attempt in state.attempts.iter() {
            referenced.insert(super::order::response_id(&attempt.attempt_id, "inference"));
            for channel in ["reasoning", "text"] {
                referenced.insert(super::attempt_channel_id(&attempt.attempt_id, channel));
            }
            // A tool result whose call belongs to a still-resident attempt is part of the same
            // unfinished Turn: a later effect of that Turn re-references the tool identity, so it is
            // as required as the attempt's own channels.
            if let pl_core::thread::AttemptOutcome::Committed(output) = &attempt.outcome {
                referenced.extend(
                    output
                        .tool_calls
                        .iter()
                        .map(|call| super::order::tool_id(&call.call_id)),
                );
            }
        }
        // A result still owed to model context is delivered by a later effect of a live Turn, so its
        // tool identity is required until that delivery commits.
        referenced.extend(
            state
                .deliveries
                .iter()
                .map(|delivery| super::order::tool_id(&delivery.call_id)),
        );
        // A live Turn's own facts stay required for every later effect of that Turn: the input that
        // opened it (named by `turns[].input_id` after the consuming commit pruned the resident
        // record), and the Turn item itself, which each later task / phase / terminal commit
        // re-projects. Both must keep their placement fact, because an identity released here is
        // resolved again with a fresh content version: the input would surface as a hole, and the
        // re-projected Turn item would restart at revision 1 and the durable row would reject the
        // new payload as a revision conflict. The window therefore only ever releases *optional*
        // history, never an identity a live Turn still references.
        for turn in state.turns.iter() {
            if !live.contains(&turn.turn_id) {
                continue;
            }
            referenced.insert(super::order::turn_id(&turn.turn_id));
            if let Some(input_id) = &turn.input_id {
                referenced.insert(input_id.clone());
            }
        }
        let keep = |id: &str, turn_id: &str| live.contains(turn_id) || referenced.contains(id);
        self.facts.retain(|id, fact| keep(id, &fact.turn_id));
        self.results.retain(|turn_id, _| live.contains(turn_id));
        self.turn_phases.retain(|turn_id, _| live.contains(turn_id));
        self.turns_seen.retain(|turn_id| live.contains(turn_id));
        self.items.retain(|id, item| {
            let retained = match self.facts.get(id) {
                Some(fact) => keep(id, &fact.turn_id),
                None => false,
            };
            // A body is only worth its bytes while it can still change: a non-terminal item is
            // updated by a later effect, the newest Turn's tool items are re-projected by a late
            // delivery, and everything else keeps placement metadata only.
            retained
                && (!item.state().is_terminal() || item.kind() == pl_protocol::ThreadItemKind::Tool)
        });
        // A Turn that opened more identities than the window is still bounded here: the oldest
        // non-terminal bodies and tool facts are dropped by canonical placement, and the identity
        // itself stays as placement metadata so a later effect still resolves it.
        if self.facts.len() > LIVE_ITEM_WINDOW {
            let mut placed = self
                .facts
                .iter()
                .map(|(id, fact)| (fact.ordinal, id.clone()))
                .collect::<Vec<_>>();
            placed.sort_unstable();
            let mut newest = placed
                .into_iter()
                .rev()
                .take(LIVE_ITEM_WINDOW)
                .map(|(_, id)| id)
                .collect::<BTreeSet<_>>();
            // The window bounds *optional* retained history; the identities the live snapshot still
            // requires (a live Turn's input, the inference/channel and tool identities of its
            // resident attempts, running tasks and undelivered results) are not optional. Numbering
            // by ordinal would evict them exactly when a Turn produced more parts than the window, so
            // they are always kept on top of the newest `LIVE_ITEM_WINDOW` others.
            newest.extend(
                referenced
                    .iter()
                    .filter(|id| self.facts.contains_key(*id))
                    .cloned(),
            );
            self.facts.retain(|id, _| newest.contains(id));
            self.items.retain(|id, _| newest.contains(id));
        }
        // An in-flight preview belongs to an identity still inside the retained window; once its
        // placement fact is gone it can no longer be resolved or committed, so its body is released
        // with it. The content versions follow the same bounded window and are not a second history:
        // an identity evicted here recovers its baseline from the canonical fact
        // ([`LiveProjection::adopt_revisions`]) or from the shared session
        // ([`LiveProjection::adopt_session_revision`]) instead of restarting at 1.
        self.previews.retain(|id, _| self.facts.contains_key(id));
        self.revisions
            .retain(|id, _| self.items.contains_key(id) || self.previews.contains_key(id));
        // The pruned tables are the authority again: re-deriving the running retained-byte totals and
        // the short-key observation index here means a bounded window can never leave either carrying
        // an entry the tables no longer hold. This runs once per commit, not per streaming frame, so
        // it stays off the hot path it keeps cheap.
        self.rederive_totals();
    }

    /// Re-derives the running retained-byte totals and the observation index from the live tables.
    fn rederive_totals(&mut self) {
        self.items_bytes = self.items.values().fold(0_u64, |total, item| {
            total.saturating_add(charged_item_bytes(item))
        });
        self.facts_bytes = self.facts.iter().fold(0_u64, |total, (id, fact)| {
            total.saturating_add(fact_bytes(id, fact))
        });
        self.previews_bytes = self.previews.iter().fold(0_u64, |total, (id, preview)| {
            total.saturating_add(preview_bytes(id, preview))
        });
        self.revisions_bytes = self
            .revisions
            .keys()
            .fold(0_u64, |total, id| total.saturating_add(revision_bytes(id)));
        self.observed.clear();
        for preview in self.previews.values() {
            if let Some(observed) = &preview.observed {
                self.observed
                    .insert(observed.clone(), preview.observed_version);
            }
        }
    }
}

/// Reads the already-committed body of a repaired identity the live window no longer holds.
///
/// A reliable-output repair names an already-committed call, but the bounded live window may already
/// have released that identity — no GUI open, a slow reader, or a rolled-over window. The Thread's
/// single projection owner stays the only fact source for the canonical item and its content version,
/// so it reads the committed body from history here and folds it back into its own tables. The
/// following projection then supplements that identity in memory and the writer only confirms the
/// projected item; no content version is invented outside this owner, nothing re-reads the effect being
/// written, and a repeated repair of the same effect finds the identity already resident.
///
/// # Errors
/// Returns a projection failure when the already-committed body cannot be read or decoded.
pub(in crate::studio) async fn seed_repaired_targets(
    projection: &mut LiveProjection,
    history: &crate::studio::storage::history::HistoryStore,
    effect: &ThreadEffectBatch,
) -> Result<(), ProjectionError> {
    if effect.delivery_repairs.is_empty() {
        return Ok(());
    }
    let mut seeded = Vec::new();
    for repair in effect.delivery_repairs.iter() {
        let tool_id = super::order::tool_id(&repair.call_id);
        if projection.holds_terminal_tool(&tool_id) {
            continue;
        }
        if let Some(item) = history
            .item(&tool_id)
            .await
            .map_err(|error| ProjectionError::History(error.to_string()))?
        {
            seeded.push(item);
        }
    }
    if !seeded.is_empty() {
        projection.seed(seeded, std::collections::BTreeSet::new());
    }
    Ok(())
}

/// The canonical identity of one aggregate streaming channel of an attempt.
///
/// It is the same in-memory identity the committed effect later finalizes for the whole channel, so a
/// preview and the terminal item it precedes never disagree on the id.
fn aggregate_id(attempt_id: &str, channel: AggregateChannel) -> String {
    match channel {
        AggregateChannel::Text => super::order::response_id(attempt_id, "text"),
        AggregateChannel::Reasoning => super::order::response_id(attempt_id, "reasoning"),
    }
}

/// Projects one live observation into its stable item identity, content field and static structure.
///
/// The identity comes from the observation's own stable fact — a request-level aggregate channel, or a
/// provider item id plus part discriminator — never from the provider payload or a late output index.
/// The returned `ThreadItemState` carries **no body**: it only feeds [`static_meta`] so the static
/// structure is encoded once, while the body is handed out by the caller straight from the observation's
/// shared `Arc<ContentBlock>`, so the hot path never calls `part.text()`.
///
/// The field index is fixed at `0`: one presentation id owns exactly one part, and the commit side
/// `project_presentation_items` puts that part into a chunk vector of length 1, so the streaming and
/// terminal field identities agree and a streaming `SummaryText(n)` never misaligns with a terminal
/// reasoning chunk. `None` reports a part that cannot belong to its item kind instead of inventing a
/// row.
fn observation_preview(
    attempt_id: &str,
    part: &ObservedPart,
) -> Option<(String, ChatField, ThreadItemState)> {
    Some(match part.identity() {
        ObservedPartIdentity::Aggregate { channel } => {
            let id = aggregate_id(attempt_id, *channel);
            match channel {
                AggregateChannel::Text => (
                    id,
                    ChatField::Body,
                    ThreadItemState::Text(ThreadTextItem::new(
                        ThreadTextChannel::Commentary,
                        String::new(),
                        Vec::new(),
                        ThreadContentLifecycle::streaming(),
                    )),
                ),
                AggregateChannel::Reasoning => (
                    id,
                    ChatField::Part(PresentationPart::ReasoningText(0)),
                    ThreadItemState::Thinking(ThreadThinkingItem::new(
                        Vec::new(),
                        vec![String::new()],
                        ThreadContentLifecycle::streaming(),
                    )),
                ),
            }
        }
        ObservedPartIdentity::Provider(identity) => {
            let id = super::order::presentation_id(
                attempt_id,
                identity.item_id.as_ref(),
                Some(identity.presentation_part()),
            );
            match (identity.item_kind, identity.part) {
                (ObservedItemKind::Text(channel), ObservedPartKind::OutputText) => (
                    id,
                    ChatField::Body,
                    ThreadItemState::Text(ThreadTextItem::new(
                        trace_channel(channel),
                        String::new(),
                        Vec::new(),
                        ThreadContentLifecycle::streaming(),
                    )),
                ),
                (ObservedItemKind::Reasoning, ObservedPartKind::ReasoningText) => (
                    id,
                    ChatField::Part(PresentationPart::ReasoningText(0)),
                    ThreadItemState::Thinking(ThreadThinkingItem::new(
                        Vec::new(),
                        vec![String::new()],
                        ThreadContentLifecycle::streaming(),
                    )),
                ),
                (ObservedItemKind::Reasoning, ObservedPartKind::SummaryText) => (
                    id,
                    ChatField::Part(PresentationPart::SummaryText(0)),
                    ThreadItemState::Thinking(ThreadThinkingItem::new(
                        vec![String::new()],
                        Vec::new(),
                        ThreadContentLifecycle::streaming(),
                    )),
                ),
                _ => return None,
            }
        }
    })
}

/// Maps the core observation channel vocabulary onto the protocol text channel.
fn trace_channel(channel: ModelTextChannel) -> ThreadTextChannel {
    match channel {
        ModelTextChannel::User => ThreadTextChannel::User,
        ModelTextChannel::Commentary => ThreadTextChannel::Commentary,
        ModelTextChannel::Final => ThreadTextChannel::Final,
    }
}

/// Whether two field maps carry byte-identical text, compared by shared chain without materializing a
/// body. The field set itself participates: dropping or adding a field is a structural change.
fn fields_equal(
    left: &BTreeMap<ChatField, Arc<ContentBlock>>,
    right: &BTreeMap<ChatField, Arc<ContentBlock>>,
) -> bool {
    left.len() == right.len()
        && left.iter().all(|(field, block)| {
            right
                .get(field)
                .is_some_and(|other| ContentBlock::text_eq(block, other))
        })
}
