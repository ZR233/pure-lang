//! Public Thread state, input contracts and typed outcomes.
use super::*;

/// Upper bound on how many terminal input identities stay resident for same-incarnation
/// idempotency. History keeps the authoritative identity; the host answers older repeats from its
/// durable identity index, so this ledger is a fixed window and never grows with history.
const TERMINAL_INPUT_LEDGER_CAPACITY: usize = 64;

/// Upper bound on how many consumed message identities stay resident for same-incarnation
/// idempotency. History keeps the accepted message; this ledger only answers a repeated delivery
/// of a stable message identity, so it is a fixed window and never grows with history.
const CONSUMED_MESSAGE_LEDGER_CAPACITY: usize = 64;

/// Upper bound on how many terminal task identities stay resident. History keeps the authoritative
/// task lifecycle; this ledger only answers an already-finished cancellation and read-only
/// inspection with the durable task identity, so it is a fixed window and never grows with history.
const TERMINAL_TASK_LEDGER_CAPACITY: usize = 64;

/// One committed effect batch as retained by the live window.
#[derive(Default)]
struct EffectWindowState {
    batches: std::collections::VecDeque<Arc<ThreadEffectBatch>>,
    /// Highest `sequence` already handed off to durable history. Every commit at or below it has
    /// been released, so a consumer behind it resynchronizes from the durable store instead of
    /// reading the exact live batch.
    durable_through: u64,
}

/// Shared transient live effect window: one [`ThreadEffectBatch`] per commit that is not durable
/// yet.
///
/// The owner and its handles read the newest committed interaction, permission or tool delivery
/// from this window instead of keeping a checkpoint-visible payload ledger. It is the same buffer
/// live observers consume, it is never serialized into `state.toml`, and its released entries are
/// answered by the host's durable identity index and history/calls reader.
///
/// # Bound and release semantics
/// The window retains exactly the write batches that are neither admitted-and-persisted nor
/// released, so its resident size is the same quantity the storage pressure budget already bounds:
/// when a Thread accumulates too many not-yet-durable bytes the owner pauses further admission
/// (see `refresh_storage_pressure`) instead of dropping an accepted fact here. As soon as a fixed
/// durable watermark is confirmed the covered batches are released immediately, and a consumer that
/// still needs them reads the durable history/calls store through the documented host contract.
///
/// The window therefore performs no serialization and no byte accounting of its own: a commit only
/// clones an `Arc` into the deque, so publishing never blocks on encoding a full body.
pub(crate) struct EffectWindow {
    state: std::sync::RwLock<EffectWindowState>,
}

impl Default for EffectWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for EffectWindow {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never block a diagnostic print behind an in-progress commit, mirroring `RwLock`'s own
        // `Debug` contract.
        match self.state.try_read() {
            Ok(state) => formatter
                .debug_struct("EffectWindow")
                .field("batches", &state.batches.len())
                .field("durable_through", &state.durable_through)
                .finish(),
            Err(_) => formatter.write_str("EffectWindow { <locked> }"),
        }
    }
}

