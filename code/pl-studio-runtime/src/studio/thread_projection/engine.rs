//! Incremental product timeline projection: one canonical commit at a time.
//!
//! `ProjectionState` holds a small head (watermark and next ordinal), one stable slot per reserved
//! identity, and only the facts each slot needs to rebuild its item. [`ProjectionState::apply`]
//! advances exactly one commit and returns only the slots and fact rows it changed, so a hot
//! consumer never replays a journal prefix. [`ProjectionState::rebuild`] replays the same per-slot
//! builders over a complete journal, so a cold rebuild and the hot path share one projection
//! algorithm.
//!
//! The durable boundary is keyed and typed:
//! - [`ProjectionFactKey`] / [`ProjectionFactRow`] describe one durable fact row, never a whole map.
//! - [`ProjectionState::requirements`] is the read plan (exact keys plus named lookups) a store must
//!   load for one commit, so it never reads all history.
//! - [`ProjectionDelta::fact_writes`] lists the dirty rows to persist and
//!   [`ProjectionDelta::new_slots`] the newly admitted slot ordinals.
//! - [`ProjectionState::persisted_slots`] excludes ephemeral previews: a running entity's preview
//!   text is never durable, so a stale preview cannot be written back.

use super::{
    ProjectionError, compactions, completions,
    content::text_content,
    inputs, messages, order,
    panel::PanelState,
    responses, tools,
    turns::{self, Stamp},
};
use pl_core::{
    context::{ContextContent, ContextSnapshot, OpaquePayload},
    model::{ActiveModelProgress, ModelError, ModelToolCall},
    thread::{
        AttemptOutcome, ContextReplacementReason, RequestAttempt, ThreadSnapshot, ToolDelivery,
        TurnRecord, TurnState as CoreTurnState,
        extensions::{ExtensionChange, ExtensionRecord},
        inbox::ThreadMessage,
        input::{InputChange, InputRecord, InputState},
        journal::ThreadCommit,
        permissions::{PermissionRecord, PermissionState},
        task::{TaskRecord, TaskStatus},
    },
};
use pl_protocol::{
    InferenceAccounting, ThreadContextDisposition, ThreadItem, ThreadItemState, ThreadTurnItem,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Category of one reserved timeline slot, exposed to keyed-storage adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum SlotKind {
    Input,
    Message,
    Turn,
    Compaction,
    Skill,
    Completion,
    Inference,
    Reasoning,
    ResponseText,
    Tool,
}

/// Stable identity of one durable projection fact row. Secondary entities that a slot builder looks
/// up are expressed as explicit [`ProjectionQuery`] values, never as a whole-map enum variant.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) enum ProjectionFactKey {
    Input(String),
    Message(String),
    Attempt(String),
    Call(String),
    CallUpdate(String),
    CallTurn(String),
    Task(String),
    TaskByCall(String),
    Delivery(String),
    Permission(String),
    TurnStamp(String),
    TurnFirst(String),
    Turn(String),
    Compaction(String),
    /// A Turn rolled back by a saved rewind replacement.
    RolledBack(String),
    /// One saved skill-view extension payload.
    SkillView(String),
}

/// One durable fact row. Large payloads are boxed so the row stays a bounded, keyed value.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum ProjectionFactRow {
    Input(Box<InputFacts>),
    Message(Box<MessageFacts>),
    Attempt(Box<AttemptFacts>),
    Call(Box<SavedCall>),
    CallUpdate(u64, i64),
    CallTurn(String),
    Task(Box<TaskRecord>),
    TaskByCall(String),
    Delivery(Box<DeliveryFacts>),
    Permission(Box<PermissionRecord>),
    TurnStamp(Stamp),
    TurnFirst(u64),
    Turn(Box<TurnRecord>),
    Compaction(Box<CompactionFacts>),
    RolledBack,
    SkillView(Box<OpaquePayload>),
}

/// A named secondary lookup over fact rows. A store answers these from its own index rows; the
/// projection never scans a whole map.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ProjectionQuery {
    /// Parent messages still awaiting consumption at or below `through`.
    PendingMessages { through: u64 },
    /// Task rows of one Turn, for its phase and budget.
    TurnTasks { turn_id: String },
    /// Attempt rows of one Turn, for its latest phase and failure.
    TurnAttempts { turn_id: String },
    /// Permission rows still pending for one Call, for tool approval.
    PendingPermissions { call_id: String },
    /// Turns opened by one input, for pending-input Turn linkage.
    TurnsByInput { input_id: String },
    /// The task row of one Call, if any.
    TaskByCall { call_id: String },
}

/// The read plan for one commit: the exact rows plus named lookups a store must load.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ProjectionRequirements {
    pub keys: Vec<ProjectionFactKey>,
    pub queries: Vec<ProjectionQuery>,
}

/// The read/write plan for one commit: the facts to load plus the slot keys it touches. Slot keys
/// are produced here because the order id helpers are private and a store must not rebuild them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProjectionPlan {
    pub requirements: ProjectionRequirements,
    /// Slot keys this commit may create or update; load their positions by key.
    pub slot_keys: Vec<String>,
}

/// Small persisted head: the durable cursor of one projection, without any slot body.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ProjectionHead {
    pub thread_id: String,
    pub watermark: u64,
    pub next_ordinal: u64,
}

/// One persisted slot: its reserved position plus its durable item, if any. Running previews are
/// deliberately absent.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct PersistedSlot {
    pub key: String,
    pub ordinal: u64,
    pub created_at: i64,
    pub kind: Option<SlotKind>,
    pub item: Option<ThreadItem>,
}

/// Everything one commit changed: the affected item slots, the newly admitted slot ordinals, and
/// the dirty durable fact rows a store writes back.
#[derive(Debug, Default)]
pub(crate) struct ProjectionDelta {
    pub watermark: u64,
    /// Item slots whose item changed.
    pub changed: Vec<String>,
    /// Item slots whose item disappeared.
    pub removed: Vec<String>,
    /// Slots whose ordinal was first reserved by this commit.
    pub new_slots: Vec<String>,
    /// Dirty durable fact rows: `Some` is an upsert, `None` a delete.
    pub fact_writes: Vec<(ProjectionFactKey, Option<ProjectionFactRow>)>,
    /// True when this commit moved the durable runtime usage/panel summary.
    pub panel_changed: bool,
    /// Turn dispositions changed by this commit's saved rewind replacements.
    pub context_disposition: Vec<(String, ThreadContextDisposition)>,
}

