//! Keyed fact rows and the bounded recovery surface over them.
//!
//! Facts are stored one durable row per key (never a single `Facts` blob) with secondary index
//! columns, so [`TimelineReader::load_requirements`] can answer a projector's
//! [`ProjectionRequirements`] with exactly the rows a commit needs.
//!
//! Two recovery paths exist and must not be confused:
//! - **Bounded working set** (worker/browse resume): [`TimelineReader::working_set`] combines
//!   `read_head` + `read_panel` + `read_slots(keys)` (stable positions, hidden placeholders
//!   included) + `load_requirements`. A caller never loads history it will not touch.
//! - **Explicit cold activation** (initial owner creation): [`TimelineReader::restore_source`]
//!   loads the whole Thread's facts and slots for `ProjectionState::restore`. It is the only
//!   full-load path and must not be used as the worker/browse resume shortcut.

use super::TimelineStoreError;
use super::content as content_codec;
use super::read::{TimelineReader, statement, unsigned};
use super::schema::{
    self, TABLE_CONTENT, TABLE_CONTENT_META, TABLE_FACTS, TABLE_ITEMS, TABLE_PANEL, TABLE_SLOTS,
};
use crate::studio::thread_projection::engine::{
    Facts, PersistedSlot, ProjectionFactKey, ProjectionFactRow, ProjectionHead, ProjectionQuery,
    ProjectionRequirements,
};
use crate::studio::thread_projection::panel::PanelState;
use pl_protocol::ThreadItem;
use sea_orm::{ConnectionTrait, QueryResult, Value};
use std::collections::BTreeSet;

/// The exact durable rows a projector's [`ProjectionRequirements`] asked for.
#[derive(Debug, Clone, Default)]
pub(crate) struct LoadedFacts {
    pub rows: Vec<(ProjectionFactKey, ProjectionFactRow)>,
}

/// A bounded resume bundle for one commit: head, panel, the required stable positions and facts.
///
/// Consuming it needs a projector injection hook (the live `ProjectionState` cannot accept rows
/// yet); see the module-level contract report.
#[derive(Debug, Clone)]
pub(crate) struct TimelineWorkingSet {
    pub head: ProjectionHead,
    pub panel: PanelState,
    /// Stable positions of the requested keys, hidden placeholders (item `None`) included.
    pub slots: Vec<PersistedSlot>,
    pub facts: LoadedFacts,
}

/// Everything `ProjectionState::restore` needs, loaded from one Thread's durable index.
#[derive(Clone)]
pub(crate) struct RestoreSource {
    pub head: ProjectionHead,
    pub facts: Facts,
    pub panel: PanelState,
    pub slots: Vec<PersistedSlot>,
}

impl TimelineReader {
    /// The bounded working set one commit needs: head, panel, requested positions and fact rows.
    ///
    /// `slot_keys` are the stable slot identities the commit will read or reserve; the caller
    /// derives them from the commit (see the module contract report on the projector hook).
    ///
    /// # Errors
    /// Fails on an unknown Thread or a corrupt row.
    pub(crate) async fn working_set(
        &self,
        thread_id: &str,
        requirements: &ProjectionRequirements,
        slot_keys: &[String],
    ) -> Result<TimelineWorkingSet, TimelineStoreError> {
        Ok(TimelineWorkingSet {
            head: self.read_head(thread_id).await?,
            panel: self.read_panel(thread_id).await?,
            slots: self.read_slots(thread_id, slot_keys).await?,
            facts: self.load_requirements(thread_id, requirements).await?,
        })
    }

    /// Loads exactly the fact rows a projector's requirements name, using keyed rows and the
    /// secondary index columns; it never scans unrelated history.
    ///
    /// # Errors
    /// Fails on a corrupt fact row or an unsupported persisted fact family.
    pub(crate) async fn load_requirements(
        &self,
        thread_id: &str,
        requirements: &ProjectionRequirements,
    ) -> Result<LoadedFacts, TimelineStoreError> {
        let mut seen = BTreeSet::new();
        let mut rows = Vec::new();
        for key in &requirements.keys {
            if let Some(row) = self.fact_row(thread_id, key).await?
                && seen.insert(key.clone())
            {
                rows.push((key.clone(), row));
            }
        }
        for query in &requirements.queries {
            for (key, row) in self.answer_query(thread_id, query).await? {
                if seen.insert(key.clone()) {
                    rows.push((key, row));
                }
            }
        }
        Ok(LoadedFacts { rows })
    }