impl EffectWindow {
    pub(crate) fn new() -> Self {
        Self {
            state: std::sync::RwLock::new(EffectWindowState::default()),
        }
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, EffectWindowState> {
        self.state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, EffectWindowState> {
        self.state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Retains one committed batch until its durable handoff releases it.
    pub(super) fn push(&self, batch: Arc<ThreadEffectBatch>) {
        self.write().batches.push_back(batch);
    }

    /// Releases every batch covered by a confirmed durable watermark.
    ///
    /// The handoff is the moment the facts become answerable from the durable store, so the live
    /// bodies are dropped immediately rather than cached as a second history (`design/15` §15.1).
    /// A consumer that has not consumed them yet observes an explicit gap and resynchronizes from
    /// history/calls instead of reading a stale body or losing the fact.
    pub(super) fn release_through(&self, durable_sequence: u64) {
        let mut state = self.write();
        if durable_sequence > state.durable_through {
            state.durable_through = durable_sequence;
        }
        while state
            .batches
            .front()
            .is_some_and(|batch| batch.sequence <= state.durable_through)
        {
            state.batches.pop_front();
        }
    }

    /// First commit sequence the window can still serve, or `None` while it can serve everything.
    ///
    /// After a durable handoff the released commits are no longer resident, so the frontier moves
    /// past them: a consumer behind it resynchronizes from durable history.
    pub(super) fn start(&self) -> Option<u64> {
        self.read().frontier()
    }

    /// One atomic page read: either the retained batches strictly after `after`, or an explicit gap.
    ///
    /// The gap check and the copy happen under one lock, so a concurrent durable release can never
    /// turn "no gap" into a silently empty page. The copy is bounded by `limit`, so reading a page
    /// never duplicates a whole window.
    pub(super) fn page_after(&self, after: u64, limit: usize) -> WindowPage {
        let state = self.read();
        if state
            .frontier()
            .is_some_and(|frontier| after.saturating_add(1) < frontier)
        {
            return WindowPage::Gap;
        }
        WindowPage::Page(
            state
                .batches
                .iter()
                .filter(|batch| batch.sequence > after)
                .take(limit)
                .cloned()
                .collect(),
        )
    }

    /// Every retained (not yet durable) batch, oldest first.
    pub(super) fn retained(&self) -> Vec<Arc<ThreadEffectBatch>> {
        self.read().batches.iter().cloned().collect()
    }
}

/// One atomic read of the transient effect window.
pub(super) enum WindowPage {
    /// Retained batches strictly after the requested sequence, oldest first.
    Page(Vec<Arc<ThreadEffectBatch>>),
    /// The requested sequence is behind the released durable frontier, so the caller must
    /// resynchronize from durable history/calls.
    Gap,
}

impl EffectWindowState {
    /// First commit sequence the window can still serve, if anything was ever released.
    fn frontier(&self) -> Option<u64> {
        self.batches
            .front()
            .map(|batch| batch.sequence)
            .or_else(|| (self.durable_through > 0).then(|| self.durable_through + 1))
    }
}

/// Returns the newest committed fact of one kind still inside the bounded [`EffectWindow`].
///
/// Terminal interaction, permission and delivery bodies are committed history: they must not be
/// cached inside the serialized snapshot. An identical repeated command or a read-only task
/// inspection returns the exact committed body while that commit is still not durable; once the
/// durable handoff has released it, the host answers from the durable identity index and
/// history/calls store.
pub(crate) fn recent_effect_fact<T>(
    window: &EffectWindow,
    mut select: impl FnMut(&ThreadEffectBatch) -> Option<T>,
) -> Option<T> {
    let window = window.read();
    window.batches.iter().rev().find_map(|batch| select(batch))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(sequence: u64) -> Arc<ThreadEffectBatch> {
        Arc::new(ThreadEffectBatch {
            thread_id: "t".to_string(),
            sequence,
            ..ThreadEffectBatch::default()
        })
    }

    /// A confirmed durable watermark releases the covered bodies immediately and leaves the
    /// watermark below them.
    #[test]
    fn durable_handoff_releases_covered_bodies_immediately() {
        let window = EffectWindow::new();
        for sequence in 1..=3 {
            window.push(batch(sequence));
        }
        assert_eq!(window.retained().len(), 3);
        assert_eq!(window.start(), Some(1));
        let WindowPage::Page(page) = window.page_after(0, 10) else {
            panic!("nothing is missing yet");
        };
        assert_eq!(page.len(), 3);
        // Confirming the first two commits durable releases their full bodies at once.
        window.release_through(2);
        assert_eq!(window.retained().len(), 1);
        assert_eq!(window.start(), Some(3));
        assert!(
            matches!(window.page_after(1, 10), WindowPage::Gap),
            "a consumer at 1 must resynchronize from durable history"
        );
        let WindowPage::Page(page) = window.page_after(2, 10) else {
            panic!("a consumer at 2 can continue from the retained head");
        };
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].sequence, 3);
    }

    /// A fully released window still reports the frontier, and a not-yet-durable commit is never
    /// dropped by the window itself (no entry/byte eviction can lose an accepted fact).
    #[test]
    fn undurable_commits_are_never_evicted_by_the_window() {
        let window = EffectWindow::new();
        for sequence in 1..=256 {
            window.push(batch(sequence));
        }
        assert_eq!(window.retained().len(), 256);
        window.release_through(256);
        assert!(window.retained().is_empty());
        assert_eq!(window.start(), Some(257));
        let WindowPage::Page(page) = window.page_after(256, 10) else {
            panic!("a fully caught-up consumer sees an empty page, not a gap");
        };
        assert!(page.is_empty());
        assert!(matches!(window.page_after(0, 10), WindowPage::Gap));
    }
}

/// Current owner state published after each accepted request or model-output commit.
///
/// A snapshot holds only the facts that current logical execution and live observation still
/// need: current context, unfinished execution, queued input, pending interactions, permission
/// ownership, undelivered results and the current runtime facts. Finished attempts/turns,
/// delivered results, replaced context versions, completed interactions and exported effect
/// deltas live in history and are handed out through [`super::ThreadEffectBatch`] instead. A few
/// bounded ledgers keep the minimal identity of facts that an identical repeated command, a
/// finished task's read-only inspection or a retry still has to answer. Finished interaction,
/// permission and tool-result bodies are never kept here: while their commit is still inside the
/// bounded live [`EffectWindow`] a repeat reads the exact body from there, and older identities are
/// answered by the host's durable identity index (see the W6-1 handoff contract).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSnapshot {
    #[serde(skip)]
    pub model_progress: Option<crate::model::ActiveModelProgress>,
    /// Ephemeral producer previews for running tasks; never persisted or used as model context.
    #[serde(skip)]
    pub tool_progress: std::collections::BTreeMap<String, Vec<ContextContent>>,
    /// Permission state that is still owned by a live task; finished approvals are history.
    #[serde(default)]
    pub permissions: std::collections::BTreeMap<String, permissions::PermissionRecord>,
    /// Commit-export buffer for permission changes; drained by the commit that exports it.
    #[serde(skip)]
    pub permission_changes: Arc<[permissions::PermissionRecord]>,
    #[serde(default)]
    pub wake_messages_through: u64,
    #[serde(skip)]
    pub input_execution: input::InputExecution,
    /// Accepted input and its routing receipt; only pending entries stay resident as the
    /// execution queue, while a bounded tail of terminal identities lives in
    /// [`Self::terminal_inputs`].
    #[serde(default)]
    pub inputs: Arc<[input::InputRecord]>,
    /// Monotonic next input ordinal. Kept across pruning so a terminal identity never releases its
    /// queue position to a later input.
    #[serde(default)]
    pub input_ordinal: u64,
    /// Bounded tail of minimal identities for admitted inputs that already reached a terminal
    /// state.
    ///
    /// Content is not retained: a repeated submission matches the stored digest and is answered
    /// from the receipt, while a conflicting body remains an identity conflict. Only the newest
    /// [`TERMINAL_INPUT_LEDGER_CAPACITY`] entries stay resident so the ledger cannot grow with
    /// history; older identities are answered by the host's durable identity index in
    /// `history.sqlite`, never by moving history entities back into core.
    #[serde(default)]
    pub terminal_inputs: Arc<[input::InputIdentity]>,
    /// Commit-export buffer for input changes; drained by the commit that exports it.
    #[serde(skip)]
    pub input_changes: Arc<[input::InputChange]>,
    /// Live session availability; replay never constructs physical model resources.
    #[serde(skip)]
    pub model_available: bool,
    /// Completed executions retained in memory until their atomic result commit succeeds.
    #[serde(skip)]
    pub pending_tool_commits: Vec<String>,
    /// Tasks that are still running or whose result has not reached model context yet.
    #[serde(default)]
    pub tasks: std::collections::BTreeMap<String, task::TaskRecord>,
    /// Commit-export buffer for task revisions; drained by the commit that exports it.
    #[serde(skip)]
    pub task_changes: Arc<[task::TaskRecord]>,
    pub commit_sequence: u64,
    pub persistence: cold::PersistenceState,
    pub lifecycle: ThreadLifecycle,
    pub context: ContextSnapshot,
    /// Attempts of turns that are still unfinished; their identity and order are needed for
    /// duplicate-call rejection, retry and correction inside the live Turn.
    pub attempts: Arc<[RequestAttempt]>,
    /// Tool call identity to owning Turn, retained only while that Turn is resident.
    ///
    /// Duplicate-call rejection reads this bounded ledger instead of the whole context history.
    /// [`Self::retain_live_facts`] rebuilds it from resident attempts and context records every
    /// commit, so it never outlives the Turn whose retry could still reuse the identity.
    #[serde(default)]
    pub live_calls: std::collections::BTreeMap<String, String>,
    /// Usage observed for the newest attempt, so the next Turn still sees the previous request.
    #[serde(default)]
    pub last_attempt_usage: Option<crate::model::ModelUsage>,
    pub discovered_tools: Arc<[ModelToolDeclaration]>,
    /// Turns that are still running; a finished Turn is a history fact.
    pub turns: Arc<[TurnRecord]>,
    pub private_context: Option<OpaquePayload>,
    /// Results that have not reached model context yet; delivered results are history.
    pub deliveries: Arc<[ToolDelivery]>,
    /// Commit-export buffer for context replacements; never resident across a published commit.
    #[serde(skip)]
    pub context_replacements: Arc<[ContextReplacement]>,
    /// Complete current fact set, one entry per stable host source.
    pub runtime_facts: Arc<[RuntimeFact]>,
    /// Current opaque application records; superseded revisions are history.
    pub extensions: std::collections::BTreeMap<String, extensions::ExtensionRecord>,
    pub extension_sequence: u64,
    /// Unconsumed messages, ordered by sequence; consumed entries are pruned and referenced only
    /// by [`Self::consumed_messages`], so the resident queue is bounded by pending notifications.
    pub inbox: Arc<[inbox::InboxRecord]>,
    pub consumed_messages: u64,
    /// Highest message sequence this Thread ever admitted; kept across consumption pruning and the
    /// cold checkpoint like [`Self::input_ordinal`].
    ///
    /// The durable message timeline keys on the admission sequence, and [`Self::consumed_messages`]
    /// is a watermark over that same sequence space, so the value must never be re-derived from the
    /// resident queue alone: once consumption drained the queue it is empty, yet the next admitted
    /// message still has to continue the sequence. Restarting it at one would either overwrite an
    /// earlier parent message with the same timeline key or leave the new message unable to wake the
    /// driver, because it no longer sorts after the consumption watermark.
    #[serde(default)]
    pub inbox_sequence: u64,
    /// Bounded tail of minimal identities for messages the owner already consumed.
    ///
    /// The body is not retained: a repeated delivery matches the stored digest and is answered from
    /// this ledger, while the same identity carrying a different body remains an identity conflict.
    /// Only the newest [`CONSUMED_MESSAGE_LEDGER_CAPACITY`] entries stay resident so the ledger
    /// cannot grow with history.
    #[serde(default)]
    pub consumed_message_identities: Arc<[inbox::MessageIdentity]>,
    /// Interactions the host still has to answer; resolved or cancelled ones are history.
    pub interactions: std::collections::BTreeMap<String, interactions::InteractionRecord>,
    /// Commit-export buffer for interaction revisions; drained by the commit that exports it.
    #[serde(skip)]
    pub interaction_changes: Arc<[interactions::InteractionRecord]>,
    /// Bounded tail of terminal task identities linked to their committed result.
    ///
    /// A finished task leaves the resident set, but an already-finished cancellation acknowledgement
    /// and read-only inspection must still answer from its durable identity instead of reviving the
    /// task. Only the newest [`TERMINAL_TASK_LEDGER_CAPACITY`] identities stay resident; a task
    /// record carries no business payload, so this ledger is a minimal identity contract.
    #[serde(default)]
    pub terminal_tasks: Arc<[task::TaskRecord]>,
    /// Commit-export buffer for extension changes; drained by the commit that exports it.
    #[serde(skip)]
    pub extension_changes: Arc<[extensions::ExtensionChange]>,
    /// Constant-size cumulative model accounting for this Thread.
    ///
    /// Current execution only keeps unfinished attempts, so the totals cannot be re-aggregated from
    /// resident attempts. This summary is the single cumulative fact: the durable writer folds each
    /// committed effect into it exactly once and stores the absolute result here (and therefore in
    /// the checkpoint), so a live subscription, a reconnect after window eviction and a cold
    /// restore all read one value instead of a value that depends on what is still resident.
    #[serde(default)]
    pub usage_summary: UsageSummary,
}

