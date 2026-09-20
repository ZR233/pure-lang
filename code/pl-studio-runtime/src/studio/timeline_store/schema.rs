//! Studio-derived SQLite schema that lives beside the core session journal.
//!
//! Every table is prefixed `timeline_` so it can never collide with the core-owned
//! `session_entries`, `session_entry_history` or `session_history_heads` tables in the same file.
//! The core session schema version (`PRAGMA user_version`) keeps its own meaning; this index
//! records its own version inside `timeline_meta`.

use crate::studio::thread_projection::engine::{ProjectionFactKey, ProjectionFactRow, SlotKind};
use pl_protocol::ThreadContextDisposition;

/// Version of the Studio timeline index schema; a change must add a new version and a migration.
///
/// v2 adds the `timeline_turns` paging columns (`turn_ordinal`, `admitted_watermark`); older
/// indexes are upgraded additively by [`super::write::TimelineWriter::ensure_schema`].
pub(crate) const TIMELINE_SCHEMA_VERSION: i64 = 2;

/// `timeline_meta` key holding the Studio index schema version.
pub(crate) const META_SCHEMA_KEY: &str = "schema";

pub(super) const TABLE_META: &str = "timeline_meta";
pub(super) const TABLE_HEAD: &str = "timeline_head";
pub(super) const TABLE_SLOTS: &str = "timeline_slots";
pub(super) const TABLE_ITEMS: &str = "timeline_items";
pub(super) const TABLE_CONTENT: &str = "timeline_content";
pub(super) const TABLE_CONTENT_META: &str = "timeline_content_meta";
pub(super) const TABLE_FACTS: &str = "timeline_facts";
pub(super) const TABLE_TURNS: &str = "timeline_turns";
pub(super) const TABLE_PANEL: &str = "timeline_panel";

/// Additive columns introduced after v1, as `(table, column, definition)`.
///
/// `admitted_watermark` is nullable on purpose: a row can be created by a saved rollback
/// disposition before (or without) an admission watermark, and that "not yet admitted" state must
/// be representable explicitly rather than faked with a synthetic sequence.
pub(super) const TIMELINE_ADDITIVE_COLUMNS: [(&str, &str, &str); 2] = [
    (TABLE_TURNS, "turn_ordinal", "turn_ordinal INTEGER"),
    (TABLE_TURNS, "admitted_watermark", "admitted_watermark INTEGER"),
];

/// Full idempotent DDL for the Studio timeline index. Creating it never rewrites core rows.
pub(super) const TIMELINE_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS timeline_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS timeline_head (
    thread_id TEXT PRIMARY KEY,
    watermark INTEGER NOT NULL,
    next_ordinal INTEGER NOT NULL,
    generation TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS timeline_slots (
    thread_id TEXT NOT NULL,
    slot_key TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (thread_id, slot_key)
);
CREATE INDEX IF NOT EXISTS timeline_slots_order ON timeline_slots (thread_id, ordinal);
CREATE TABLE IF NOT EXISTS timeline_items (
    thread_id TEXT NOT NULL,
    slot_key TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    kind TEXT,
    generation_from INTEGER NOT NULL,
    generation_to INTEGER,
    revision INTEGER NOT NULL,
    item_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    turn_id TEXT NOT NULL,
    preview TEXT,
    preview_truncated INTEGER NOT NULL,
    content_ref TEXT,
    content_digest TEXT,
    content_total INTEGER,
    content_revision INTEGER,
    PRIMARY KEY (thread_id, slot_key, generation_from)
);
CREATE INDEX IF NOT EXISTS timeline_items_visible
    ON timeline_items (thread_id, generation_from, generation_to);
CREATE INDEX IF NOT EXISTS timeline_items_order
    ON timeline_items (thread_id, ordinal, generation_from);
CREATE TABLE IF NOT EXISTS timeline_content (
    thread_id TEXT NOT NULL,
    ref_id TEXT NOT NULL,
    chunk INTEGER NOT NULL,
    bytes BLOB NOT NULL,
    PRIMARY KEY (thread_id, ref_id, chunk)
);
CREATE TABLE IF NOT EXISTS timeline_content_meta (
    thread_id TEXT NOT NULL,
    ref_id TEXT NOT NULL,
    digest TEXT NOT NULL,
    total_bytes INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    item_id TEXT NOT NULL,
    slot_key TEXT NOT NULL,
    PRIMARY KEY (thread_id, ref_id)
);
CREATE TABLE IF NOT EXISTS timeline_facts (
    thread_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    key TEXT NOT NULL,
    seq INTEGER NOT NULL,
    aux_turn TEXT,
    aux_call TEXT,
    row TEXT NOT NULL,
    PRIMARY KEY (thread_id, kind, key)
);
CREATE INDEX IF NOT EXISTS timeline_facts_turn ON timeline_facts (thread_id, kind, aux_turn, seq);
CREATE INDEX IF NOT EXISTS timeline_facts_call ON timeline_facts (thread_id, kind, aux_call);
CREATE INDEX IF NOT EXISTS timeline_facts_seq ON timeline_facts (thread_id, kind, seq);
CREATE TABLE IF NOT EXISTS timeline_turns (
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    turn_json TEXT NOT NULL,
    last_ordinal INTEGER NOT NULL,
    last_item_id TEXT NOT NULL,
    context_disposition TEXT NOT NULL,
    turn_ordinal INTEGER,
    admitted_watermark INTEGER,
    PRIMARY KEY (thread_id, turn_id)
);
CREATE INDEX IF NOT EXISTS timeline_turns_order ON timeline_turns (thread_id, turn_ordinal);
CREATE TABLE IF NOT EXISTS timeline_panel (
    thread_id TEXT PRIMARY KEY,
    watermark INTEGER NOT NULL,
    panel TEXT NOT NULL
);
"#;

/// Stable text label for a slot category, persisted in `timeline_items.kind`.
pub(super) fn slot_kind_label(kind: SlotKind) -> &'static str {
    match kind {
        SlotKind::Input => "input",
        SlotKind::Message => "message",
        SlotKind::Turn => "turn",
        SlotKind::Compaction => "compaction",
        SlotKind::Skill => "skill",
        SlotKind::Completion => "completion",
        SlotKind::Inference => "inference",
        SlotKind::Reasoning => "reasoning",
        SlotKind::ResponseText => "responseText",
        SlotKind::Tool => "tool",
    }
}