    /// The bounded runtime usage/panel summary of one Thread, or the default when none is stored.
    ///
    /// # Errors
    /// Fails when a stored summary cannot be decoded.
    pub(crate) async fn read_panel(
        &self,
        thread_id: &str,
    ) -> Result<PanelState, TimelineStoreError> {
        let sql = format!("SELECT panel FROM {TABLE_PANEL} WHERE thread_id=?");
        let row = self
            .db
            .query_one_raw(statement(&sql, vec![thread_id.into()]))
            .await?;
        match row {
            Some(row) => Ok(serde_json::from_str(&row.try_get::<String>("", "panel")?)?),
            None => Ok(PanelState::default()),
        }
    }

    /// Reads the stable positions of `keys` only, hidden placeholders included.
    ///
    /// # Errors
    /// Fails when a requested slot has no stable position or a stored item cannot be decoded.
    pub(crate) async fn read_slots(
        &self,
        thread_id: &str,
        keys: &[String],
    ) -> Result<Vec<PersistedSlot>, TimelineStoreError> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = vec!["?"; keys.len()].join(",");
        let sql = format!(
            "SELECT s.slot_key AS slot_key, s.ordinal AS ordinal, s.created_at AS created_at, i.kind AS kind, i.preview AS preview, i.preview_truncated AS preview_truncated, i.content_ref AS content_ref FROM {TABLE_SLOTS} s LEFT JOIN {TABLE_ITEMS} i ON i.thread_id = s.thread_id AND i.slot_key = s.slot_key AND i.generation_to IS NULL WHERE s.thread_id=? AND s.slot_key IN ({placeholders}) ORDER BY s.ordinal ASC"
        );
        let mut values: Vec<Value> = vec![thread_id.into()];
        values.extend(keys.iter().map(|key| Value::String(Some(key.clone()))));
        let rows = self.db.query_all_raw(statement(&sql, values)).await?;
        self.slots_from_rows(thread_id, rows).await
    }

    /// The stable slot ordinals of one Thread in ascending order, for index verification.
    ///
    /// # Errors
    /// Fails on a non-contiguous, negative or otherwise corrupt ordinal column.
    pub(crate) async fn slot_ordinals(
        &self,
        thread_id: &str,
    ) -> Result<Vec<u64>, TimelineStoreError> {
        let sql =
            format!("SELECT ordinal FROM {TABLE_SLOTS} WHERE thread_id=? ORDER BY ordinal ASC");
        let rows = self
            .db
            .query_all_raw(statement(&sql, vec![thread_id.into()]))
            .await?;
        rows.into_iter()
            .map(|row| unsigned(row.try_get::<i64>("", "ordinal")?))
            .collect()
    }

    /// Loads one Thread's durable head, facts, panel and current slots for `ProjectionState::restore`.
    ///
    /// This is the explicit cold-activation path and the only full-load entry; it must not back the
    /// write-behind worker or a browse resume, which use [`TimelineReader::working_set`].
    ///
    /// # Errors
    /// Fails when the Thread has no durable index or a persisted row cannot be decoded. `parent_id`
    /// mirrors `ProjectionState::new`, restoring the inbox message source for child Threads.
    pub(crate) async fn restore_source(
        &self,
        thread_id: &str,
        parent_id: Option<&str>,
    ) -> Result<RestoreSource, TimelineStoreError> {
        let head = self.head_row(thread_id).await?;
        let mut facts = self.load_all_facts(thread_id).await?;
        facts.message_source = parent_id.map(|parent| format!("agent:{parent}"));
        let panel = self.read_panel(thread_id).await?;
        let slots = self.all_slots(thread_id).await?;
        Ok(RestoreSource {
            head: ProjectionHead {
                thread_id: thread_id.to_owned(),
                watermark: head.watermark,
                next_ordinal: head.next_ordinal,
            },
            facts,
            panel,
            slots,
        })
    }

    async fn fact_row(
        &self,
        thread_id: &str,
        key: &ProjectionFactKey,
    ) -> Result<Option<ProjectionFactRow>, TimelineStoreError> {
        let sql = format!("SELECT row FROM {TABLE_FACTS} WHERE thread_id=? AND kind=? AND key=?");
        let row = self
            .db
            .query_one_raw(statement(
                &sql,
                vec![
                    thread_id.into(),
                    schema::fact_kind_label(key).into(),
                    schema::fact_key_string(key).into(),
                ],
            ))
            .await?;
        row.map(|row| row.try_get::<String>("", "row"))
            .transpose()?
            .map(|json| decode_fact_row(&json))
            .transpose()
    }

    async fn answer_query(
        &self,
        thread_id: &str,
        query: &ProjectionQuery,
    ) -> Result<Vec<(ProjectionFactKey, ProjectionFactRow)>, TimelineStoreError> {
        match query {
            ProjectionQuery::PendingMessages { through } => {
                self.rows_for(
                    thread_id,
                    "kind='message' AND seq<=? AND (aux_turn IS NULL OR aux_turn='') ORDER BY seq ASC",
                    vec![(*through as i64).into()],
                    ProjectionFactKey::Message,
                )
                .await
            }
            ProjectionQuery::TurnTasks { turn_id } => {
                self.rows_for(
                    thread_id,
                    "kind='task' AND aux_turn=?",
                    vec![turn_id.as_str().into()],
                    ProjectionFactKey::Task,
                )
                .await
            }
            ProjectionQuery::TurnAttempts { turn_id } => {
                self.rows_for(
                    thread_id,
                    "kind='attempt' AND aux_turn=? ORDER BY seq ASC",
                    vec![turn_id.as_str().into()],
                    ProjectionFactKey::Attempt,
                )
                .await
            }
            ProjectionQuery::PendingPermissions { call_id } => {
                self.rows_for(
                    thread_id,
                    "kind='permission' AND aux_call=?",
                    vec![call_id.as_str().into()],
                    ProjectionFactKey::Permission,
                )
                .await
            }
            ProjectionQuery::TurnsByInput { input_id } => {
                let mut rows = self
                    .rows_for(
                        thread_id,
                        "kind='turn' AND aux_turn=?",
                        vec![input_id.as_str().into()],
                        ProjectionFactKey::Turn,
                    )
                    .await?;
                // `latest_turn_for_input` also orders by each Turn's first sequence.
                let turn_ids: Vec<String> = rows
                    .iter()
                    .filter_map(|(key, _)| match key {
                        ProjectionFactKey::Turn(turn_id) => Some(turn_id.clone()),
                        _ => None,
                    })
                    .collect();
                for turn_id in turn_ids {
                    if let Some(row) = self
                        .fact_row(thread_id, &ProjectionFactKey::TurnFirst(turn_id.clone()))
                        .await?
                    {
                        rows.push((ProjectionFactKey::TurnFirst(turn_id), row));
                    }
                }
                Ok(rows)
            }
            ProjectionQuery::TaskByCall { call_id } => {
                let rows = self
                    .rows_for(
                        thread_id,
                        "kind='task' AND aux_call=?",
                        vec![call_id.as_str().into()],
                        ProjectionFactKey::Task,
                    )
                    .await?;
                let mut out = Vec::with_capacity(rows.len() * 2);
                for (key, row) in rows {
                    if let ProjectionFactKey::Task(task_id) = &key {
                        out.push((
                            ProjectionFactKey::TaskByCall(call_id.clone()),
                            ProjectionFactRow::TaskByCall(task_id.clone()),
                        ));
                    }
                    out.push((key, row));
                }
                Ok(out)
            }
        }
    }

    async fn rows_for(
        &self,
        thread_id: &str,
        predicate: &str,
        values: Vec<Value>,
        key_of: fn(String) -> ProjectionFactKey,
    ) -> Result<Vec<(ProjectionFactKey, ProjectionFactRow)>, TimelineStoreError> {
        let sql = format!("SELECT key, row FROM {TABLE_FACTS} WHERE thread_id=? AND {predicate}");
        let mut bound = vec![thread_id.into()];
        bound.extend(values);
        let rows = self.db.query_all_raw(statement(&sql, bound)).await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let key: String = row.try_get("", "key")?;
            let json: String = row.try_get("", "row")?;
            out.push((key_of(key), decode_fact_row(&json)?));
        }
        Ok(out)
    }

    async fn load_all_facts(&self, thread_id: &str) -> Result<Facts, TimelineStoreError> {
        let sql = format!("SELECT kind, key, row FROM {TABLE_FACTS} WHERE thread_id=?");
        let rows = self
            .db
            .query_all_raw(statement(&sql, vec![thread_id.into()]))
            .await?;
        let mut facts = Facts::default();
        for row in rows {
            let kind: String = row.try_get("", "kind")?;
            let key: String = row.try_get("", "key")?;
            let json: String = row.try_get("", "row")?;
            let key = schema::fact_key_from_parts(&kind, &key).ok_or_else(|| {
                TimelineStoreError::Corrupt(format!("unknown persisted fact family `{kind}`"))
            })?;
            facts.put(key, decode_fact_row(&json)?);
        }
        Ok(facts)
    }

    async fn all_slots(&self, thread_id: &str) -> Result<Vec<PersistedSlot>, TimelineStoreError> {
        let sql = format!(
            "SELECT s.slot_key AS slot_key, s.ordinal AS ordinal, s.created_at AS created_at, i.kind AS kind, i.preview AS preview, i.preview_truncated AS preview_truncated, i.content_ref AS content_ref FROM {TABLE_SLOTS} s LEFT JOIN {TABLE_ITEMS} i ON i.thread_id = s.thread_id AND i.slot_key = s.slot_key AND i.generation_to IS NULL WHERE s.thread_id=? ORDER BY s.ordinal ASC"
        );
        let rows = self
            .db
            .query_all_raw(statement(&sql, vec![thread_id.into()]))
            .await?;
        self.slots_from_rows(thread_id, rows).await
    }

    async fn slots_from_rows(
        &self,
        thread_id: &str,
        rows: Vec<QueryResult>,
    ) -> Result<Vec<PersistedSlot>, TimelineStoreError> {
        let mut slots = Vec::with_capacity(rows.len());
        for row in rows {
            let slot_key: String = row.try_get("", "slot_key")?;
            let ordinal = unsigned(row.try_get::<i64>("", "ordinal")?)?;
            let created_at: i64 = row.try_get("", "created_at")?;
            let kind: Option<String> = row.try_get("", "kind")?;
            let item = match kind {
                None => None,
                Some(label) => {
                    let kind = schema::slot_kind_from_label(&label).ok_or_else(|| {
                        TimelineStoreError::Corrupt(format!("unknown slot kind `{label}`"))
                    })?;
                    let preview: Option<String> = row.try_get("", "preview")?;
                    let truncated: i64 = row.try_get("", "preview_truncated")?;
                    let content_ref: Option<String> = row.try_get("", "content_ref")?;
                    let json = match (truncated != 0, content_ref) {
                        (true, Some(ref_id)) => self.reassemble_content(thread_id, &ref_id).await?,
                        _ => preview.ok_or_else(|| {
                            TimelineStoreError::Corrupt(format!(
                                "slot {slot_key} has no preview or content reference"
                            ))
                        })?,
                    };
                    Some((kind, serde_json::from_str::<ThreadItem>(&json)?))
                }
            };
            slots.push(PersistedSlot {
                key: slot_key,
                ordinal,
                created_at,
                kind: item.as_ref().map(|(kind, _)| *kind),
                item: item.map(|(_, item)| item),
            });
        }
        Ok(slots)
    }

    async fn reassemble_content(
        &self,
        thread_id: &str,
        ref_id: &str,
    ) -> Result<String, TimelineStoreError> {
        let meta_sql = format!(
            "SELECT digest, total_bytes FROM {TABLE_CONTENT_META} WHERE thread_id=? AND ref_id=?"
        );
        let meta = self
            .db
            .query_one_raw(statement(&meta_sql, vec![thread_id.into(), ref_id.into()]))
            .await?;
        let meta = meta.ok_or_else(|| TimelineStoreError::UnknownContentRef {
            ref_id: ref_id.to_owned(),
        })?;
        let digest: String = meta.try_get("", "digest")?;
        let total = unsigned(meta.try_get::<i64>("", "total_bytes")?)?;
        let chunk_sql = format!(
            "SELECT bytes FROM {TABLE_CONTENT} WHERE thread_id=? AND ref_id=? ORDER BY chunk ASC"
        );
        let rows = self
            .db
            .query_all_raw(statement(&chunk_sql, vec![thread_id.into(), ref_id.into()]))
            .await?;
        let mut bytes = Vec::with_capacity(total.min(1 << 24) as usize);
        for row in rows {
            let chunk: Vec<u8> = row.try_get("", "bytes")?;
            bytes.extend_from_slice(&chunk);
        }
        if bytes.len() as u64 != total || content_codec::digest_hex(&bytes) != digest {
            return Err(TimelineStoreError::Corrupt(format!(
                "timeline content {ref_id} failed integrity verification"
            )));
        }
        String::from_utf8(bytes).map_err(|_| {
            TimelineStoreError::Corrupt(format!("timeline content {ref_id} is not UTF-8"))
        })
    }
}