/// One frozen monetary contribution in a [`UsageSummary`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageCost {
    pub currency: String,
    pub amount: f64,
}

/// Constant-size, idempotently-updated cumulative model accounting for one Thread.
///
/// Every field is an absolute total, never a delta, and `applied_sequence` records the highest
/// effect sequence already folded, so folding the same effect twice is a no-op instead of a double
/// count.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    /// Highest committed effect sequence already folded into this summary.
    #[serde(default)]
    pub applied_sequence: u64,
    /// Newest admitted model binding, projected before any response exists.
    #[serde(default)]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub inference_count: u64,
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub cached_prompt_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    /// Valid cache samples only: input tokens, and the cache reads inside them.
    #[serde(default)]
    pub cache_input_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub latest_context_tokens: u64,
    #[serde(default)]
    pub has_incomplete_usage: bool,
    #[serde(default)]
    pub has_unpriced_usage: bool,
    #[serde(default)]
    pub cache_incomplete: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub estimated_costs: Vec<UsageCost>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub estimated_cache_savings: Vec<UsageCost>,
    /// Output tokens of the newest Turn, reset when a new Turn starts.
    #[serde(default)]
    pub turn_completion_tokens: u64,
    /// Decode time of the newest Turn, reset when a new Turn starts.
    #[serde(default)]
    pub turn_decode_millis: u64,
    /// Turn whose per-turn counters are currently accumulated.
    #[serde(default)]
    pub turn_id: String,
}