/// Parses a persisted slot label; an unknown label is a corruption, never a silent fallback.
pub(super) fn slot_kind_from_label(label: &str) -> Option<SlotKind> {
    Some(match label {
        "input" => SlotKind::Input,
        "message" => SlotKind::Message,
        "turn" => SlotKind::Turn,
        "compaction" => SlotKind::Compaction,
        "skill" => SlotKind::Skill,
        "completion" => SlotKind::Completion,
        "inference" => SlotKind::Inference,
        "reasoning" => SlotKind::Reasoning,
        "responseText" => SlotKind::ResponseText,
        "tool" => SlotKind::Tool,
        _ => return None,
    })
}

/// Persisted label for a Turn's context disposition.
pub(super) fn disposition_label(disposition: ThreadContextDisposition) -> &'static str {
    match disposition {
        ThreadContextDisposition::Active => "active",
        ThreadContextDisposition::RolledBack => "rolledBack",
    }
}

/// Parses a persisted disposition label; an unknown label is a corruption.
pub(super) fn disposition_from_label(label: &str) -> Option<ThreadContextDisposition> {
    Some(match label {
        "active" => ThreadContextDisposition::Active,
        "rolledBack" => ThreadContextDisposition::RolledBack,
        _ => return None,
    })
}

/// Stable text label for a fact row family, used as part of `timeline_facts` primary key.
pub(super) fn fact_kind_label(key: &ProjectionFactKey) -> &'static str {
    match key {
        ProjectionFactKey::Input(_) => "input",
        ProjectionFactKey::Message(_) => "message",
        ProjectionFactKey::Attempt(_) => "attempt",
        ProjectionFactKey::Call(_) => "call",
        ProjectionFactKey::CallUpdate(_) => "callUpdate",
        ProjectionFactKey::CallTurn(_) => "callTurn",
        ProjectionFactKey::Task(_) => "task",
        ProjectionFactKey::TaskByCall(_) => "taskByCall",
        ProjectionFactKey::Delivery(_) => "delivery",
        ProjectionFactKey::Permission(_) => "permission",
        ProjectionFactKey::TurnStamp(_) => "turnStamp",
        ProjectionFactKey::TurnFirst(_) => "turnFirst",
        ProjectionFactKey::Turn(_) => "turn",
        ProjectionFactKey::Compaction(_) => "compaction",
        ProjectionFactKey::RolledBack(_) => "rolledBack",
        ProjectionFactKey::SkillView(_) => "skillView",
    }
}