/// One projected slot's candidate item, before positions are folded in and published.
struct Candidate {
    key: String,
    kind: SlotKind,
    item: Option<ThreadItem>,
    /// True when the item embeds an ephemeral preview and must not be persisted as-is.
    ephemeral: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct InputFacts {
    pub record: InputRecord,
    pub accepted: (u64, i64),
    pub updated: (u64, i64),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct MessageFacts {
    pub sequence: u64,
    pub message: ThreadMessage,
    pub turn_id: String,
    pub revision: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct AttemptFacts {
    pub record: RequestAttempt,
    pub seq: u64,
    pub created_at: i64,
    pub revision: u64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SavedCall {
    pub turn_id: String,
    pub call: ModelToolCall,
    pub sequence: u64,
    pub at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct DeliveryFacts {
    pub delivery: ToolDelivery,
    pub sequence: u64,
    pub at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct CompactionFacts {
    pub record: ExtensionRecord,
    pub commit_at: i64,
    pub compaction: bool,
    /// Decoded auxiliary accounting, so the panel can correct a rewrite/delete from this row.
    pub accounting: InferenceAccounting,
}

/// Bounded rollback record for one fact mutation. Only the keys a single commit touched are stored,
/// so the largest variant stays one slot's facts; large payloads are boxed to keep the enum small.
enum Undo {
    Input(String, Option<Box<InputFacts>>),
    Message(String, Option<Box<MessageFacts>>),
    Attempt(String, Option<Box<AttemptFacts>>),
    Call(String, Option<Box<SavedCall>>),
    CallUpdate(String, Option<(u64, i64)>),
    CallTurn(String, Option<String>),
    Task(String, Option<Box<TaskRecord>>),
    TaskByCall(String, Option<String>),
    Delivery(String, Option<Box<DeliveryFacts>>),
    Permission(String, Option<Box<PermissionRecord>>),
    TurnStamp(String, Option<Stamp>),
    Turn(String, Option<Box<TurnRecord>>),
    TurnFirst(String, Option<u64>),
    Compaction(String, Option<Box<CompactionFacts>>),
    RolledBack(String),
    SkillView(String, Option<Box<OpaquePayload>>),
}

impl Undo {
    fn key(&self) -> ProjectionFactKey {
        match self {
            Undo::Input(key, _) => ProjectionFactKey::Input(key.clone()),
            Undo::Message(key, _) => ProjectionFactKey::Message(key.clone()),
            Undo::Attempt(key, _) => ProjectionFactKey::Attempt(key.clone()),
            Undo::Call(key, _) => ProjectionFactKey::Call(key.clone()),
            Undo::CallUpdate(key, _) => ProjectionFactKey::CallUpdate(key.clone()),
            Undo::CallTurn(key, _) => ProjectionFactKey::CallTurn(key.clone()),
            Undo::Task(key, _) => ProjectionFactKey::Task(key.clone()),
            Undo::TaskByCall(key, _) => ProjectionFactKey::TaskByCall(key.clone()),
            Undo::Delivery(key, _) => ProjectionFactKey::Delivery(key.clone()),
            Undo::Permission(key, _) => ProjectionFactKey::Permission(key.clone()),
            Undo::TurnStamp(key, _) => ProjectionFactKey::TurnStamp(key.clone()),
            Undo::Turn(key, _) => ProjectionFactKey::Turn(key.clone()),
            Undo::TurnFirst(key, _) => ProjectionFactKey::TurnFirst(key.clone()),
            Undo::Compaction(key, _) => ProjectionFactKey::Compaction(key.clone()),
            Undo::RolledBack(key) => ProjectionFactKey::RolledBack(key.clone()),
            Undo::SkillView(key, _) => ProjectionFactKey::SkillView(key.clone()),
        }
    }
}

/// The durable facts a projection reads, keyed so a store can page them per key. Only keys a commit
/// touches are accessed again. Live previews are skipped and never persisted.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct Facts {
    /// Durable inbox facts are kept only for the parent agent; other sources reserve a slot only.
    pub message_source: Option<String>,
    pub inputs: BTreeMap<String, InputFacts>,
    pub messages: BTreeMap<String, MessageFacts>,
    pub attempts: BTreeMap<String, AttemptFacts>,
    pub calls: BTreeMap<String, SavedCall>,
    pub call_updates: BTreeMap<String, (u64, i64)>,
    pub call_turn: BTreeMap<String, String>,
    pub tasks: BTreeMap<String, TaskRecord>,
    pub task_by_call: BTreeMap<String, String>,
    pub deliveries: BTreeMap<String, DeliveryFacts>,
    pub permissions: BTreeMap<String, PermissionRecord>,
    pub turn_stamps: BTreeMap<String, Stamp>,
    pub turn_first: BTreeMap<String, u64>,
    pub turns: BTreeMap<String, TurnRecord>,
    pub compactions: BTreeMap<String, CompactionFacts>,
    /// Saved skill-view extension payloads, keyed by extension id.
    #[serde(default)]
    pub skill_views: BTreeMap<String, OpaquePayload>,
    /// Turn ids rolled back by saved rewind replacements.
    #[serde(default)]
    pub rolled_back: BTreeSet<String>,
    // Ephemeral previews: never durable, never serialized, only read for running entities.
    #[serde(skip)]
    pub model_progress: Option<ActiveModelProgress>,
    #[serde(skip)]
    pub tool_progress: BTreeMap<String, Vec<ContextContent>>,
}

impl Facts {
    /// Reads one durable fact row by key, for a keyed store load.
    pub(crate) fn get(&self, key: &ProjectionFactKey) -> Option<ProjectionFactRow> {
        Some(match key {
            ProjectionFactKey::Input(key) => {
                ProjectionFactRow::Input(Box::new(self.inputs.get(key)?.clone()))
            }
            ProjectionFactKey::Message(key) => {
                ProjectionFactRow::Message(Box::new(self.messages.get(key)?.clone()))
            }
            ProjectionFactKey::Attempt(key) => {
                ProjectionFactRow::Attempt(Box::new(self.attempts.get(key)?.clone()))
            }
            ProjectionFactKey::Call(key) => {
                ProjectionFactRow::Call(Box::new(self.calls.get(key)?.clone()))
            }
            ProjectionFactKey::CallUpdate(key) => {
                let (sequence, at) = *self.call_updates.get(key)?;
                ProjectionFactRow::CallUpdate(sequence, at)
            }
            ProjectionFactKey::CallTurn(key) => {
                ProjectionFactRow::CallTurn(self.call_turn.get(key)?.clone())
            }
            ProjectionFactKey::Task(key) => {
                ProjectionFactRow::Task(Box::new(self.tasks.get(key)?.clone()))
            }
            ProjectionFactKey::TaskByCall(key) => {
                ProjectionFactRow::TaskByCall(self.task_by_call.get(key)?.clone())
            }
            ProjectionFactKey::Delivery(key) => {
                ProjectionFactRow::Delivery(Box::new(self.deliveries.get(key)?.clone()))
            }
            ProjectionFactKey::Permission(key) => {
                ProjectionFactRow::Permission(Box::new(self.permissions.get(key)?.clone()))
            }
            ProjectionFactKey::TurnStamp(key) => {
                ProjectionFactRow::TurnStamp(*self.turn_stamps.get(key)?)
            }
            ProjectionFactKey::TurnFirst(key) => {
                ProjectionFactRow::TurnFirst(*self.turn_first.get(key)?)
            }
            ProjectionFactKey::Turn(key) => {
                ProjectionFactRow::Turn(Box::new(self.turns.get(key)?.clone()))
            }
            ProjectionFactKey::Compaction(key) => {
                ProjectionFactRow::Compaction(Box::new(self.compactions.get(key)?.clone()))
            }
            ProjectionFactKey::SkillView(key) => {
                ProjectionFactRow::SkillView(Box::new(self.skill_views.get(key)?.clone()))
            }
            ProjectionFactKey::RolledBack(key) => {
                self.rolled_back.get(key)?;
                ProjectionFactRow::RolledBack
            }
        })
    }

    /// Stores one durable fact row loaded from a keyed store. Mismatched pairs are ignored.
    pub(crate) fn put(&mut self, key: ProjectionFactKey, row: ProjectionFactRow) {
        match (key, row) {
            (ProjectionFactKey::Input(key), ProjectionFactRow::Input(value)) => {
                self.inputs.insert(key, *value);
            }
            (ProjectionFactKey::Message(key), ProjectionFactRow::Message(value)) => {
                self.messages.insert(key, *value);
            }
            (ProjectionFactKey::Attempt(key), ProjectionFactRow::Attempt(value)) => {
                self.attempts.insert(key, *value);
            }
            (ProjectionFactKey::Call(key), ProjectionFactRow::Call(value)) => {
                self.calls.insert(key, *value);
            }
            (ProjectionFactKey::CallUpdate(key), ProjectionFactRow::CallUpdate(sequence, at)) => {
                self.call_updates.insert(key, (sequence, at));
            }
            (ProjectionFactKey::CallTurn(key), ProjectionFactRow::CallTurn(value)) => {
                self.call_turn.insert(key, value);
            }
            (ProjectionFactKey::Task(key), ProjectionFactRow::Task(value)) => {
                self.tasks.insert(key, *value);
            }
            (ProjectionFactKey::TaskByCall(key), ProjectionFactRow::TaskByCall(value)) => {
                self.task_by_call.insert(key, value);
            }
            (ProjectionFactKey::Delivery(key), ProjectionFactRow::Delivery(value)) => {
                self.deliveries.insert(key, *value);
            }
            (ProjectionFactKey::Permission(key), ProjectionFactRow::Permission(value)) => {
                self.permissions.insert(key, *value);
            }
            (ProjectionFactKey::TurnStamp(key), ProjectionFactRow::TurnStamp(value)) => {
                self.turn_stamps.insert(key, value);
            }
            (ProjectionFactKey::TurnFirst(key), ProjectionFactRow::TurnFirst(value)) => {
                self.turn_first.insert(key, value);
            }
            (ProjectionFactKey::Turn(key), ProjectionFactRow::Turn(value)) => {
                self.turns.insert(key, *value);
            }
            (ProjectionFactKey::Compaction(key), ProjectionFactRow::Compaction(value)) => {
                self.compactions.insert(key, *value);
            }
            (ProjectionFactKey::SkillView(key), ProjectionFactRow::SkillView(value)) => {
                self.skill_views.insert(key, *value);
            }
            (ProjectionFactKey::RolledBack(key), ProjectionFactRow::RolledBack) => {
                self.rolled_back.insert(key);
            }
            _ => {}
        }
    }

    /// Removes one durable fact row by key.
    pub(crate) fn remove(&mut self, key: &ProjectionFactKey) {
        match key {
            ProjectionFactKey::Input(key) => {
                self.inputs.remove(key);
            }
            ProjectionFactKey::Message(key) => {
                self.messages.remove(key);
            }
            ProjectionFactKey::Attempt(key) => {
                self.attempts.remove(key);
            }
            ProjectionFactKey::Call(key) => {
                self.calls.remove(key);
            }
            ProjectionFactKey::CallUpdate(key) => {
                self.call_updates.remove(key);
            }
            ProjectionFactKey::CallTurn(key) => {
                self.call_turn.remove(key);
            }
            ProjectionFactKey::Task(key) => {
                self.tasks.remove(key);
            }
            ProjectionFactKey::TaskByCall(key) => {
                self.task_by_call.remove(key);
            }
            ProjectionFactKey::Delivery(key) => {
                self.deliveries.remove(key);
            }
            ProjectionFactKey::Permission(key) => {
                self.permissions.remove(key);
            }
            ProjectionFactKey::TurnStamp(key) => {
                self.turn_stamps.remove(key);
            }
            ProjectionFactKey::TurnFirst(key) => {
                self.turn_first.remove(key);
            }
            ProjectionFactKey::Turn(key) => {
                self.turns.remove(key);
            }
            ProjectionFactKey::Compaction(key) => {
                self.compactions.remove(key);
            }
            ProjectionFactKey::SkillView(key) => {
                self.skill_views.remove(key);
            }
            ProjectionFactKey::RolledBack(key) => {
                self.rolled_back.remove(key);
            }
        }
    }

    /// Iterates every durable row for keyed persistence.
    pub(crate) fn rows(&self) -> impl Iterator<Item = (ProjectionFactKey, ProjectionFactRow)> + '_ {
        let inputs = self.inputs.iter().map(|(key, value)| {
            (
                ProjectionFactKey::Input(key.clone()),
                ProjectionFactRow::Input(Box::new(value.clone())),
            )
        });
        let messages = self.messages.iter().map(|(key, value)| {
            (
                ProjectionFactKey::Message(key.clone()),
                ProjectionFactRow::Message(Box::new(value.clone())),
            )
        });
        let attempts = self.attempts.iter().map(|(key, value)| {
            (
                ProjectionFactKey::Attempt(key.clone()),
                ProjectionFactRow::Attempt(Box::new(value.clone())),
            )
        });
        let calls = self.calls.iter().map(|(key, value)| {
            (
                ProjectionFactKey::Call(key.clone()),
                ProjectionFactRow::Call(Box::new(value.clone())),
            )
        });
        let updates = self.call_updates.iter().map(|(key, (sequence, at))| {
            (
                ProjectionFactKey::CallUpdate(key.clone()),
                ProjectionFactRow::CallUpdate(*sequence, *at),
            )
        });
        let call_turns = self.call_turn.iter().map(|(key, value)| {
            (
                ProjectionFactKey::CallTurn(key.clone()),
                ProjectionFactRow::CallTurn(value.clone()),
            )
        });
        let tasks = self.tasks.iter().map(|(key, value)| {
            (
                ProjectionFactKey::Task(key.clone()),
                ProjectionFactRow::Task(Box::new(value.clone())),
            )
        });
        let task_by_call = self.task_by_call.iter().map(|(key, value)| {
            (
                ProjectionFactKey::TaskByCall(key.clone()),
                ProjectionFactRow::TaskByCall(value.clone()),
            )
        });
        let deliveries = self.deliveries.iter().map(|(key, value)| {
            (
                ProjectionFactKey::Delivery(key.clone()),
                ProjectionFactRow::Delivery(Box::new(value.clone())),
            )
        });
        let permissions = self.permissions.iter().map(|(key, value)| {
            (
                ProjectionFactKey::Permission(key.clone()),
                ProjectionFactRow::Permission(Box::new(value.clone())),
            )
        });
        let stamps = self.turn_stamps.iter().map(|(key, value)| {
            (
                ProjectionFactKey::TurnStamp(key.clone()),
                ProjectionFactRow::TurnStamp(*value),
            )
        });
        let firsts = self.turn_first.iter().map(|(key, value)| {
            (
                ProjectionFactKey::TurnFirst(key.clone()),
                ProjectionFactRow::TurnFirst(*value),
            )
        });
        let turns = self.turns.iter().map(|(key, value)| {
            (
                ProjectionFactKey::Turn(key.clone()),
                ProjectionFactRow::Turn(Box::new(value.clone())),
            )
        });
        let compactions = self.compactions.iter().map(|(key, value)| {
            (
                ProjectionFactKey::Compaction(key.clone()),
                ProjectionFactRow::Compaction(Box::new(value.clone())),
            )
        });
        let skill_views = self.skill_views.iter().map(|(key, value)| {
            (
                ProjectionFactKey::SkillView(key.clone()),
                ProjectionFactRow::SkillView(Box::new(value.clone())),
            )
        });
        let rolled_back = self.rolled_back.iter().map(|key| {
            (
                ProjectionFactKey::RolledBack(key.clone()),
                ProjectionFactRow::RolledBack,
            )
        });
        inputs
            .chain(messages)
            .chain(attempts)
            .chain(calls)
            .chain(updates)
            .chain(call_turns)
            .chain(tasks)
            .chain(task_by_call)
            .chain(deliveries)
            .chain(permissions)
            .chain(stamps)
            .chain(firsts)
            .chain(turns)
            .chain(compactions)
            .chain(skill_views)
            .chain(rolled_back)
    }

    /// Parent messages still awaiting consumption at or below `through`, newest key first order.
    pub(crate) fn pending_messages(&self, through: u64) -> Vec<String> {
        let mut pending: Vec<(u64, &str)> = self
            .messages
            .iter()
            .filter(|(_, facts)| facts.sequence <= through && facts.turn_id.is_empty())
            .map(|(id, facts)| (facts.sequence, id.as_str()))
            .collect();
        pending.sort();
        pending.into_iter().map(|(_, id)| id.to_string()).collect()
    }

    /// Task ids of one Turn.
    pub(crate) fn turn_tasks(&self, turn_id: &str) -> Vec<String> {
        let mut tasks: Vec<&str> = self
            .tasks
            .iter()
            .filter(|(_, task)| task.turn_id == turn_id)
            .map(|(id, _)| id.as_str())
            .collect();
        tasks.sort();
        tasks.into_iter().map(str::to_owned).collect()
    }

    /// Attempt ids of one Turn, ordered by admission sequence.
    pub(crate) fn turn_attempts(&self, turn_id: &str) -> Vec<String> {
        let mut attempts: Vec<(u64, &str)> = self
            .attempts
            .iter()
            .filter(|(_, facts)| facts.record.turn_id == turn_id)
            .map(|(id, facts)| (facts.seq, id.as_str()))
            .collect();
        attempts.sort();
        attempts.into_iter().map(|(_, id)| id.to_string()).collect()
    }

    /// Whether one Call still has a pending permission.
    pub(crate) fn has_pending_permission(&self, call_id: &str) -> bool {
        self.permissions.values().any(|permission| {
            permission.call_id == call_id && permission.state == PermissionState::Pending
        })
    }

    /// The newest Turn opened by one input, if any.
    pub(crate) fn latest_turn_for_input(&self, input_id: &str) -> Option<String> {
        let mut best: Option<(u64, &str)> = None;
        for (turn_id, turn) in self.turns.iter() {
            if turn.input_id.as_deref() == Some(input_id) {
                let seq = self.turn_first.get(turn_id).copied().unwrap_or(0);
                if best.is_none_or(|(current, _)| seq >= current) {
                    best = Some((seq, turn_id.as_str()));
                }
            }
        }
        best.map(|(_, turn_id)| turn_id.to_string())
    }

    fn apply(&mut self, commit: &ThreadCommit) -> Result<Vec<Undo>, ProjectionError> {
        let mut undo = Vec::new();
        match self.stage(commit, &mut undo) {
            Ok(()) => Ok(undo),
            Err(error) => {
                self.revert(undo);
                Err(error)
            }
        }
    }

    fn stage(
        &mut self,
        commit: &ThreadCommit,
        undo: &mut Vec<Undo>,
    ) -> Result<(), ProjectionError> {
        let at = commit.committed_at;
        let seq = commit.sequence;
        for change in commit.inputs.iter() {
            match change {
                InputChange::Accepted(record) => {
                    let id = record.input.id.clone();
                    let facts = InputFacts {
                        record: record.clone(),
                        accepted: (seq, at),
                        updated: (seq, at),
                    };
                    undo.push(Undo::Input(
                        id.clone(),
                        self.inputs.insert(id, facts).map(Box::new),
                    ));
                }
                InputChange::Transition {
                    id,
                    revision,
                    state,
                } => {
                    let mut facts = self
                        .inputs
                        .get(id)
                        .cloned()
                        .ok_or_else(|| ProjectionError::MissingInput(id.clone()))?;
                    facts.record.state = state.clone();
                    facts.record.revision = *revision;
                    facts.updated = (seq, at);
                    undo.push(Undo::Input(
                        id.clone(),
                        self.inputs.insert(id.clone(), facts).map(Box::new),
                    ));
                }
            }
        }
        if let Some(source) = self.message_source.clone() {
            for record in commit.inbox.iter() {
                if record.message.source_id != source
                    || self.messages.contains_key(&record.message.id)
                {
                    continue;
                }
                let id = record.message.id.clone();
                let facts = MessageFacts {
                    sequence: record.sequence,
                    message: record.message.clone(),
                    turn_id: String::new(),
                    revision: seq,
                    created_at: at,
                    updated_at: at,
                };
                undo.push(Undo::Message(
                    id.clone(),
                    self.messages.insert(id, facts).map(Box::new),
                ));
            }
        }
        if let Some(turn) = &commit.turn {
            let id = turn.turn_id.clone();
            if !self.turn_first.contains_key(&id) {
                undo.push(Undo::TurnFirst(
                    id.clone(),
                    self.turn_first.insert(id.clone(), seq),
                ));
            }
            let stamp = self.turn_stamps.get(&id).copied().unwrap_or(Stamp {
                started_at: at,
                updated_at: at,
                revision: seq,
            });
            undo.push(Undo::TurnStamp(
                id.clone(),
                self.turn_stamps.insert(
                    id.clone(),
                    Stamp {
                        started_at: stamp.started_at,
                        updated_at: at,
                        revision: seq,
                    },
                ),
            ));
            undo.push(Undo::Turn(
                id.clone(),
                self.turns.insert(id, turn.clone()).map(Box::new),
            ));
            if let Some(input_id) = &turn.input_id
                && let Some(facts) = self.inputs.get(input_id).cloned()
            {
                let updated = InputFacts {
                    updated: (seq, at),
                    ..facts
                };
                undo.push(Undo::Input(
                    input_id.clone(),
                    self.inputs.insert(input_id.clone(), updated).map(Box::new),
                ));
            }
        }
        if let Some(update) = &commit.attempt {
            let id = update.attempt_id.clone();
            let record = RequestAttempt {
                request_metadata: update.request_metadata.clone(),
                tool_projection: update.tool_projection.clone(),
                turn_id: update.turn_id.clone(),
                attempt_id: id.clone(),
                retry_of: update.retry_of.clone(),
                input: ContextSnapshot::default(),
                tools: Vec::new().into(),
                outcome: update.outcome.clone(),
                input_estimate: update.input_estimate,
            };
            let facts = match self.attempts.get(&id) {
                Some(old) => AttemptFacts {
                    record,
                    seq: old.seq,
                    created_at: old.created_at,
                    revision: seq,
                    updated_at: at,
                },
                None => AttemptFacts {
                    record,
                    seq,
                    created_at: at,
                    revision: seq,
                    updated_at: at,
                },
            };
            undo.push(Undo::Attempt(
                id.clone(),
                self.attempts.insert(id.clone(), facts).map(Box::new),
            ));
            if let Some(stamp) = self.turn_stamps.get(&update.turn_id).copied() {
                undo.push(Undo::TurnStamp(
                    update.turn_id.clone(),
                    self.turn_stamps.insert(
                        update.turn_id.clone(),
                        Stamp {
                            started_at: stamp.started_at,
                            updated_at: at,
                            revision: seq,
                        },
                    ),
                ));
            }
            if let AttemptOutcome::Committed(output) = &update.outcome {
                for call in &output.tool_calls {
                    if self.calls.contains_key(&call.call_id) {
                        return Err(ProjectionError::DuplicateCall(call.call_id.clone()));
                    }
                    undo.push(Undo::Call(
                        call.call_id.clone(),
                        self.calls
                            .insert(
                                call.call_id.clone(),
                                SavedCall {
                                    turn_id: update.turn_id.clone(),
                                    call: call.clone(),
                                    sequence: seq,
                                    at,
                                },
                            )
                            .map(Box::new),
                    ));
                    undo.push(Undo::CallTurn(
                        call.call_id.clone(),
                        self.call_turn
                            .insert(call.call_id.clone(), update.turn_id.clone()),
                    ));
                }
            }
        }
        for task in commit.tasks.iter() {
            undo.push(Undo::Task(
                task.id.clone(),
                self.tasks
                    .insert(task.id.clone(), task.clone())
                    .map(Box::new),
            ));
            undo.push(Undo::TaskByCall(
                task.call_id.clone(),
                self.task_by_call
                    .insert(task.call_id.clone(), task.id.clone()),
            ));
            undo.push(Undo::CallUpdate(
                task.call_id.clone(),
                self.call_updates.insert(task.call_id.clone(), (seq, at)),
            ));
            if self
                .turns
                .get(&task.turn_id)
                .is_some_and(|turn| turn.state == CoreTurnState::Running)
                && let Some(stamp) = self.turn_stamps.get(&task.turn_id).copied()
            {
                undo.push(Undo::TurnStamp(
                    task.turn_id.clone(),
                    self.turn_stamps.insert(
                        task.turn_id.clone(),
                        Stamp {
                            started_at: stamp.started_at,
                            updated_at: at,
                            revision: seq,
                        },
                    ),
                ));
            }
        }
        for permission in commit.permissions.iter() {
            undo.push(Undo::Permission(
                permission.id.clone(),
                self.permissions
                    .insert(permission.id.clone(), permission.clone())
                    .map(Box::new),
            ));
            undo.push(Undo::CallUpdate(
                permission.call_id.clone(),
                self.call_updates
                    .insert(permission.call_id.clone(), (seq, at)),
            ));
        }
        for delivery in commit.deliveries.iter() {
            undo.push(Undo::Delivery(
                delivery.call_id.clone(),
                self.deliveries
                    .insert(
                        delivery.call_id.clone(),
                        DeliveryFacts {
                            delivery: delivery.clone(),
                            sequence: seq,
                            at,
                        },
                    )
                    .map(Box::new),
            ));
            undo.push(Undo::CallUpdate(
                delivery.call_id.clone(),
                self.call_updates
                    .insert(delivery.call_id.clone(), (seq, at)),
            ));
        }
        let compaction = commit
            .replacements
            .iter()
            .any(|replacement| replacement.reason == ContextReplacementReason::Compaction);
        for change in commit.extensions.iter() {
            match change {
                ExtensionChange::Put { id, record } => {
                    let format = record.payload.format();
                    if format == "pl.studio.compaction" {
                        // Decode tolerantly here; the panel corrects a bad receipt on its own path.
                        let accounting = super::compactions::receipt(&record.payload)
                            .ok()
                            .flatten()
                            .map(|receipt| receipt.accounting)
                            .unwrap_or_default();
                        undo.push(Undo::Compaction(
                            id.clone(),
                            self.compactions
                                .insert(
                                    id.clone(),
                                    CompactionFacts {
                                        record: record.clone(),
                                        commit_at: at,
                                        compaction,
                                        accounting,
                                    },
                                )
                                .map(Box::new),
                        ));
                    } else if format == "pl.tool.skill-view" {
                        undo.push(Undo::SkillView(
                            id.clone(),
                            self.skill_views
                                .insert(id.clone(), record.payload.clone())
                                .map(Box::new),
                        ));
                    }
                }
                ExtensionChange::Delete { id, .. } => {
                    if self.skill_views.contains_key(id) {
                        undo.push(Undo::SkillView(
                            id.clone(),
                            self.skill_views.remove(id).map(Box::new),
                        ));
                    }
                }
            }
        }
        for replacement in commit.replacements.iter() {
            if replacement.reason != ContextReplacementReason::Rewind {
                continue;
            }
            let retained: BTreeSet<&str> = replacement
                .current
                .records
                .iter()
                .filter_map(|record| record.turn_id.as_deref())
                .collect();
            for record in replacement.previous.records.iter() {
                if let Some(turn_id) = &record.turn_id
                    && !retained.contains(turn_id.as_str())
                    && self.rolled_back.insert(turn_id.clone())
                {
                    undo.push(Undo::RolledBack(turn_id.clone()));
                }
            }
        }
        if let Some(through) = commit.consumed_messages {
            let turn = commit
                .attempt
                .as_ref()
                .map(|attempt| attempt.turn_id.clone());
            let ids = self.pending_messages(through);
            for id in ids {
                let turn_id = turn
                    .clone()
                    .ok_or_else(|| ProjectionError::MissingMessage(id.clone()))?;
                let mut facts = self
                    .messages
                    .get(&id)
                    .cloned()
                    .expect("named lookup returned an existing message");
                undo.push(Undo::Message(id.clone(), Some(Box::new(facts.clone()))));
                facts.turn_id = turn_id;
                facts.revision = seq;
                facts.updated_at = at;
                self.messages.insert(id, facts);
            }
        }
        Ok(())
    }

    fn revert(&mut self, undo: Vec<Undo>) {
        for entry in undo.into_iter().rev() {
            match entry {
                Undo::Input(key, old) => restore(&mut self.inputs, key, old.map(|value| *value)),
                Undo::Message(key, old) => {
                    restore(&mut self.messages, key, old.map(|value| *value))
                }
                Undo::Attempt(key, old) => {
                    restore(&mut self.attempts, key, old.map(|value| *value))
                }
                Undo::Call(key, old) => restore(&mut self.calls, key, old.map(|value| *value)),
                Undo::CallUpdate(key, old) => restore(&mut self.call_updates, key, old),
                Undo::CallTurn(key, old) => restore(&mut self.call_turn, key, old),
                Undo::Task(key, old) => restore(&mut self.tasks, key, old.map(|value| *value)),
                Undo::TaskByCall(key, old) => restore(&mut self.task_by_call, key, old),
                Undo::Delivery(key, old) => {
                    restore(&mut self.deliveries, key, old.map(|value| *value))
                }
                Undo::Permission(key, old) => {
                    restore(&mut self.permissions, key, old.map(|value| *value))
                }
                Undo::TurnStamp(key, old) => restore(&mut self.turn_stamps, key, old),
                Undo::Turn(key, old) => restore(&mut self.turns, key, old.map(|value| *value)),
                Undo::TurnFirst(key, old) => restore(&mut self.turn_first, key, old),
                Undo::Compaction(key, old) => {
                    restore(&mut self.compactions, key, old.map(|value| *value))
                }
                Undo::RolledBack(key) => {
                    self.rolled_back.remove(&key);
                }
                Undo::SkillView(key, old) => {
                    restore(&mut self.skill_views, key, old.map(|value| *value))
                }
            }
        }
    }
}

fn restore<K: Ord, V>(map: &mut BTreeMap<K, V>, key: K, old: Option<V>) {
    match old {
        Some(value) => {
            map.insert(key, value);
        }
        None => {
            map.remove(&key);
        }
    }
}

/// One Thread's incremental projection over its canonical commits.
pub(crate) struct ProjectionState {
    thread_id: String,
    watermark: u64,
    next_ordinal: u64,
    positions: BTreeMap<String, order::Position>,
    items: BTreeMap<String, (SlotKind, ThreadItem)>,
    /// Slots whose current item embeds an ephemeral preview and must not be persisted verbatim.
    ephemeral: BTreeSet<String>,
    /// Bounded runtime usage/panel summary, advanced by the shared builders.
    panel: PanelState,
    facts: Facts,
}

impl ProjectionState {
    /// Creates an empty projection for one Thread owner.
    pub(crate) fn new(thread_id: impl Into<String>, parent_id: Option<&str>) -> Self {
        Self {
            thread_id: thread_id.into(),
            watermark: 0,
            next_ordinal: 0,
            positions: BTreeMap::new(),
            items: BTreeMap::new(),
            ephemeral: BTreeSet::new(),
            panel: PanelState::default(),
            facts: Facts {
                message_source: parent_id.map(|parent| format!("agent:{parent}")),
                ..Default::default()
            },
        }
    }

    /// Rebuilds a projection from a complete journal for explicit reconstruction.
    ///
    /// # Errors
    /// Rejects a truncated or mixed-owner journal and any saved fact the slot builders reject.
    pub(crate) fn rebuild(
        thread_id: &str,
        parent_id: Option<&str>,
        snapshot: &ThreadSnapshot,
        journal: &[Arc<ThreadCommit>],
    ) -> Result<Self, ProjectionError> {
        let through = snapshot.commit_sequence;
        let positions = order::positions(journal, through)?;
        let next_ordinal = positions
            .values()
            .map(|position| position.ordinal)
            .max()
            .unwrap_or(0);
        let mut state = Self::new(thread_id, parent_id);
        state.positions = positions;
        state.next_ordinal = next_ordinal;
        state.facts.model_progress = snapshot.model_progress.clone();
        state.facts.tool_progress = snapshot.tool_progress.clone();
        for commit in journal.iter().filter(|commit| commit.sequence <= through) {
            // A rebuild never needs the panel, so it stays tolerant of a bad panel receipt.
            state.step(commit, false)?;
        }
        if state.watermark != through {
            return Err(ProjectionError::JournalOrder);
        }
        Ok(state)
    }

    /// Restores a projection from its persisted head, facts and slots without replaying any journal.
    pub(crate) fn restore(
        head: ProjectionHead,
        facts: Facts,
        panel: PanelState,
        slots: impl IntoIterator<Item = PersistedSlot>,
    ) -> Self {
        let mut positions = BTreeMap::new();
        let mut items = BTreeMap::new();
        for slot in slots {
            positions.insert(
                slot.key.clone(),
                order::Position {
                    ordinal: slot.ordinal,
                    created_at: slot.created_at,
                },
            );
            if let (Some(kind), Some(item)) = (slot.kind, slot.item) {
                items.insert(slot.key, (kind, item));
            }
        }
        Self {
            thread_id: head.thread_id,
            watermark: head.watermark,
            next_ordinal: head.next_ordinal,
            positions,
            items,
            ephemeral: BTreeSet::new(),
            panel,
            facts,
        }
    }

    /// The small persistent head of this projection.
    pub(crate) fn head(&self) -> ProjectionHead {
        ProjectionHead {
            thread_id: self.thread_id.clone(),
            watermark: self.watermark,
            next_ordinal: self.next_ordinal,
        }
    }

    /// The durable keyed facts backing this projection, excluding live previews.
    pub(crate) fn facts(&self) -> &Facts {
        &self.facts
    }

    /// The bounded runtime usage/panel summary for keyed persistence.
    pub(crate) fn panel_state(&self) -> &PanelState {
        &self.panel
    }

    /// Materializes the current runtime usage/panel summary.
    ///
    /// # Errors
    /// Rejects a saved todo/workflow/skill payload the producers cannot decode.
    pub(crate) fn panel(&self) -> Result<pl_protocol::ThreadRuntimeSnapshot, ProjectionError> {
        self.panel.materialize(&self.thread_id)
    }

    /// Alias of [`ProjectionState::head`] for a keyed store worker.
    pub(crate) fn read_head(&self) -> ProjectionHead {
        self.head()
    }

    /// Alias of [`ProjectionState::panel`] for a keyed store worker.
    ///
    /// # Errors
    /// Rejects a saved todo/workflow payload the producers cannot decode.
    pub(crate) fn read_panel(&self) -> Result<pl_protocol::ThreadRuntimeSnapshot, ProjectionError> {
        self.panel()
    }

    /// Reads only the requested slot rows, without materializing the whole projection.
    pub(crate) fn read_slots<'a>(
        &self,
        keys: impl IntoIterator<Item = &'a str>,
    ) -> Vec<PersistedSlot> {
        keys.into_iter().filter_map(|key| self.slot(key)).collect()
    }

    /// Turn ids rolled back by saved rewind replacements.
    pub(crate) fn rolled_back(&self) -> &BTreeSet<String> {
        &self.facts.rolled_back
    }

    /// Loads exactly the requested fact rows into the working set, validating each key against the
    /// row's embedded identity. It never touches the head, watermark, ordinal counter or slots.
    ///
    /// # Errors
    /// Rejects a row whose identity disagrees with its key.
    pub(crate) fn load_facts(
        &mut self,
        rows: impl IntoIterator<Item = (ProjectionFactKey, ProjectionFactRow)>,
    ) -> Result<(), ProjectionError> {
        for (key, row) in rows {
            if !row_matches_key(&key, &row) {
                return Err(ProjectionError::FactIdentity(format!("{key:?}")));
            }
            self.facts.put(key, row);
        }
        Ok(())
    }

    /// Loads exactly the requested slot rows, validating key/item identity, unique keys and unique
    /// ordinals before applying anything (atomic). It never recomputes the ordinal counter from the
    /// loaded map size and leaves the head, watermark and dirty items untouched.
    ///
    /// # Errors
    /// Rejects a slot whose item identity disagrees with its key, a duplicate key/ordinal, or an
    /// ordinal that conflicts with an already reserved slot.
    pub(crate) fn load_slots(
        &mut self,
        slots: impl IntoIterator<Item = PersistedSlot>,
    ) -> Result<(), ProjectionError> {
        let slots: Vec<PersistedSlot> = slots.into_iter().collect();
        let mut keys = BTreeSet::new();
        let mut ordinals = BTreeSet::new();
        for slot in &slots {
            if !keys.insert(slot.key.clone()) {
                return Err(ProjectionError::FactIdentity(format!(
                    "duplicate slot key {}",
                    slot.key
                )));
            }
            if !ordinals.insert(slot.ordinal) {
                return Err(ProjectionError::FactIdentity(format!(
                    "duplicate slot ordinal {}",
                    slot.ordinal
                )));
            }
            if let Some(item) = &slot.item {
                if item.id != slot.key {
                    return Err(ProjectionError::FactIdentity(format!(
                        "slot {} carries item {}",
                        slot.key, item.id
                    )));
                }
                if slot.kind.is_none() {
                    return Err(ProjectionError::FactIdentity(format!(
                        "slot {} carries an item without its kind",
                        slot.key
                    )));
                }
            }
        }
        let existing_ordinals: BTreeSet<u64> = self
            .positions
            .values()
            .map(|position| position.ordinal)
            .collect();
        for slot in &slots {
            match self.positions.get(&slot.key) {
                Some(position) if position.ordinal != slot.ordinal => {
                    return Err(ProjectionError::FactIdentity(format!(
                        "slot {} conflicts with its reserved ordinal",
                        slot.key
                    )));
                }
                None if existing_ordinals.contains(&slot.ordinal) => {
                    return Err(ProjectionError::FactIdentity(format!(
                        "ordinal {} is already reserved by another slot",
                        slot.ordinal
                    )));
                }
                _ => {}
            }
        }
        for slot in slots {
            self.positions.insert(
                slot.key.clone(),
                order::Position {
                    ordinal: slot.ordinal,
                    created_at: slot.created_at,
                },
            );
            self.ephemeral.remove(&slot.key);
            match (slot.kind, slot.item) {
                (Some(kind), Some(item)) => {
                    self.items.insert(slot.key, (kind, item));
                }
                _ => {
                    self.items.remove(&slot.key);
                }
            }
        }
        Ok(())
    }

    /// Builds the read/write plan for one commit without mutating this projection.
    pub(crate) fn apply_plan(&self, commit: &ThreadCommit) -> ProjectionPlan {
        let mut new_slots = Vec::new();
        let mut affected: BTreeSet<String> = BTreeSet::new();
        {
            let mut mark = |id: String| {
                if !self.positions.contains_key(&id) {
                    new_slots.push(id.clone());
                }
                affected.insert(id);
            };
            // Reserved slots, in the canonical admission order.
            for input in commit.inputs.iter() {
                if let InputChange::Accepted(record) = input {
                    mark(record.input.id.clone());
                }
            }
            for record in commit.inbox.iter() {
                mark(order::message_id(&record.message.id));
            }
            if let Some(turn) = &commit.turn {
                mark(order::turn_id(&turn.turn_id));
            }
            for change in commit.extensions.iter() {
                if let ExtensionChange::Put { id, record } = change
                    && record.payload.format() == "pl.studio.compaction"
                {
                    mark(order::compaction_id(id));
                }
            }
            for delivery in commit.deliveries.iter() {
                mark(order::skill_id(&delivery.call_id));
                mark(order::completion_id(&delivery.call_id));
            }
            if let Some(update) = &commit.attempt {
                for kind in ["inference", "reasoning", "text"] {
                    mark(order::response_id(&update.attempt_id, kind));
                }
                if let AttemptOutcome::Committed(output) = &update.outcome {
                    for call in &output.tool_calls {
                        mark(order::tool_id(&call.call_id));
                    }
                }
            }
        }
        // Additional slots recomputed by this commit beyond its own reservations.
        for change in commit.inputs.iter() {
            match change {
                InputChange::Accepted(record) => {
                    affected.insert(record.input.id.clone());
                }
                InputChange::Transition { id, .. } => {
                    affected.insert(id.clone());
                }
            }
        }
        if let Some(turn) = &commit.turn {
            if let Some(input_id) = &turn.input_id {
                affected.insert(input_id.clone());
            }
            affected.insert(order::turn_id(&turn.turn_id));
        }
        // Consuming messages updates earlier-slotted message items, whose positions the bounded
        // working set must load: their keys come from the `PendingMessages` lookup for this commit.
        if let Some(through) = commit.consumed_messages {
            for id in self.facts.pending_messages(through) {
                affected.insert(order::message_id(&id));
            }
        }
        if let Some(update) = &commit.attempt {
            affected.insert(order::turn_id(&update.turn_id));
            affected.insert(order::response_id(&update.attempt_id, "inference"));
            affected.insert(order::response_id(&update.attempt_id, "reasoning"));
            affected.insert(order::response_id(&update.attempt_id, "text"));
            if let AttemptOutcome::Committed(output) = &update.outcome {
                for call in &output.tool_calls {
                    affected.insert(order::tool_id(&call.call_id));
                    affected.insert(order::skill_id(&call.call_id));
                    affected.insert(order::completion_id(&call.call_id));
                }
            }
        }
        for task in commit.tasks.iter() {
            affected.insert(order::turn_id(&task.turn_id));
            affected.insert(order::tool_id(&task.call_id));
            affected.insert(order::skill_id(&task.call_id));
        }
        for permission in commit.permissions.iter() {
            affected.insert(order::tool_id(&permission.call_id));
            affected.insert(order::skill_id(&permission.call_id));
        }
        for delivery in commit.deliveries.iter() {
            affected.insert(order::tool_id(&delivery.call_id));
            affected.insert(order::skill_id(&delivery.call_id));
            affected.insert(order::completion_id(&delivery.call_id));
        }
        ProjectionPlan {
            requirements: self.requirements(commit),
            slot_keys: affected.into_iter().collect(),
        }
    }

    /// The exact fact rows and named lookups a store must load to apply `commit`.
    pub(crate) fn requirements(&self, commit: &ThreadCommit) -> ProjectionRequirements {
        let mut keys = BTreeSet::new();
        let mut queries = Vec::new();
        let push_turn = |keys: &mut BTreeSet<ProjectionFactKey>,
                         queries: &mut Vec<ProjectionQuery>,
                         turn_id: &str| {
            keys.insert(ProjectionFactKey::Turn(turn_id.into()));
            keys.insert(ProjectionFactKey::TurnStamp(turn_id.into()));
            keys.insert(ProjectionFactKey::TurnFirst(turn_id.into()));
            queries.push(ProjectionQuery::TurnTasks {
                turn_id: turn_id.into(),
            });
            queries.push(ProjectionQuery::TurnAttempts {
                turn_id: turn_id.into(),
            });
        };
        let push_call = |keys: &mut BTreeSet<ProjectionFactKey>,
                         queries: &mut Vec<ProjectionQuery>,
                         call_id: &str| {
            keys.insert(ProjectionFactKey::Call(call_id.into()));
            keys.insert(ProjectionFactKey::CallUpdate(call_id.into()));
            keys.insert(ProjectionFactKey::CallTurn(call_id.into()));
            keys.insert(ProjectionFactKey::Delivery(call_id.into()));
            queries.push(ProjectionQuery::TaskByCall {
                call_id: call_id.into(),
            });
            queries.push(ProjectionQuery::PendingPermissions {
                call_id: call_id.into(),
            });
        };
        for change in commit.inputs.iter() {
            let id = match change {
                InputChange::Accepted(record) => record.input.id.clone(),
                InputChange::Transition { id, .. } => id.clone(),
            };
            keys.insert(ProjectionFactKey::Input(id.clone()));
            queries.push(ProjectionQuery::TurnsByInput { input_id: id });
        }
        if let Some(source) = self.facts.message_source.clone() {
            for record in commit.inbox.iter() {
                if record.message.source_id == source {
                    keys.insert(ProjectionFactKey::Message(record.message.id.clone()));
                }
            }
        }
        if let Some(through) = commit.consumed_messages {
            queries.push(ProjectionQuery::PendingMessages { through });
        }
        if let Some(turn) = &commit.turn {
            push_turn(&mut keys, &mut queries, &turn.turn_id);
            if let Some(input_id) = &turn.input_id {
                keys.insert(ProjectionFactKey::Input(input_id.clone()));
                queries.push(ProjectionQuery::TurnsByInput {
                    input_id: input_id.clone(),
                });
            }
        }
        for change in commit.extensions.iter() {
            match change {
                ExtensionChange::Put { id, record } => {
                    let format = record.payload.format();
                    if format == "pl.studio.compaction" {
                        keys.insert(ProjectionFactKey::Compaction(id.clone()));
                    } else if format == "pl.tool.skill-view" {
                        // The panel needs the affected old skill row to correct a rewrite.
                        keys.insert(ProjectionFactKey::SkillView(id.clone()));
                    }
                }
                ExtensionChange::Delete { id, .. } => {
                    keys.insert(ProjectionFactKey::Compaction(id.clone()));
                    keys.insert(ProjectionFactKey::SkillView(id.clone()));
                }
            }
        }
        if let Some(update) = &commit.attempt {
            keys.insert(ProjectionFactKey::Attempt(update.attempt_id.clone()));
            push_turn(&mut keys, &mut queries, &update.turn_id);
            if let AttemptOutcome::Committed(output) = &update.outcome {
                for call in &output.tool_calls {
                    push_call(&mut keys, &mut queries, &call.call_id);
                }
            }
        }
        for task in commit.tasks.iter() {
            push_call(&mut keys, &mut queries, &task.call_id);
            keys.insert(ProjectionFactKey::Task(task.id.clone()));
            keys.insert(ProjectionFactKey::TaskByCall(task.call_id.clone()));
            push_turn(&mut keys, &mut queries, &task.turn_id);
        }
        for permission in commit.permissions.iter() {
            push_call(&mut keys, &mut queries, &permission.call_id);
            keys.insert(ProjectionFactKey::Permission(permission.id.clone()));
        }
        for delivery in commit.deliveries.iter() {
            push_call(&mut keys, &mut queries, &delivery.call_id);
        }
        ProjectionRequirements {
            keys: keys.into_iter().collect(),
            queries,
        }
    }

    /// Iterates every reserved slot for keyed persistence. A running entity's preview item is
    /// excluded: the reservation is kept but the preview content is never offered for storage.
    pub(crate) fn persisted_slots(&self) -> impl Iterator<Item = PersistedSlot> + '_ {
        self.positions.iter().map(|(key, position)| {
            let entry = self
                .items
                .get(key)
                .filter(|_| !self.ephemeral.contains(key));
            PersistedSlot {
                key: key.clone(),
                ordinal: position.ordinal,
                created_at: position.created_at,
                kind: entry.map(|(kind, _)| *kind),
                item: entry.map(|(_, item)| item.clone()),
            }
        })
    }

    /// One reserved slot's persisted row, so a keyed store loads only the new/dirty positions.
    pub(crate) fn slot(&self, key: &str) -> Option<PersistedSlot> {
        let position = self.positions.get(key)?;
        let entry = self
            .items
            .get(key)
            .filter(|_| !self.ephemeral.contains(key));
        Some(PersistedSlot {
            key: key.to_string(),
            ordinal: position.ordinal,
            created_at: position.created_at,
            kind: entry.map(|(kind, _)| *kind),
            item: entry.map(|(_, item)| item.clone()),
        })
    }

    /// Advances exactly one canonical commit, returning the affected slots and dirty fact rows.
    ///
    /// # Errors
    /// Rejects a non-contiguous or cross-owner commit and any malformed saved fact; a failure
    /// leaves the projection unchanged, including its watermark, ordinal counter, items and facts.
    pub(crate) fn apply(
        &mut self,
        commit: &ThreadCommit,
    ) -> Result<ProjectionDelta, ProjectionError> {
        self.step(commit, true)
    }

    /// Folds one commit while optionally advancing the panel summary. `rebuild` disables the panel
    /// so an explicit reconstruction never fails on a saved panel receipt the item projection
    /// tolerates.
    fn step(
        &mut self,
        commit: &ThreadCommit,
        advance_panel: bool,
    ) -> Result<ProjectionDelta, ProjectionError> {
        if commit.thread_id != self.thread_id
            || commit.sequence
                != self
                    .watermark
                    .checked_add(1)
                    .ok_or(ProjectionError::Count)?
        {
            return Err(ProjectionError::JournalOrder);
        }
        // Capture the affected old panel rows before this commit overwrites them.
        let mut previous_compactions = BTreeMap::new();
        let mut previous_skills = BTreeMap::new();
        for change in commit.extensions.iter() {
            let id = match change {
                ExtensionChange::Put { id, .. } | ExtensionChange::Delete { id, .. } => id,
            };
            if let Some(facts) = self.facts.compactions.get(id) {
                previous_compactions.insert(id.clone(), facts.accounting.clone());
            }
            if let Some(payload) = self.facts.skill_views.get(id)
                && let Ok(name) = pl_tool::skill::saved_skill_name(payload)
            {
                previous_skills.insert(id.clone(), name);
            }
        }
        let ordinal_before = self.next_ordinal;
        let added = match order::reserve_commit(commit, &mut self.positions, &mut self.next_ordinal)
        {
            Ok(added) => added,
            Err(error) => {
                self.next_ordinal = ordinal_before;
                return Err(error);
            }
        };
        let undo = match self.facts.apply(commit) {
            Ok(undo) => undo,
            Err(error) => {
                rollback_positions(&mut self.positions, &added);
                self.next_ordinal = ordinal_before;
                return Err(error);
            }
        };
        let written: BTreeSet<ProjectionFactKey> = undo.iter().map(Undo::key).collect();
        // Advance the bounded panel summary before publishing, so a failure rolls back everything.
        let panel = if advance_panel {
            match self
                .panel
                .advance(commit, &previous_compactions, &previous_skills)
            {
                Ok(panel) => panel,
                Err(error) => {
                    self.facts.revert(undo);
                    rollback_positions(&mut self.positions, &added);
                    self.next_ordinal = ordinal_before;
                    return Err(error);
                }
            }
        } else {
            self.panel.clone()
        };
        // Compute every affected slot's candidate before publishing, so a failure in one builder
        // cannot leave other slots partially advanced.
        let candidates = match self.recompute(commit) {
            Ok(candidates) => candidates,
            Err(error) => {
                self.facts.revert(undo);
                rollback_positions(&mut self.positions, &added);
                self.next_ordinal = ordinal_before;
                return Err(error);
            }
        };
        let mut delta = ProjectionDelta {
            watermark: commit.sequence,
            new_slots: added,
            panel_changed: commit.attempt.is_some()
                || commit.turn.is_some()
                || !commit.extensions.is_empty()
                || !commit.deliveries.is_empty(),
            context_disposition: rewind_dispositions(commit),
            ..Default::default()
        };
        self.publish(candidates, &mut delta);
        delta.fact_writes = written
            .into_iter()
            .map(|key| {
                let row = self.facts.get(&key);
                (key, row)
            })
            .collect();
        self.panel = panel;
        self.watermark = commit.sequence;
        Ok(delta)
    }

    /// Replaces the ephemeral model preview and returns the running slots it changed.
    ///
    /// # Errors
    /// Returns a projection failure without changing the durable state.
    pub(crate) fn set_model_progress(
        &mut self,
        progress: Option<ActiveModelProgress>,
    ) -> Result<ProjectionDelta, ProjectionError> {
        let mut ids = BTreeSet::new();
        if let Some(preview) = &self.facts.model_progress {
            ids.insert(preview.attempt_id.clone());
        }
        if let Some(preview) = &progress {
            ids.insert(preview.attempt_id.clone());
        }
        self.facts.model_progress = progress;
        let mut delta = ProjectionDelta {
            watermark: self.watermark,
            ..Default::default()
        };
        let mut candidates = Vec::new();
        for id in ids {
            candidates.extend(self.attempt_candidates(&id)?);
        }
        self.publish(candidates, &mut delta);
        Ok(delta)
    }

    /// Replaces the ephemeral tool preview and returns the running slots it changed.
    ///
    /// # Errors
    /// Returns a projection failure without changing the durable state.
    pub(crate) fn set_tool_progress(
        &mut self,
        progress: BTreeMap<String, Vec<ContextContent>>,
    ) -> Result<ProjectionDelta, ProjectionError> {
        let mut ids: BTreeSet<String> = self.facts.tool_progress.keys().cloned().collect();
        ids.extend(progress.keys().cloned());
        self.facts.tool_progress = progress;
        let mut delta = ProjectionDelta {
            watermark: self.watermark,
            ..Default::default()
        };
        let mut candidates = Vec::new();
        for task_id in ids {
            if let Some(call_id) = self
                .facts
                .tasks
                .get(&task_id)
                .map(|task| task.call_id.clone())
            {
                candidates.extend(self.call_candidates(&call_id)?);
            }
        }
        self.publish(candidates, &mut delta);
        Ok(delta)
    }

    /// Last commit sequence folded into this projection.
    pub(crate) fn watermark(&self) -> u64 {
        self.watermark
    }

    /// Next ordinal this projection will hand out.
    pub(crate) fn next_ordinal(&self) -> u64 {
        self.next_ordinal
    }

    /// Reads one slot's current item for a keyed cold read, without materializing the whole state.
    pub(crate) fn item(&self, key: &str) -> Option<&ThreadItem> {
        self.items.get(key).map(|(_, item)| item)
    }

    /// Iterates the populated slots with their category, so a storage adapter can page by key.
    pub(crate) fn slots(&self) -> impl Iterator<Item = (&str, SlotKind, &ThreadItem)> {
        self.items
            .iter()
            .map(|(key, (kind, item))| (key.as_str(), *kind, item))
    }

    /// Materializes every item ordered by its fixed admission ordinal.
    pub(crate) fn materialize(&self) -> Vec<ThreadItem> {
        let mut items: Vec<ThreadItem> = self.slots().map(|(_, _, item)| item.clone()).collect();
        items.sort_by_key(|item| item.ordinal);
        items
    }

    /// Computes the candidate items affected by one commit, reading the already-applied facts but
    /// never mutating any published state.
    fn recompute(&self, commit: &ThreadCommit) -> Result<Vec<Candidate>, ProjectionError> {
        let seq = commit.sequence;
        let mut inputs = BTreeSet::new();
        let mut turns = BTreeSet::new();
        let mut compactions = BTreeSet::new();
        let mut attempts = BTreeSet::new();
        let mut calls = BTreeSet::new();
        let mut completions = BTreeSet::new();
        for change in commit.inputs.iter() {
            match change {
                InputChange::Accepted(record) => {
                    inputs.insert(record.input.id.clone());
                }
                InputChange::Transition { id, .. } => {
                    inputs.insert(id.clone());
                }
            }
        }
        if let Some(turn) = &commit.turn {
            turns.insert(turn.turn_id.clone());
            if let Some(input_id) = &turn.input_id {
                inputs.insert(input_id.clone());
            }
        }
        for change in commit.extensions.iter() {
            if let ExtensionChange::Put { id, record } = change
                && record.payload.format() == "pl.studio.compaction"
            {
                compactions.insert(id.clone());
            }
        }
        if let Some(update) = &commit.attempt {
            attempts.insert(update.attempt_id.clone());
            turns.insert(update.turn_id.clone());
            if let AttemptOutcome::Committed(output) = &update.outcome {
                for call in &output.tool_calls {
                    calls.insert(call.call_id.clone());
                    completions.insert(call.call_id.clone());
                }
            }
        }
        for task in commit.tasks.iter() {
            calls.insert(task.call_id.clone());
            turns.insert(task.turn_id.clone());
        }
        for permission in commit.permissions.iter() {
            calls.insert(permission.call_id.clone());
        }
        for delivery in commit.deliveries.iter() {
            calls.insert(delivery.call_id.clone());
            completions.insert(delivery.call_id.clone());
        }
        // Any message whose facts changed in this commit (admission or consumption).
        let messages: Vec<String> = self
            .facts
            .messages
            .iter()
            .filter(|(_, facts)| facts.revision == seq)
            .map(|(id, _)| id.clone())
            .collect();

        let mut candidates = Vec::new();
        for id in &inputs {
            candidates.extend(self.input_candidate(id));
        }
        for id in &messages {
            candidates.extend(self.message_candidate(id));
        }
        for id in &turns {
            candidates.extend(self.turn_candidate(id)?);
        }
        for id in &compactions {
            candidates.extend(self.compaction_candidate(id)?);
        }
        for id in &attempts {
            candidates.extend(self.attempt_candidates(id)?);
        }
        for id in &calls {
            candidates.extend(self.call_candidates(id)?);
        }
        for id in &completions {
            candidates.extend(self.completion_candidate(id));
        }
        Ok(candidates)
    }

    fn publish(&mut self, candidates: Vec<Candidate>, delta: &mut ProjectionDelta) {
        for candidate in candidates {
            let key = candidate.key;
            let Some(position) = self.positions.get(&key).copied() else {
                self.ephemeral.remove(&key);
                if self.items.remove(&key).is_some() {
                    delta.removed.push(key);
                }
                continue;
            };
            match candidate.item {
                Some(mut item) => {
                    item.ordinal = position.ordinal;
                    item.created_at = position.created_at;
                    let changed = self
                        .items
                        .get(&key)
                        .is_none_or(|(_, previous)| previous != &item);
                    if changed {
                        delta.changed.push(key.clone());
                    }
                    if candidate.ephemeral {
                        self.ephemeral.insert(key.clone());
                    } else {
                        self.ephemeral.remove(&key);
                    }
                    self.items.insert(key, (candidate.kind, item));
                }
                None => {
                    self.ephemeral.remove(&key);
                    if self.items.remove(&key).is_some() {
                        delta.removed.push(key);
                    }
                }
            }
        }
    }

    fn input_candidate(&self, id: &str) -> Option<Candidate> {
        let facts = self.facts.inputs.get(id)?;
        let turn_id = match &facts.record.state {
            InputState::Consumed { turn_id, .. } => turn_id.clone(),
            InputState::Pending | InputState::Discarded => {
                self.facts.latest_turn_for_input(id).unwrap_or_default()
            }
        };
        let item = inputs::input_item(
            &self.thread_id,
            &facts.record,
            facts.accepted,
            facts.updated,
            &turn_id,
        );
        Some(Candidate {
            key: id.to_string(),
            kind: SlotKind::Input,
            item,
            ephemeral: false,
        })
    }

    fn message_candidate(&self, id: &str) -> Option<Candidate> {
        let facts = self.facts.messages.get(id)?;
        let item = messages::message_item(
            &self.thread_id,
            &facts.message,
            &facts.turn_id,
            facts.revision,
            facts.created_at,
            facts.updated_at,
        );
        Some(Candidate {
            key: order::message_id(id),
            kind: SlotKind::Message,
            item: Some(item),
            ephemeral: false,
        })
    }

    fn turn_candidate(&self, id: &str) -> Result<Option<Candidate>, ProjectionError> {
        let key = order::turn_id(id);
        let (Some(record), Some(stamp)) = (
            self.facts.turns.get(id).cloned(),
            self.facts.turn_stamps.get(id).copied(),
        ) else {
            return Ok(Some(Candidate {
                key,
                kind: SlotKind::Turn,
                item: None,
                ephemeral: false,
            }));
        };
        let tasks: Vec<&TaskRecord> = self
            .facts
            .turn_tasks(id)
            .iter()
            .filter_map(|task_id| self.facts.tasks.get(task_id))
            .collect();
        let latest_attempt = self.latest_attempt_outcome(id);
        let latest_failure = self.latest_failure(id);
        let turn = turns::turn_projected(
            &self.thread_id,
            &record,
            &stamp,
            &tasks,
            latest_attempt,
            latest_failure,
        )?;
        let item = ThreadItem::new(
            order::turn_id(&turn.id),
            self.thread_id.clone(),
            turn.id.clone(),
            0,
            turn.revision,
            turn.state.started_at().unwrap_or(turn.updated_at),
            turn.updated_at,
            ThreadItemState::Turn(ThreadTurnItem::new(turn.state).with_input_id(turn.input_id)),
        );
        Ok(Some(Candidate {
            key,
            kind: SlotKind::Turn,
            item: Some(item),
            ephemeral: false,
        }))
    }

    fn compaction_candidate(&self, id: &str) -> Result<Option<Candidate>, ProjectionError> {
        let Some(facts) = self.facts.compactions.get(id) else {
            return Ok(None);
        };
        let item = compactions::compaction_item(
            &self.thread_id,
            id,
            &facts.record,
            facts.commit_at,
            facts.compaction,
        )?;
        Ok(Some(Candidate {
            key: order::compaction_id(id),
            kind: SlotKind::Compaction,
            item,
            ephemeral: false,
        }))
    }

    fn attempt_candidates(&self, id: &str) -> Result<Vec<Candidate>, ProjectionError> {
        let Some(facts) = self.facts.attempts.get(id) else {
            return Ok(Vec::new());
        };
        let preview = self
            .facts
            .model_progress
            .as_ref()
            .filter(|preview| preview.attempt_id == id);
        let ephemeral =
            matches!(facts.record.outcome, AttemptOutcome::Running) && preview.is_some();
        let items = responses::response_items(
            &self.thread_id,
            &facts.record,
            facts.created_at,
            facts.updated_at,
            facts.revision,
            preview,
        )?;
        let mut by_key: BTreeMap<String, ThreadItem> = items
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect();
        let mut candidates = Vec::new();
        for (key, kind, is_preview) in [
            (
                order::response_id(id, "inference"),
                SlotKind::Inference,
                false,
            ),
            (
                order::response_id(id, "reasoning"),
                SlotKind::Reasoning,
                true,
            ),
            (order::response_id(id, "text"), SlotKind::ResponseText, true),
        ] {
            candidates.push(Candidate {
                key: key.clone(),
                kind,
                item: by_key.remove(&key),
                ephemeral: is_preview && ephemeral,
            });
        }
        Ok(candidates)
    }

    fn call_candidates(&self, call_id: &str) -> Result<Vec<Candidate>, ProjectionError> {
        let Some(saved) = self.facts.calls.get(call_id) else {
            return Ok(Vec::new());
        };
        let task = self
            .facts
            .task_by_call
            .get(call_id)
            .and_then(|task_id| self.facts.tasks.get(task_id))
            .cloned();
        let delivery = self
            .facts
            .deliveries
            .get(call_id)
            .map(|facts| facts.delivery.clone());
        let permission_pending = self.facts.has_pending_permission(call_id);
        let progress = task
            .as_ref()
            .and_then(|task| self.facts.tool_progress.get(&task.id))
            .map(|content| text_content(content))
            .unwrap_or_default();
        let preview = task
            .as_ref()
            .is_some_and(|task| task.status == TaskStatus::Running)
            && !progress.is_empty();
        let (revision, updated_at) = self
            .facts
            .call_updates
            .get(call_id)
            .copied()
            .unwrap_or((saved.sequence, saved.at));
        let items = tools::call_items(
            &self.thread_id,
            &saved.turn_id,
            &saved.call,
            saved.at,
            task.as_ref(),
            delivery.as_ref(),
            permission_pending,
            &progress,
            revision,
            updated_at,
        )?;
        let mut by_key: BTreeMap<String, ThreadItem> = items
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect();
        let tool_key = order::tool_id(call_id);
        let skill_key = order::skill_id(call_id);
        let tool = by_key.remove(&tool_key);
        let skill = by_key.remove(&skill_key);
        Ok(vec![
            Candidate {
                key: skill_key,
                kind: SlotKind::Skill,
                item: skill,
                ephemeral: false,
            },
            Candidate {
                key: tool_key,
                kind: SlotKind::Tool,
                item: tool,
                ephemeral: preview,
            },
        ])
    }

    fn completion_candidate(&self, call_id: &str) -> Option<Candidate> {
        let key = order::completion_id(call_id);
        let Some(facts) = self.facts.deliveries.get(call_id) else {
            return Some(Candidate {
                key,
                kind: SlotKind::Completion,
                item: None,
                ephemeral: false,
            });
        };
        let turn_id = self
            .facts
            .call_turn
            .get(call_id)
            .cloned()
            .unwrap_or_default();
        let item = completions::completion_item(
            &self.thread_id,
            &facts.delivery,
            &turn_id,
            facts.sequence,
            facts.at,
        );
        Some(Candidate {
            key,
            kind: SlotKind::Completion,
            item,
            ephemeral: false,
        })
    }

    fn latest_attempt_outcome(&self, turn_id: &str) -> Option<&AttemptOutcome> {
        self.facts
            .turn_attempts(turn_id)
            .iter()
            .rev()
            .find_map(|attempt_id| self.facts.attempts.get(attempt_id))
            .map(|facts| &facts.record.outcome)
    }

    fn latest_failure(&self, turn_id: &str) -> Option<&Arc<ModelError>> {
        self.facts
            .turn_attempts(turn_id)
            .iter()
            .rev()
            .find_map(|attempt_id| {
                let facts = self.facts.attempts.get(attempt_id)?;
                match &facts.record.outcome {
                    AttemptOutcome::Failed(error) => Some(error),
                    _ => None,
                }
            })
    }
}

fn rollback_positions(positions: &mut BTreeMap<String, order::Position>, added: &[String]) {
    for key in added {
        positions.remove(key);
    }
}

/// Turn dispositions produced by one commit's saved rewind replacements.
fn rewind_dispositions(commit: &ThreadCommit) -> Vec<(String, ThreadContextDisposition)> {
    let mut dispositions = Vec::new();
    for replacement in commit.replacements.iter() {
        if replacement.reason != ContextReplacementReason::Rewind {
            continue;
        }
        let retained: BTreeSet<&str> = replacement
            .current
            .records
            .iter()
            .filter_map(|record| record.turn_id.as_deref())
            .collect();
        for record in replacement.previous.records.iter() {
            if let Some(turn_id) = &record.turn_id
                && !retained.contains(turn_id.as_str())
            {
                dispositions.push((turn_id.clone(), ThreadContextDisposition::RolledBack));
            }
        }
    }
    dispositions
}

/// Whether a loaded row's embedded identity agrees with the key that named it.
fn row_matches_key(key: &ProjectionFactKey, row: &ProjectionFactRow) -> bool {
    match (key, row) {
        (ProjectionFactKey::Input(key), ProjectionFactRow::Input(value)) => {
            &value.record.input.id == key
        }
        (ProjectionFactKey::Message(key), ProjectionFactRow::Message(value)) => {
            &value.message.id == key
        }
        (ProjectionFactKey::Attempt(key), ProjectionFactRow::Attempt(value)) => {
            &value.record.attempt_id == key
        }
        (ProjectionFactKey::Call(key), ProjectionFactRow::Call(value)) => {
            &value.call.call_id == key
        }
        (ProjectionFactKey::CallUpdate(_), ProjectionFactRow::CallUpdate(..)) => true,
        (ProjectionFactKey::CallTurn(_), ProjectionFactRow::CallTurn(_)) => true,
        (ProjectionFactKey::Task(key), ProjectionFactRow::Task(value)) => &value.id == key,
        (ProjectionFactKey::TaskByCall(_), ProjectionFactRow::TaskByCall(_)) => true,
        (ProjectionFactKey::Delivery(key), ProjectionFactRow::Delivery(value)) => {
            &value.delivery.call_id == key
        }
        (ProjectionFactKey::Permission(key), ProjectionFactRow::Permission(value)) => {
            &value.id == key
        }
        (ProjectionFactKey::TurnStamp(_), ProjectionFactRow::TurnStamp(_)) => true,
        (ProjectionFactKey::TurnFirst(_), ProjectionFactRow::TurnFirst(_)) => true,
        (ProjectionFactKey::Turn(key), ProjectionFactRow::Turn(value)) => &value.turn_id == key,
        (ProjectionFactKey::Compaction(_), ProjectionFactRow::Compaction(_)) => true,
        (ProjectionFactKey::RolledBack(_), ProjectionFactRow::RolledBack) => true,
        (ProjectionFactKey::SkillView(_), ProjectionFactRow::SkillView(_)) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{
        context::OpaquePayload,
        model::{ModelProgress, ModelStepOutput, ModelUsage},
        thread::{
            TurnOutcome as CoreTurnOutcome,
            input::InputDelivery,
            journal::{AttemptUpdate, ThreadCommit},
        },
    };
    use pretty_assertions::assert_eq;

    fn commit(sequence: u64) -> ThreadCommit {
        ThreadCommit {
            committed_at: sequence as i64,
            thread_id: "thread".into(),
            sequence,
            permissions: Vec::new().into(),
            wake_messages_through: None,
            inputs: Vec::new().into(),
            tasks: Vec::new().into(),
            context: None,
            private_context: None,
            attempt: None,
            turn: None,
            discovered_tools: None,
            deliveries: Vec::new().into(),
            extensions: Vec::new().into(),
            inbox: Vec::new().into(),
            consumed_messages: None,
            interactions: Vec::new().into(),
            replacements: Vec::new().into(),
            runtime_facts: None,
            lifecycle: None,
        }
    }

    fn prompt(text: &str, presentation: &str) -> OpaquePayload {
        OpaquePayload::new(
            "pl.studio.prompt",
            1,
            serde_json::json!({
                "text": text,
                "presentation": presentation,
                "attachments": [],
            })
            .to_string(),
        )
        .unwrap()
    }

    fn accepted(id: &str, ordinal: u64, payload: OpaquePayload) -> InputChange {
        InputChange::Accepted(InputRecord {
            accepted_sequence: 1,
            delivery: InputDelivery::NextTurn,
            ordinal,
            revision: 1,
            state: InputState::Pending,
            input: pl_core::thread::input::ThreadInput {
                id: id.into(),
                payload,
                context: Vec::new(),
            },
        })
    }

    fn attempt(attempt_id: &str, outcome: AttemptOutcome) -> AttemptUpdate {
        AttemptUpdate {
            request_metadata: None,
            tool_projection: None,
            turn_id: "t1".into(),
            attempt_id: attempt_id.into(),
            retry_of: None,
            input_revision: 0,
            tools: Vec::new().into(),
            outcome,
            input_estimate: None,
        }
    }

    fn output(attempt_id: &str, content: Vec<ContextContent>) -> ModelStepOutput {
        ModelStepOutput {
            attempt_id: attempt_id.into(),
            base_context_revision: 0,
            content,
            tool_calls: Vec::new(),
            private_context: None,
            usage: ModelUsage::default(),
        }
    }

    fn text(text: &str) -> ContextContent {
        ContextContent::Text { text: text.into() }
    }

    fn started_turn() -> TurnRecord {
        TurnRecord {
            elapsed_ms: None,
            input_id: Some("input1".into()),
            turn_id: "t1".into(),
            state: CoreTurnState::Running,
            model_steps: 0,
        }
    }

    fn changed(delta: &ProjectionDelta) -> BTreeSet<String> {
        delta.changed.iter().cloned().collect()
    }

    #[test]
    fn hidden_slots_keep_their_ordinal_and_each_commit_only_reports_affected_slots() {
        let mut state = ProjectionState::new("thread", None);
        let mut first = commit(1);
        first.inputs = vec![
            accepted("input1", 1, prompt("hello", "visible")),
            accepted("input2", 2, prompt("secret", "hidden")),
        ]
        .into();
        first.turn = Some(started_turn());
        let delta = state.apply(&first).unwrap();
        assert_eq!(state.item("input1").unwrap().ordinal, 1);
        assert!(state.item("input2").is_none());
        assert_eq!(state.item(&order::turn_id("t1")).unwrap().ordinal, 3);
        assert_eq!(
            changed(&delta),
            BTreeSet::from(["input1".to_string(), order::turn_id("t1")])
        );
        // New ordinals and their dirty facts are reported for the store.
        assert_eq!(
            delta.new_slots.iter().cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "input1".to_string(),
                "input2".to_string(),
                order::turn_id("t1"),
            ])
        );
        let written: BTreeSet<_> = delta
            .fact_writes
            .iter()
            .map(|(key, _)| key.clone())
            .collect();
        assert!(written.contains(&ProjectionFactKey::Input("input1".into())));
        assert!(written.contains(&ProjectionFactKey::Turn("t1".into())));

        let mut second = commit(2);
        second.attempt = Some(attempt("a1", AttemptOutcome::Running));
        let delta = state.apply(&second).unwrap();
        assert_eq!(
            changed(&delta),
            BTreeSet::from([order::response_id("a1", "inference"), order::turn_id("t1")])
        );
        assert_eq!(
            state
                .item(&order::response_id("a1", "inference"))
                .unwrap()
                .ordinal,
            4
        );

        let mut third = commit(3);
        third.attempt = Some(attempt(
            "a1",
            AttemptOutcome::Committed(output("a1", vec![text("answer")])),
        ));
        let delta = state.apply(&third).unwrap();
        assert_eq!(
            changed(&delta),
            BTreeSet::from([
                order::response_id("a1", "inference"),
                order::response_id("a1", "text"),
                order::turn_id("t1"),
            ])
        );

        let mut fourth = commit(4);
        fourth.turn = Some(TurnRecord {
            state: CoreTurnState::Finished(CoreTurnOutcome::Completed),
            model_steps: 1,
            ..started_turn()
        });
        let delta = state.apply(&fourth).unwrap();
        // Finishing the Turn also advances the input it was opened for, as the saved journal does.
        assert_eq!(
            changed(&delta),
            BTreeSet::from([order::turn_id("t1"), "input1".to_string()])
        );
    }

    #[test]
    fn requirements_name_the_keys_and_lookups_a_store_must_load_for_one_commit() {
        let state = ProjectionState::new("thread", None);
        let mut first = commit(1);
        first.inputs = vec![accepted("input1", 1, prompt("hello", "visible"))].into();
        first.turn = Some(started_turn());
        let plan = state.requirements(&first);
        assert!(
            plan.keys
                .contains(&ProjectionFactKey::Input("input1".into()))
        );
        assert!(plan.keys.contains(&ProjectionFactKey::Turn("t1".into())));
        assert!(
            plan.keys
                .contains(&ProjectionFactKey::TurnStamp("t1".into()))
        );
        assert!(plan.queries.contains(&ProjectionQuery::TurnTasks {
            turn_id: "t1".into()
        }));
        assert!(plan.queries.contains(&ProjectionQuery::TurnsByInput {
            input_id: "input1".into()
        }));
    }

    #[test]
    fn typed_fact_rows_round_trip_and_never_carry_previews() {
        let mut state = ProjectionState::new("thread", None);
        let mut first = commit(1);
        first.turn = Some(started_turn());
        state.apply(&first).unwrap();
        let mut second = commit(2);
        second.attempt = Some(attempt("a1", AttemptOutcome::Running));
        state.apply(&second).unwrap();
        let preview = ActiveModelProgress {
            attempt_id: "a1".into(),
            progress: ModelProgress {
                content: vec![text("live draft")],
                reasoning: None,
            },
        };
        state.set_model_progress(Some(preview)).unwrap();

        // The running preview is visible in the hot items but never offered for persistence.
        assert!(state.item(&order::response_id("a1", "text")).is_some());
        let persisted_text = state
            .persisted_slots()
            .find(|slot| slot.key == order::response_id("a1", "text"))
            .unwrap();
        assert!(persisted_text.item.is_none());
        // A keyed cold read loads only the needed slot position, without materializing the rest.
        assert!(
            state
                .slot(&order::response_id("a1", "text"))
                .unwrap()
                .item
                .is_none()
        );

        // Durable fact rows round-trip by key through get/put.
        let key = ProjectionFactKey::Turn("t1".into());
        let row = ProjectionState::facts(&state).get(&key).unwrap();
        let mut restored_facts = Facts::default();
        restored_facts.put(key.clone(), row);
        assert!(restored_facts.get(&key).is_some());
    }

    #[test]
    fn a_commit_that_fails_after_a_successful_upsert_leaves_items_ordinals_and_watermark_unchanged()
    {
        let mut state = ProjectionState::new("thread", None);
        let mut first = commit(1);
        first.inputs = vec![accepted("input1", 1, prompt("hello", "visible"))].into();
        first.turn = Some(started_turn());
        first.attempt = Some(AttemptUpdate {
            request_metadata: None,
            tool_projection: None,
            turn_id: "t1".into(),
            attempt_id: "a1".into(),
            retry_of: None,
            input_revision: 0,
            tools: Vec::new().into(),
            outcome: AttemptOutcome::Committed(ModelStepOutput {
                attempt_id: "a1".into(),
                base_context_revision: 0,
                content: Vec::new(),
                tool_calls: vec![pl_core::model::ModelToolCall {
                    call_id: "c".into(),
                    tool_id: "x".into(),
                    arguments: OpaquePayload::text("{}"),
                }],
                private_context: None,
                usage: ModelUsage::default(),
            }),
            input_estimate: None,
        });
        state.apply(&first).unwrap();
        let before = state.materialize();
        let watermark = state.watermark();
        let next_ordinal = state.next_ordinal();

        // This commit first changes input1, then a saved terminal task without its delivery makes
        // the tool-call projection fail, so nothing may be published.
        let mut broken = commit(2);
        broken.inputs = vec![InputChange::Transition {
            id: "input1".into(),
            revision: 2,
            state: InputState::Discarded,
        }]
        .into();
        broken.tasks = vec![TaskRecord {
            id: "task:c".into(),
            call_id: "c".into(),
            tool_id: "x".into(),
            turn_id: "t1".into(),
            revision: 1,
            status: TaskStatus::Succeeded,
            cancel_requested: false,
            acknowledgement: None,
        }]
        .into();
        let error = state.apply(&broken).unwrap_err();
        assert!(matches!(error, ProjectionError::MissingToolResult(id) if id == "c"));
        assert_eq!(state.watermark(), watermark);
        assert_eq!(state.next_ordinal(), next_ordinal);
        assert_eq!(state.materialize(), before);
    }

    #[test]
    fn persisted_slots_and_facts_restore_the_projection_without_replaying_the_journal() {
        let journal: Vec<_> = {
            let mut first = commit(1);
            first.inputs = vec![accepted("input1", 1, prompt("hello", "visible"))].into();
            first.turn = Some(started_turn());
            let mut second = commit(2);
            second.attempt = Some(attempt("a1", AttemptOutcome::Running));
            let mut third = commit(3);
            third.attempt = Some(attempt(
                "a1",
                AttemptOutcome::Committed(output("a1", vec![text("answer")])),
            ));
            [first, second, third].into_iter().map(Arc::new).collect()
        };
        let state = ProjectionState::rebuild(
            "thread",
            None,
            &ThreadSnapshot {
                commit_sequence: 3,
                ..Default::default()
            },
            &journal,
        )
        .unwrap();

        let restored = ProjectionState::restore(
            state.head(),
            state.facts().clone(),
            state.panel_state().clone(),
            state.persisted_slots(),
        );
        assert_eq!(restored.materialize(), state.materialize());
        assert_eq!(restored.watermark(), state.watermark());
        assert_eq!(restored.next_ordinal(), state.next_ordinal());
        assert_eq!(
            restored.panel().unwrap().usage,
            state.panel().unwrap().usage
        );
        assert_eq!(
            restored.persisted_slots().count(),
            state.persisted_slots().count()
        );
    }

    fn prepared_request(model: &str, window: Option<u64>) -> OpaquePayload {
        let mut binding = serde_json::json!({
            "providerInstanceId": "p",
            "requestedModel": model,
            "adapter": serde_json::to_value(pl_model::provider::ProviderAdapterKind::DeepSeek)
                .unwrap(),
            "protocol": serde_json::to_value(pl_model::provider::ProviderWireProtocol::Responses)
                .unwrap(),
            "isolation": "p::responses",
            "purpose": "turn",
        });
        if let Some(window) = window {
            binding["contextWindow"] = serde_json::json!(window);
        }
        let content = serde_json::json!({
            "binding": binding,
            "tools": [],
            "toolChoice": "auto",
            "parallelToolCalls": false,
            "reasoning": null,
            "temperature": null,
            "maxTokens": null,
        });
        OpaquePayload::new("pl.model.prepared-request", 1, content.to_string()).unwrap()
    }

    #[test]
    fn panel_state_advances_per_commit_and_keeps_model_capacity_and_usage() {
        let mut state = ProjectionState::new("thread", None);
        let mut first = commit(1);
        first.attempt = Some(AttemptUpdate {
            request_metadata: Some(prepared_request("main-model", Some(2_000))),
            tool_projection: None,
            turn_id: "t1".into(),
            attempt_id: "a1".into(),
            retry_of: None,
            input_revision: 0,
            tools: Vec::new().into(),
            outcome: AttemptOutcome::Committed(ModelStepOutput {
                attempt_id: "a1".into(),
                base_context_revision: 0,
                content: Vec::new(),
                tool_calls: Vec::new(),
                private_context: None,
                usage: ModelUsage {
                    input_tokens: Some(5),
                    output_tokens: Some(3),
                    ..Default::default()
                },
            }),
            input_estimate: None,
        });
        let delta = state.apply(&first).unwrap();
        assert!(delta.panel_changed);
        let panel = state.panel().unwrap();
        assert_eq!(panel.usage.model, "main-model");
        assert_eq!(panel.usage.context_window, Some(2_000));
        assert_eq!(panel.usage.prompt_tokens, 5);
        assert_eq!(panel.usage.completion_tokens, 3);
        assert_eq!(panel.usage.inference_count, 1);
    }

    #[test]
    fn load_facts_validates_identity_without_touching_the_head() {
        let mut state = ProjectionState::new("thread", None);
        let InputChange::Accepted(record) = accepted("a", 1, prompt("x", "visible")) else {
            unreachable!("accepted")
        };
        let facts = Facts {
            inputs: BTreeMap::from([(
                "a".to_string(),
                InputFacts {
                    record,
                    accepted: (1, 1),
                    updated: (1, 1),
                },
            )]),
            ..Default::default()
        };
        let key = ProjectionFactKey::Input("a".into());
        let row = facts.get(&key).unwrap();
        // A row whose identity disagrees with its key is rejected and mutates nothing.
        let error = state
            .load_facts([(ProjectionFactKey::Input("b".into()), row.clone())])
            .unwrap_err();
        assert!(matches!(error, ProjectionError::FactIdentity(_)));
        assert_eq!(state.watermark(), 0);
        assert!(state.facts().get(&key).is_none());
        // A matching row loads into the working set; the head and watermark stay put.
        state.load_facts([(key.clone(), row)]).unwrap();
        assert!(state.facts().get(&key).is_some());
        assert_eq!(state.watermark(), 0);
        assert_eq!(state.next_ordinal(), 0);
    }

    #[test]
    fn rewind_replacements_report_rolled_back_turn_ids_and_persist_them() {
        use pl_core::{
            context::{ContextRecord, ContextSnapshot, ContextSource},
            thread::ContextReplacement,
        };
        fn record(turn_id: &str) -> ContextRecord {
            ContextRecord {
                id: format!("r:{turn_id}"),
                turn_id: Some(turn_id.into()),
                source: ContextSource::Runtime {
                    source_id: "s".into(),
                },
                content: Vec::new(),
                tool_calls: Vec::new(),
            }
        }
        let mut state = ProjectionState::new("thread", None);
        let mut commit = commit(1);
        commit.replacements = vec![ContextReplacement {
            reason: ContextReplacementReason::Rewind,
            previous: ContextSnapshot {
                revision: 1,
                records: vec![record("t1")].into(),
            },
            current: ContextSnapshot {
                revision: 2,
                records: Vec::new().into(),
            },
            previous_private_context: None,
        }]
        .into();
        let delta = state.apply(&commit).unwrap();
        assert_eq!(
            delta.context_disposition,
            vec![("t1".to_string(), ThreadContextDisposition::RolledBack)]
        );
        assert!(state.rolled_back().contains("t1"));
        // Rewind state is a durable keyed fact the store writes back.
        let written: BTreeSet<_> = delta
            .fact_writes
            .iter()
            .map(|(key, _)| key.clone())
            .collect();
        assert!(written.contains(&ProjectionFactKey::RolledBack("t1".into())));
    }

    #[test]
    fn a_state_loaded_with_only_the_commit_working_set_matches_the_full_state() {
        let journal: Vec<_> = {
            let mut first = commit(1);
            first.inputs = vec![accepted("input1", 1, prompt("hello", "visible"))].into();
            first.turn = Some(started_turn());
            let mut second = commit(2);
            second.attempt = Some(attempt("a1", AttemptOutcome::Running));
            [first, second].into_iter().map(Arc::new).collect()
        };
        let mut full = ProjectionState::rebuild(
            "thread",
            None,
            &ThreadSnapshot {
                commit_sequence: 2,
                ..Default::default()
            },
            &journal,
        )
        .unwrap();
        let mut third = commit(3);
        third.attempt = Some(attempt(
            "a1",
            AttemptOutcome::Committed(output("a1", vec![text("answer")])),
        ));

        // Load only what this commit's plan names, never the whole state.
        let plan = full.apply_plan(&third);
        let rows: Vec<_> = plan
            .requirements
            .keys
            .iter()
            .filter_map(|key| full.facts().get(key).map(|row| (key.clone(), row)))
            .collect();
        let slots = full.read_slots(plan.slot_keys.iter().map(String::as_str));
        assert!(slots.iter().any(|slot| slot.key == order::turn_id("t1")));
        assert!(
            slots
                .iter()
                .any(|slot| slot.key == order::response_id("a1", "inference"))
        );

        let mut minimal = ProjectionState::restore(
            full.head(),
            Facts::default(),
            full.panel_state().clone(),
            Vec::new(),
        );
        minimal.load_facts(rows).unwrap();
        minimal.load_slots(slots).unwrap();

        let full_delta = full.apply(&third).unwrap();
        let minimal_delta = minimal.apply(&third).unwrap();
        // The affected slots and the sequence/ordinal head agree with the full state.
        assert_eq!(minimal_delta.changed, full_delta.changed);
        assert_eq!(minimal_delta.new_slots, full_delta.new_slots);
        assert_eq!(minimal.watermark(), full.watermark());
        assert_eq!(minimal.next_ordinal(), full.next_ordinal());
        for key in &full_delta.changed {
            assert_eq!(minimal.item(key), full.item(key));
        }
    }

    #[test]
    fn a_consuming_commit_loads_consumed_message_positions_from_the_plan() {
        use pl_core::thread::inbox::{InboxRecord, ThreadMessage};
        let mut first = commit(1);
        first.inbox = vec![InboxRecord {
            sequence: 1,
            message: ThreadMessage {
                id: "m1".into(),
                source_id: "agent:parent".into(),
                payload: OpaquePayload::text("task"),
                context: Vec::new(),
            },
        }]
        .into();
        let mut full = ProjectionState::rebuild(
            "thread",
            Some("parent"),
            &ThreadSnapshot {
                commit_sequence: 1,
                ..Default::default()
            },
            &[Arc::new(first)],
        )
        .unwrap();
        let mut second = commit(2);
        second.turn = Some(started_turn());
        second.attempt = Some(attempt("a1", AttemptOutcome::Running));
        second.consumed_messages = Some(1);

        let plan = full.apply_plan(&second);
        // The consumed message's slot position is in the plan, and its lookup is declared.
        assert!(plan.slot_keys.contains(&order::message_id("m1")));
        assert!(
            plan.requirements
                .queries
                .contains(&ProjectionQuery::PendingMessages { through: 1 })
        );

        // Resolve the plan's named lookup into extra rows, exactly like a keyed store would.
        let mut rows: Vec<_> = plan
            .requirements
            .keys
            .iter()
            .filter_map(|key| full.facts().get(key).map(|row| (key.clone(), row)))
            .collect();
        for query in &plan.requirements.queries {
            if let ProjectionQuery::PendingMessages { through } = query {
                for id in full.facts().pending_messages(*through) {
                    let key = ProjectionFactKey::Message(id);
                    if let Some(row) = full.facts().get(&key) {
                        rows.push((key, row));
                    }
                }
            }
        }
        let slots = full.read_slots(plan.slot_keys.iter().map(String::as_str));
        assert!(slots.iter().any(|slot| slot.key == order::message_id("m1")));

        let mut minimal = ProjectionState::restore(
            full.head(),
            Facts::default(),
            full.panel_state().clone(),
            Vec::new(),
        );
        minimal.load_facts(rows).unwrap();
        minimal.load_slots(slots).unwrap();

        let full_delta = full.apply(&second).unwrap();
        let minimal_delta = minimal.apply(&second).unwrap();
        assert_eq!(minimal_delta.changed, full_delta.changed);
        assert_eq!(minimal_delta.new_slots, full_delta.new_slots);
        // The bounded working set updates the consumed message exactly like the full state.
        assert_eq!(
            minimal.item(&order::message_id("m1")),
            full.item(&order::message_id("m1"))
        );
        assert_eq!(
            minimal.item(&order::message_id("m1")).unwrap().turn_id,
            "t1"
        );
    }

    #[test]
    fn terminal_result_is_never_overwritten_by_a_later_stale_preview() {
        let mut state = ProjectionState::new("thread", None);
        let mut first = commit(1);
        first.turn = Some(started_turn());
        state.apply(&first).unwrap();
        let mut second = commit(2);
        second.attempt = Some(attempt("a1", AttemptOutcome::Running));
        state.apply(&second).unwrap();

        let preview = ActiveModelProgress {
            attempt_id: "a1".into(),
            progress: ModelProgress {
                content: vec![text("live draft")],
                reasoning: None,
            },
        };
        let delta = state.set_model_progress(Some(preview.clone())).unwrap();
        assert_eq!(
            changed(&delta),
            BTreeSet::from([order::response_id("a1", "text")])
        );
        assert_eq!(
            state
                .item(&order::response_id("a1", "text"))
                .unwrap()
                .state(),
            &ThreadItemState::Text(pl_protocol::ThreadTextItem::new(
                pl_protocol::ThreadTextChannel::Commentary,
                "live draft".into(),
                Vec::new(),
                pl_protocol::ThreadContentLifecycle::streaming(),
            ))
        );

        let mut third = commit(3);
        third.attempt = Some(attempt(
            "a1",
            AttemptOutcome::Committed(output("a1", vec![text("final answer")])),
        ));
        state.apply(&third).unwrap();
        assert_eq!(
            state
                .item(&order::response_id("a1", "text"))
                .unwrap()
                .text()
                .map(|text| text.text()),
            Some("final answer")
        );

        // A stale preview arriving after the terminal commit cannot reopen the attempt.
        let delta = state.set_model_progress(Some(preview)).unwrap();
        assert!(delta.changed.is_empty() && delta.removed.is_empty());
        assert_eq!(
            state
                .item(&order::response_id("a1", "text"))
                .unwrap()
                .text()
                .map(|text| text.text()),
            Some("final answer")
        );
    }

    #[test]
    fn parent_message_is_reserved_before_consumption_and_linked_to_its_turn() {
        use pl_core::thread::inbox::{InboxRecord, ThreadMessage};
        let mut state = ProjectionState::new("thread", Some("parent"));
        let mut first = commit(1);
        first.inbox = vec![
            InboxRecord {
                sequence: 1,
                message: ThreadMessage {
                    id: "m1".into(),
                    source_id: "agent:parent".into(),
                    payload: OpaquePayload::text(" task "),
                    context: Vec::new(),
                },
            },
            InboxRecord {
                sequence: 2,
                message: ThreadMessage {
                    id: "m2".into(),
                    source_id: "studio.notification".into(),
                    payload: OpaquePayload::text("internal"),
                    context: Vec::new(),
                },
            },
        ]
        .into();
        let delta = state.apply(&first).unwrap();
        assert!(changed(&delta).contains(&order::message_id("m1")));
        assert!(state.item(&order::message_id("m2")).is_none());
        assert_eq!(state.item(&order::message_id("m1")).unwrap().turn_id, "");

        let mut second = commit(2);
        second.turn = Some(started_turn());
        second.attempt = Some(attempt("a1", AttemptOutcome::Running));
        second.consumed_messages = Some(1);
        let delta = state.apply(&second).unwrap();
        assert!(changed(&delta).contains(&order::message_id("m1")));
        assert_eq!(state.item(&order::message_id("m1")).unwrap().turn_id, "t1");
    }

    #[test]
    fn consuming_messages_without_a_saved_attempt_leaves_the_projection_unchanged() {
        use pl_core::thread::inbox::{InboxRecord, ThreadMessage};
        let mut state = ProjectionState::new("thread", Some("parent"));
        let mut first = commit(1);
        first.inbox = vec![InboxRecord {
            sequence: 1,
            message: ThreadMessage {
                id: "m1".into(),
                source_id: "agent:parent".into(),
                payload: OpaquePayload::text("task"),
                context: Vec::new(),
            },
        }]
        .into();
        state.apply(&first).unwrap();

        let mut broken = commit(2);
        broken.consumed_messages = Some(1);
        let error = state.apply(&broken).unwrap_err();
        assert!(matches!(error, ProjectionError::MissingMessage(id) if id == "m1"));
        assert_eq!(state.watermark(), 1);
        assert_eq!(state.item(&order::message_id("m1")).unwrap().turn_id, "");

        let mut expected = ProjectionState::new("thread", Some("parent"));
        expected.apply(&first).unwrap();
        assert_eq!(state.materialize(), expected.materialize());
    }

    #[test]
    fn restarting_from_persisted_slots_continues_without_replaying_the_prefix() {
        let journal: Vec<_> = {
            let mut first = commit(1);
            first.inputs = vec![accepted("input1", 1, prompt("hello", "visible"))].into();
            first.turn = Some(started_turn());
            let mut second = commit(2);
            second.attempt = Some(attempt("a1", AttemptOutcome::Running));
            let mut third = commit(3);
            third.attempt = Some(attempt(
                "a1",
                AttemptOutcome::Committed(output("a1", vec![text("answer")])),
            ));
            [first, second, third].into_iter().map(Arc::new).collect()
        };
        let full = ProjectionState::rebuild(
            "thread",
            None,
            &ThreadSnapshot {
                commit_sequence: 3,
                ..Default::default()
            },
            &journal,
        )
        .unwrap()
        .materialize();

        let mut resumed = ProjectionState::rebuild(
            "thread",
            None,
            &ThreadSnapshot {
                commit_sequence: 1,
                ..Default::default()
            },
            &journal[..1],
        )
        .unwrap();
        assert_eq!(resumed.watermark(), 1);
        assert_eq!(resumed.next_ordinal(), 2);
        for commit in &journal[1..] {
            resumed.apply(commit).unwrap();
        }
        assert_eq!(resumed.materialize(), full);
    }
}