impl ThreadSnapshot {
    /// Clears volatile execution handles and observation buffers that are never persisted.
    ///
    /// This is the part shared by every captured state: the restart DTO additionally drops
    /// terminal facts with [`Self::retain_live_facts`], while the effect-matched transfer state
    /// keeps them so a history writer can project its own effect.
    pub(crate) fn clear_ephemeral(&mut self) {
        self.model_progress = None;
        self.tool_progress.clear();
        self.model_available = false;
        self.pending_tool_commits.clear();
        self.input_execution = Default::default();
        self.persistence = Default::default();
    }

    /// Drops terminal execution facts and already-exported history from this snapshot.
    ///
    /// After this call the snapshot holds only facts that current execution and live observation
    /// still own. The matching [`super::ThreadEffectBatch`] remains the only copy of everything
    /// dropped here, so history and call records are unaffected.
    pub(crate) fn retain_live_facts(&mut self) {
        // Export buffers are consumed by the commit that publishes them.
        self.permission_changes = Default::default();
        self.input_changes = Default::default();
        self.task_changes = Default::default();
        self.extension_changes = Default::default();
        self.interaction_changes = Default::default();
        self.context_replacements = Default::default();

        let newest_usage = self
            .attempts
            .last()
            .and_then(RequestAttempt::usage)
            .cloned();
        if newest_usage.is_some() {
            self.last_attempt_usage = newest_usage;
        }

        retain_arc_slice(&mut self.turns, |turn| turn.state == TurnState::Running);
        let running_turns = self
            .turns
            .iter()
            .map(|turn| turn.turn_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        // A Turn that is still running keeps all of its attempts for retry, correction and
        // duplicate-call rejection. An in-flight attempt is current execution even without a Turn
        // record: like a running task it owns a live producer preview, so it stays resident until
        // its outcome commits and observation can advertise that preview. A Turn whose record
        // already left the resident set keeps only its newest retryable attempt, which is the single
        // bounded seed `retry_attempt` reuses; every other finished attempt is history.
        let retry_seed = self
            .attempts
            .iter()
            .rev()
            .find(|attempt| !running_turns.contains(attempt.turn_id.as_str()))
            .filter(|attempt| {
                matches!(
                    &attempt.outcome,
                    AttemptOutcome::Failed(_) | AttemptOutcome::Cancelled { .. }
                )
            })
            .map(|attempt| attempt.attempt_id.clone());
        retain_arc_slice(&mut self.attempts, |attempt| {
            running_turns.contains(attempt.turn_id.as_str())
                || matches!(&attempt.outcome, AttemptOutcome::Running)
                || retry_seed.as_deref() == Some(attempt.attempt_id.as_str())
        });
        // A call identity is only needed while its Turn can still retry or correct an attempt.
        // Rebuild from resident facts so the ledger cannot outlive the Turn that owns it.
        let mut live_calls = std::collections::BTreeMap::new();
        for record in self.context.records.iter() {
            if let Some(turn_id) = record.turn_id.as_ref()
                && running_turns.contains(turn_id.as_str())
            {
                for call in record.tool_calls.iter() {
                    live_calls.insert(call.call_id.clone(), turn_id.clone());
                }
            }
        }
        for attempt in self.attempts.iter() {
            if let AttemptOutcome::Committed(output) = &attempt.outcome {
                for call in output.tool_calls.iter() {
                    live_calls.insert(call.call_id.clone(), attempt.turn_id.clone());
                }
            }
        }
        self.live_calls = live_calls;

        // A resolved or cancelled interaction is committed history: its complete record is already
        // in the matching effect batch inside the bounded live [`EffectWindow`], so the snapshot
        // keeps only the still-pending map instead of a checkpoint-visible payload ledger.
        self.interactions
            .retain(|_, record| record.state == interactions::InteractionState::Pending);

        // Terminal inputs keep their identity contract without retaining their bodies, bounded to
        // the newest window; older identities are answered from the host's durable index.
        let newest_ordinal = self
            .inputs
            .iter()
            .map(|record| record.ordinal)
            .max()
            .unwrap_or(0);
        self.input_ordinal = self.input_ordinal.max(newest_ordinal);
        let mut terminal = self.terminal_inputs.to_vec();
        for record in self.inputs.iter() {
            if record.state == input::InputState::Pending {
                continue;
            }
            if terminal
                .iter()
                .any(|identity| identity.id == record.input.id)
            {
                continue;
            }
            terminal.push(input::InputIdentity::from_record(record));
        }
        retain_arc_slice(&mut self.inputs, |record| {
            record.state == input::InputState::Pending
        });
        let excess = terminal
            .len()
            .saturating_sub(TERMINAL_INPUT_LEDGER_CAPACITY);
        if excess > 0 {
            terminal.drain(..excess);
        }
        self.terminal_inputs = terminal.into();

        // A consumed message is referenced by its watermark; only the pending queue stays resident.
        let consumed_messages = self.consumed_messages;
        // The admitted sequence outlives the resident queue: a message that leaves the queue as
        // consumed must never release its sequence to a later message.
        self.inbox_sequence = self
            .inbox_sequence
            .max(self.inbox.last().map_or(0, |record| record.sequence))
            .max(consumed_messages);
        // Consumed messages leave only their minimal identity behind: a repeated delivery of the
        // same stable id is answered by digest instead of being delivered a second time. The ledger
        // is a fixed window over the newest entries, so it never accumulates a full history.
        let mut consumed_ledger = self.consumed_message_identities.to_vec();
        for record in self.inbox.iter() {
            if record.sequence > consumed_messages {
                continue;
            }
            if consumed_ledger
                .iter()
                .any(|identity| identity.id == record.message.id)
            {
                continue;
            }
            consumed_ledger.push(inbox::MessageIdentity::from_record(record));
        }
        retain_arc_slice(&mut self.inbox, |record| {
            record.sequence > consumed_messages
        });
        let consumed_excess = consumed_ledger
            .len()
            .saturating_sub(CONSUMED_MESSAGE_LEDGER_CAPACITY);
        if consumed_excess > 0 {
            consumed_ledger.drain(..consumed_excess);
        }
        self.consumed_message_identities = consumed_ledger.into();

        // A result is undelivered until its inbox message reaches model context.
        let consumed = self.consumed_messages;
        let undelivered = self
            .inbox
            .iter()
            .filter(|record| record.sequence > consumed)
            .map(|record| record.message.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let resident_delivery = |delivery: &ToolDelivery| match &delivery.target {
            ToolDeliveryTarget::Inbox { message_id } => undelivered.contains(message_id.as_str()),
            ToolDeliveryTarget::CallResult => false,
        };
        // A settled result is committed history: the complete delivery stays in the matching effect
        // batch inside the bounded live [`EffectWindow`], so the snapshot keeps only the results
        // still owed to model context instead of a checkpoint-visible payload ledger.
        retain_arc_slice(&mut self.deliveries, |delivery| resident_delivery(delivery));

        let undelivered_calls = self
            .deliveries
            .iter()
            .map(|delivery| delivery.call_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        // A task is resident only while it is running or its result is still owed to model context;
        // every finished task leaves only a bounded terminal fact behind.
        let mut terminal_tasks = self.terminal_tasks.to_vec();
        for (id, record) in self.tasks.iter() {
            if record.status == task::TaskStatus::Running
                || undelivered_calls.contains(record.call_id.as_str())
                || terminal_tasks.iter().any(|previous| previous.id == *id)
            {
                continue;
            }
            terminal_tasks.push(record.clone());
        }
        self.tasks.retain(|_, record| {
            record.status == task::TaskStatus::Running
                || undelivered_calls.contains(record.call_id.as_str())
        });
        let task_excess = terminal_tasks
            .len()
            .saturating_sub(TERMINAL_TASK_LEDGER_CAPACITY);
        if task_excess > 0 {
            terminal_tasks.drain(..task_excess);
        }
        self.terminal_tasks = terminal_tasks.into();

        let live_tasks = self
            .tasks
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        // A permission is resident only while its owning task is live; a settled decision is
        // committed history inside the bounded live [`EffectWindow`], so an identical retry replays
        // it from there without a checkpoint-visible payload ledger and without a new lease.
        self.permissions
            .retain(|_, record| live_tasks.contains(record.task_id.as_str()));
    }
}

/// Rebuilds a shared slice only when at least one element is dropped, so an unchanged slice keeps
/// its identity for commit diffing.
fn retain_arc_slice<T: Clone>(slice: &mut Arc<[T]>, keep: impl Fn(&T) -> bool) {
    if slice.iter().all(&keep) {
        return;
    }
    *slice = slice
        .iter()
        .filter(|item| keep(item))
        .cloned()
        .collect::<Vec<_>>()
        .into();
}

/// Complete current facts from a stable host source. Empty content explicitly invalidates it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeFact {
    pub source_id: String,
    pub content: Vec<ContextContent>,
}

/// Explicit host-selected context transformation, independent of summarizer implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum ContextReplacementReason {
    Compaction,
    Rewind,
    Rebuild,
}

/// An immutable context replacement fact; prior content remains available for history replay.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextReplacement {
    pub reason: ContextReplacementReason,
    pub previous: ContextSnapshot,
    pub current: ContextSnapshot,
    pub previous_private_context: Option<OpaquePayload>,
}