/// The inner identity of a fact key, used as the second part of the primary key.
pub(super) fn fact_key_string(key: &ProjectionFactKey) -> &str {
    match key {
        ProjectionFactKey::Input(key)
        | ProjectionFactKey::Message(key)
        | ProjectionFactKey::Attempt(key)
        | ProjectionFactKey::Call(key)
        | ProjectionFactKey::CallUpdate(key)
        | ProjectionFactKey::CallTurn(key)
        | ProjectionFactKey::Task(key)
        | ProjectionFactKey::TaskByCall(key)
        | ProjectionFactKey::Delivery(key)
        | ProjectionFactKey::Permission(key)
        | ProjectionFactKey::TurnStamp(key)
        | ProjectionFactKey::TurnFirst(key)
        | ProjectionFactKey::Turn(key)
        | ProjectionFactKey::Compaction(key)
        | ProjectionFactKey::RolledBack(key)
        | ProjectionFactKey::SkillView(key) => key,
    }
}

/// Rebuilds a fact key from its persisted `(kind, key)` pair, or `None` for an unknown family.
pub(super) fn fact_key_from_parts(kind: &str, key: &str) -> Option<ProjectionFactKey> {
    Some(match kind {
        "input" => ProjectionFactKey::Input(key.to_owned()),
        "message" => ProjectionFactKey::Message(key.to_owned()),
        "attempt" => ProjectionFactKey::Attempt(key.to_owned()),
        "call" => ProjectionFactKey::Call(key.to_owned()),
        "callUpdate" => ProjectionFactKey::CallUpdate(key.to_owned()),
        "callTurn" => ProjectionFactKey::CallTurn(key.to_owned()),
        "task" => ProjectionFactKey::Task(key.to_owned()),
        "taskByCall" => ProjectionFactKey::TaskByCall(key.to_owned()),
        "delivery" => ProjectionFactKey::Delivery(key.to_owned()),
        "permission" => ProjectionFactKey::Permission(key.to_owned()),
        "turnStamp" => ProjectionFactKey::TurnStamp(key.to_owned()),
        "turnFirst" => ProjectionFactKey::TurnFirst(key.to_owned()),
        "turn" => ProjectionFactKey::Turn(key.to_owned()),
        "compaction" => ProjectionFactKey::Compaction(key.to_owned()),
        "rolledBack" => ProjectionFactKey::RolledBack(key.to_owned()),
        "skillView" => ProjectionFactKey::SkillView(key.to_owned()),
        _ => return None,
    })
}

/// Secondary index columns for one fact row: `(seq, aux_turn, aux_call)`.
///
/// The meaning is per-family but stable: `aux_turn` groups Task/Attempt rows of one Turn and Turn
/// rows of one input; `aux_call` groups Permission/Task/Delivery rows of one call.
pub(super) fn fact_index_columns(
    key: &ProjectionFactKey,
    row: &ProjectionFactRow,
) -> (i64, Option<String>, Option<String>) {
    let call_id = || Some(fact_key_string(key).to_owned());
    match (key, row) {
        (ProjectionFactKey::Message(_), ProjectionFactRow::Message(facts)) => {
            (facts.sequence as i64, Some(facts.turn_id.clone()), None)
        }
        (ProjectionFactKey::Attempt(_), ProjectionFactRow::Attempt(facts)) => {
            (facts.seq as i64, Some(facts.record.turn_id.clone()), None)
        }
        (ProjectionFactKey::Call(_), ProjectionFactRow::Call(facts)) => (
            facts.sequence as i64,
            Some(facts.turn_id.clone()),
            call_id(),
        ),
        (ProjectionFactKey::CallUpdate(_), ProjectionFactRow::CallUpdate(sequence, _)) => {
            (*sequence as i64, None, call_id())
        }
        (ProjectionFactKey::CallTurn(_), ProjectionFactRow::CallTurn(turn_id)) => {
            (0, Some(turn_id.clone()), call_id())
        }
        (ProjectionFactKey::Task(_), ProjectionFactRow::Task(task)) => {
            (0, Some(task.turn_id.clone()), Some(task.call_id.clone()))
        }
        (ProjectionFactKey::TaskByCall(_), ProjectionFactRow::TaskByCall(_)) => {
            (0, None, call_id())
        }
        (ProjectionFactKey::Delivery(_), ProjectionFactRow::Delivery(facts)) => {
            (facts.sequence as i64, None, call_id())
        }
        (ProjectionFactKey::Permission(_), ProjectionFactRow::Permission(permission)) => {
            (0, None, Some(permission.call_id.clone()))
        }
        (ProjectionFactKey::TurnFirst(_), ProjectionFactRow::TurnFirst(sequence)) => (
            *sequence as i64,
            Some(fact_key_string(key).to_owned()),
            None,
        ),
        (ProjectionFactKey::TurnStamp(_), ProjectionFactRow::TurnStamp(_)) => {
            (0, Some(fact_key_string(key).to_owned()), None)
        }
        (ProjectionFactKey::Turn(_), ProjectionFactRow::Turn(turn)) => {
            (0, turn.input_id.clone(), None)
        }
        _ => (0, None, None),
    }
}