fn decode_fact_row(json: &str) -> Result<ProjectionFactRow, TimelineStoreError> {
    serde_json::from_str(json).map_err(|error| {
        TimelineStoreError::Corrupt(format!("fact row could not be decoded: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::timeline_store::fixture;
    use pl_core::thread::input::InputState;
    use pl_core::thread::{TurnOutcome, TurnRecord, TurnState};
    use pretty_assertions::assert_eq;
    use std::collections::BTreeSet;

    #[tokio::test]
    async fn requirements_load_exactly_the_named_rows_and_never_the_whole_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let mut first = fixture::commit(1);
        first.inputs = vec![fixture::accepted("input1", 1, "hello")].into();
        first.turn = Some(fixture::turn(Some("input1")));
        let mut second = fixture::commit(2);
        second.inputs = vec![fixture::accepted("input2", 2, "other")].into();
        let journal = [first, second];
        let state = fixture::indexed(&path, "thread", &journal).await;

        // The next commit touches only `input1` and Turn `t1`; it must never load `input2`.
        let mut third = fixture::commit(3);
        third.inputs = vec![pl_core::thread::input::InputChange::Transition {
            id: "input1".into(),
            revision: 2,
            state: InputState::Discarded,
        }]
        .into();
        third.turn = Some(TurnRecord {
            state: TurnState::Finished(TurnOutcome::Completed),
            model_steps: 1,
            ..fixture::turn(Some("input1"))
        });
        let requirements = state.requirements(&third);
        let reader = TimelineReader::open(&path).await.unwrap();
        let loaded = reader
            .load_requirements("thread", &requirements)
            .await
            .unwrap();
        let keys: BTreeSet<ProjectionFactKey> =
            loaded.rows.iter().map(|(key, _)| key.clone()).collect();
        assert!(keys.contains(&ProjectionFactKey::Input("input1".into())));
        assert!(keys.contains(&ProjectionFactKey::Turn("t1".into())));
        assert!(
            !keys.contains(&ProjectionFactKey::Input("input2".into())),
            "an unrelated fact must not be loaded"
        );

        let restored = reader.restore_source("thread", None).await.unwrap();
        let all: usize = restored.facts.rows().count();
        assert!(
            all > loaded.rows.len(),
            "whole table {all} vs selective {}",
            loaded.rows.len()
        );
        assert_eq!(restored.head.watermark, 2);
        reader.close().await.unwrap();
    }

    #[tokio::test]
    async fn bounded_working_set_reads_head_panel_slots_and_hidden_positions() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let mut journal = vec![{
            let mut first = fixture::commit(1);
            first.turn = Some(fixture::turn(None));
            first
        }];
        let mut running = fixture::commit(2);
        running.attempt = Some(fixture::running_attempt("a1"));
        journal.push(running);
        let state = fixture::indexed(&path, "thread", &journal).await;

        // A running attempt reserves a hidden reasoning placeholder with no durable item.
        let hidden = state
            .persisted_slots()
            .find(|slot| slot.item.is_none())
            .expect("a hidden placeholder slot exists");
        assert!(hidden.kind.is_none());
        let visible = state
            .persisted_slots()
            .find(|slot| slot.item.is_some())
            .expect("a visible slot exists");

        let reader = TimelineReader::open(&path).await.unwrap();
        let head = reader.read_head("thread").await.unwrap();
        assert_eq!(head.watermark, 2);
        let panel = reader.read_panel("thread").await.unwrap();
        assert_eq!(
            serde_json::to_value(&panel).unwrap(),
            serde_json::to_value(state.panel_state()).unwrap()
        );

        let slots = reader
            .read_slots("thread", &[hidden.key.clone(), visible.key.clone()])
            .await
            .unwrap();
        assert_eq!(slots.len(), 2, "only the requested positions are read");
        let hidden_row = slots.iter().find(|slot| slot.key == hidden.key).unwrap();
        assert!(
            hidden_row.item.is_none(),
            "hidden placeholder keeps its position"
        );
        assert_eq!(hidden_row.ordinal, hidden.ordinal);

        let present = reader
            .read_slots("thread", &[visible.key.clone()])
            .await
            .unwrap();
        assert_eq!(present.len(), 1);
        assert_eq!(present[0].item, visible.item);

        let empty = reader.read_slots("thread", &[]).await.unwrap();
        assert!(empty.is_empty());
        reader.close().await.unwrap();
    }
}