/// Candidate context supplied by a trusted host, with compare-and-swap admission.
#[derive(Debug)]
pub struct ReplaceContext {
    pub expected_revision: u64,
    pub reason: ContextReplacementReason,
    pub records: Vec<ContextRecord>,
}

/// Owner lifecycle, independent of product modes and workflows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum ThreadLifecycle {
    #[default]
    Open,
    Closing,
    Closed,
}

/// An admitted model request and its terminal outcome, retained even after failure.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestAttempt {
    #[serde(default)]
    pub request_metadata: Option<OpaquePayload>,
    #[serde(default)]
    pub tool_projection: Option<OpaquePayload>,
    pub turn_id: String,
    pub attempt_id: String,
    pub retry_of: Option<String>,
    pub input: ContextSnapshot,
    pub tools: Arc<[ModelToolDeclaration]>,
    pub outcome: AttemptOutcome,
    pub input_estimate: Option<crate::model::TokenEstimate>,
}

/// A provider result only becomes canonical through a Thread commit.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum AttemptOutcome {
    Running,
    Interrupted,
    Committed(ModelStepOutput),
    Cancelled {
        result: Result<ModelStepOutput, Arc<ModelError>>,
    },
    Failed(Arc<ModelError>),
    Rejected {
        output: ModelStepOutput,
        reason: ModelOutputViolation,
    },
}

impl RequestAttempt {
    /// Returns usage observed for this attempt, including cancelled and rejected responses.
    pub fn usage(&self) -> Option<&crate::model::ModelUsage> {
        match &self.outcome {
            AttemptOutcome::Running | AttemptOutcome::Interrupted => None,
            AttemptOutcome::Committed(output) | AttemptOutcome::Rejected { output, .. } => {
                Some(&output.usage)
            }
            AttemptOutcome::Failed(error) => Some(&error.usage),
            AttemptOutcome::Cancelled { result } => Some(match result {
                Ok(output) => &output.usage,
                Err(error) => &error.usage,
            }),
        }
    }
}

/// Exact reason a provider response was rejected before tool execution.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, thiserror::Error)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum ModelOutputViolation {
    #[error("model attempt identity mismatch: expected {expected}, got {actual}")]
    AttemptIdentity { expected: String, actual: String },
    #[error("model context revision mismatch: expected {expected}, got {actual}")]
    ContextRevision { expected: u64, actual: u64 },
    #[error("model called unknown tool {tool_id} (call {call_id})")]
    UnknownTool { tool_id: String, call_id: String },
    #[error("model returned an empty tool call identity")]
    EmptyCallIdentity,
    #[error("model reused tool call identity {call_id}")]
    DuplicateCallIdentity { call_id: String },
    #[error(
        "tools {tool_ids:?} require an exclusive response; no tools in this batch were executed"
    )]
    SoloBatch { tool_ids: Vec<String> },
}

/// Whether a call completed inside the delivery window or remains owned by the Thread.
#[derive(Debug, Clone)]
pub enum ToolDispatch {
    Completed(crate::tool::ToolOutput),
    Running(task::TaskRecord),
}

/// Actual destination of a tool's frozen model projection.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum ToolDeliveryTarget {
    #[default]
    CallResult,
    Inbox {
        message_id: String,
    },
}

/// The complete tool result committed with its original call identity.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDelivery {
    #[serde(default)]
    pub target: ToolDeliveryTarget,
    pub call_id: String,
    pub tool_id: String,
    pub output: crate::tool::ToolOutput,
    pub delivered_context: Vec<ContextContent>,
    pub outcome: ToolOutcome,
}

/// Execution facts are typed and cannot be forged through result payload content.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum ToolOutcome {
    Succeeded,
    Cancelled,
    Interrupted,
    Failed(Arc<crate::tool::opaque::ToolError>),
}

/// One step's supplied input. Retry uses a new attempt identity and may omit new content.
#[derive(Debug)]
pub struct StepInput {
    pub turn_id: String,
    pub attempt_id: String,
    pub content: Vec<ContextContent>,
    pub cancellation: CancellationToken,
}

/// Host-selected model step policy; usage remains observable without a stopping budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelStepLimit {
    Unlimited,
    Limited(std::num::NonZeroU32),
}
impl ModelStepLimit {
    pub(super) fn reached(self, completed: u32) -> bool {
        match self {
            Self::Unlimited => false,
            Self::Limited(limit) => completed >= limit.get(),
        }
    }
}

/// A Turn executed by the same owner that holds model and tool instances.
#[derive(Debug)]
pub struct TurnInput {
    pub turn_id: String,
    pub attempt_prefix: String,
    pub content: Vec<ContextContent>,
    pub max_model_steps: crate::thread::ModelStepLimit,
    pub cancellation: CancellationToken,
}

/// Terminal reason for a successfully observed Turn execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnOutcome {
    Completed,
    WaitingInteraction,
    StepLimit,
}

/// Durable Turn execution state, independent of provider response status.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum TurnState {
    Running,
    Finished(TurnOutcome),
    Cancelled,
    Interrupted,
    Failed { description: String },
}

/// One accepted bounded Turn and its final disposition.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnRecord {
    /// Measured execution time; interrupted recovery may have no final duration.
    #[serde(default)]
    pub elapsed_ms: Option<u64>,
    /// Input whose execution opened this Turn, including attempts that fail before model admission.
    #[serde(default)]
    pub input_id: Option<String>,
    pub turn_id: String,
    pub state: TurnState,
    pub model_steps: u32,
}

/// Observed final model step plus the reason no further inference was started.
#[derive(Debug, Clone)]
pub struct TurnCompletion {
    pub model_steps: u32,
    pub outcome: TurnOutcome,
    pub last_output: ModelStepOutput,
}

/// Admission policy selected by the host; it never interprets provider-specific content.
#[derive(Debug, Clone, Copy, Default)]
pub enum ContextCapacity {
    #[default]
    Unbounded,
    RequireExact {
        max_input_tokens: u64,
    },
    AcceptApproximate {
        max_input_tokens: u64,
    },
    AllowUnknown {
        max_input_tokens: u64,
    },
}

impl ContextCapacity {
    pub(super) fn admit(
        self,
        estimate: Option<crate::model::TokenEstimate>,
    ) -> Result<(), ThreadError> {
        use crate::model::EstimateAccuracy;
        let (limit, accepts_approximate, accepts_unknown) = match self {
            Self::Unbounded => return Ok(()),
            Self::RequireExact { max_input_tokens } => (max_input_tokens, false, false),
            Self::AcceptApproximate { max_input_tokens } => (max_input_tokens, true, false),
            Self::AllowUnknown { max_input_tokens } => (max_input_tokens, true, true),
        };
        match estimate {
            None if !accepts_unknown => Err(ThreadError::UnknownCapacity),
            Some(estimate)
                if estimate.accuracy == EstimateAccuracy::Approximate && !accepts_approximate =>
            {
                Err(ThreadError::UnknownCapacity)
            }
            Some(estimate) if estimate.tokens > limit => {
                Err(ThreadError::ContextCapacity { estimate, limit })
            }
            Some(_) | None => Ok(()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ThreadError {
    #[error("tool registration was rejected and cleanup remains pending: {0}")]
    RejectedTools(#[source] Box<crate::tool::opaque::RejectedTools>),
    #[error("input requires an idle Thread with no pending input")]
    InputRequiresIdle,
    #[error("steering input requires a running Turn")]
    InputRequiresActiveTurn,
    #[error("input is being prepared for model admission")]
    InputInUse,
    #[error("input was already consumed")]
    InputConsumed,
    #[error("Thread model session is unavailable; replace the model binding to continue")]
    ModelUnavailable,
    #[error("frozen tool permissions were revoked before admission or execution")]
    ToolPermissionRevoked,
    #[error("completed tool results are waiting for a successful Thread commit")]
    PendingToolCommit,
    #[error("task control is not granted to this tool")]
    TaskAccessDenied,
    #[error("task control caller is no longer running")]
    TaskAccessExpired,
    #[error("a task cannot wait for itself")]
    TaskSelfWait,
    #[error("Thread owner is closed")]
    Closed,
    #[error("request cancelled")]
    Cancelled,
    #[error("request identity is empty or was already admitted")]
    InvalidIdentity,
    #[error("extension {id} revision conflict: expected {expected:?}, actual {actual:?}")]
    ExtensionConflict {
        id: String,
        expected: Option<u64>,
        actual: Option<u64>,
    },
    #[error("extension sequence conflict: expected {expected}, actual {actual}")]
    ExtensionSequenceConflict { expected: u64, actual: u64 },
    #[error("model output does not match the admitted request")]
    InvalidOutput,
    #[error(transparent)]
    ModelOutput(#[from] ModelOutputViolation),
    #[error(
        "taskId {task_id:?} was not found in this Thread; use the exact taskId from the tool receipt, or an empty taskIds list to wait for messages"
    )]
    TaskNotFound { task_id: String },
    #[error("context revision exhausted")]
    RevisionExhausted,
    #[error("context revision conflict: expected {expected}, actual {actual}")]
    ContextConflict { expected: u64, actual: u64 },
    #[error("context contains an empty or duplicate record identity")]
    InvalidContext,
    #[error("Thread persistence is blocked: {0}")]
    Storage(#[source] Arc<cold::ColdStoreError>),
    #[error("new model execution is paused by cold-storage pressure")]
    StoragePressure,
    #[error("model did not provide an estimate with the required accuracy")]
    UnknownCapacity,
    #[error("model input estimate {estimate:?} exceeds capacity {limit}")]
    ContextCapacity {
        estimate: crate::model::TokenEstimate,
        limit: u64,
    },
    #[error("invalid context relationships: {0}")]
    Context(#[from] crate::context::ContextError),
    #[error("tool call is missing or already delivered")]
    MissingCall,
    #[error("pending tool calls must be delivered before another model step")]
    PendingTools,
    #[error("a host interaction must be resolved before model execution can continue")]
    PendingInteraction,
    #[error("tool execution failed: {0}")]
    Tool(#[source] Arc<crate::tool::opaque::ToolError>),
    #[error("invalid tool registry: {0}")]
    Registry(#[from] crate::tool::opaque::RegistryError),
    #[error("model operation failed: {0}")]
    Model(#[source] Arc<ModelError>),
}
