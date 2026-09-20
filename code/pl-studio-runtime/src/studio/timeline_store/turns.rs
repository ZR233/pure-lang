//! Keyset Turn paging over the durable Turn metadata written by `persist_commit`.
//!
//! Turn rows live in `timeline_turns`; no new table is introduced and the projection is untouched.
//! Paging walks the stable Turn admission ordinal (`turn_ordinal`) by key, never by `OFFSET`, and
//! binds the same Thread/generation/watermark contract the item page uses. A read watermark also
//! bounds admitted Turns (`admitted_watermark <= read_watermark`), so a frozen cursor never picks up
//! a Turn admitted by a later commit.

use super::read::{HeadRow, TimelineReader, statement, unsigned};
use super::schema::{TABLE_TURNS, disposition_from_label};
use super::{TimelineBudget, TimelineStoreError, TimelineTurnMeta};
use base64::Engine as _;
use pl_protocol::Turn;
use sea_orm::{ConnectionTrait, Value};

/// A row counts at a read watermark when it was admitted by that watermark or its admission is
/// unknown (NULL): a Turn admitted by a later commit must never appear in a frozen page, while a
/// row without an admission watermark carries no Turn item and is filtered out elsewhere anyway.
const ADMITTED_FILTER: &str = "(admitted_watermark IS NULL OR admitted_watermark<=?)";

/// Internal keyset anchor for Turn pages, mirroring [`super::TimelinePageQuery`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TimelineTurnQuery {
    Latest,
    Before { cursor: String },
    After { cursor: String },
    Around { turn_id: String },
}

/// Opaque Turn cursor bound to one Thread, index generation, read watermark and last Turn sort key.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TurnCursor {
    pub version: u32,
    pub thread_id: String,
    pub generation: String,
    pub watermark: u64,
    pub ordinal: u64,
    pub turn_id: String,
}

impl TurnCursor {
    pub(super) fn encode(&self) -> Result<String, TimelineStoreError> {
        Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(self)?))
    }

    pub(super) fn decode(value: &str) -> Result<Self, TimelineStoreError> {
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(value.trim())
            .map_err(|error| TimelineStoreError::InvalidCursor(error.to_string()))?;
        let cursor: Self = serde_json::from_slice(&bytes)
            .map_err(|error| TimelineStoreError::InvalidCursor(error.to_string()))?;
        if cursor.version != 1 {
            return Err(TimelineStoreError::InvalidCursor(format!(
                "unsupported cursor version {}",
                cursor.version
            )));
        }
        Ok(cursor)
    }
}

/// One consistent Turn window plus its bidirectional cursors.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TurnWindow {
    pub thread_id: String,
    pub generation: String,
    pub read_watermark: u64,
    pub head_watermark: u64,
    pub turns: Vec<TimelineTurnMeta>,
    pub older_cursor: Option<String>,
    pub newer_cursor: Option<String>,
    pub byte_budget_exhausted: bool,
}

impl TimelineReader {
    /// Serves one keyset Turn page with the same item-count cap and byte budget as item pages.
    ///
    /// # Errors
    /// Fails on an unknown Thread, an invalid or stale cursor, an unknown `Around` Turn, or a
    /// corrupt row.
    pub(crate) async fn page_turns(
        &self,
        thread_id: &str,
        query: &TimelineTurnQuery,
        budget: &TimelineBudget,
    ) -> Result<TurnWindow, TimelineStoreError> {
        let head = self.head_row(thread_id).await?;
        let (read_watermark, below, above, from, descending) = match query {
            TimelineTurnQuery::Latest => (head.watermark, None, None, None, true),
            TimelineTurnQuery::Before { cursor } => {
                let cursor = TurnCursor::decode(cursor)?;
                self.validate_turn_cursor(thread_id, &head, &cursor)?;
                (cursor.watermark, Some(cursor.ordinal), None, None, true)
            }
            TimelineTurnQuery::After { cursor } => {
                let cursor = TurnCursor::decode(cursor)?;
                self.validate_turn_cursor(thread_id, &head, &cursor)?;
                (cursor.watermark, None, Some(cursor.ordinal), None, false)
            }
            TimelineTurnQuery::Around { turn_id } => {
                let position = self
                    .turn_ordinal_of(thread_id, turn_id, head.watermark)
                    .await?
                    .ok_or_else(|| TimelineStoreError::UnknownTurn {
                        turn_id: turn_id.clone(),
                    })?;
                let start = self
                    .turn_around_start(thread_id, head.watermark, position, budget.max_items)
                    .await?;
                (head.watermark, None, None, Some(start), false)
            }
        };
        let mut rows = self
            .fetch_turn_rows(
                thread_id,
                read_watermark,
                below,
                above,
                from,
                descending,
                budget.max_items,
            )
            .await?;
        if descending {
            rows.reverse();
        }
        let (turns, byte_budget_exhausted) = trim_turn_budget(rows, budget.max_bytes);

        let older_cursor = match turns.first() {
            Some(first)
                if self
                    .turn_exists(thread_id, read_watermark, Some(first.ordinal), None)
                    .await? =>
            {
                Some(self.turn_cursor(
                    thread_id,
                    &head,
                    read_watermark,
                    first.ordinal,
                    &first.turn.id,
                )?)
            }
            _ => None,
        };
        let newer_cursor = match turns.last() {
            Some(last)
                if self
                    .turn_exists(thread_id, read_watermark, None, Some(last.ordinal))
                    .await? =>
            {
                Some(self.turn_cursor(
                    thread_id,
                    &head,
                    read_watermark,
                    last.ordinal,
                    &last.turn.id,
                )?)
            }
            _ => None,
        };

        Ok(TurnWindow {
            thread_id: thread_id.to_owned(),
            generation: head.generation,
            read_watermark,
            head_watermark: head.watermark,
            turns,
            older_cursor,
            newer_cursor,
            byte_budget_exhausted,
        })
    }

    fn validate_turn_cursor(
        &self,
        thread_id: &str,
        head: &HeadRow,
        cursor: &TurnCursor,
    ) -> Result<(), TimelineStoreError> {
        if cursor.thread_id != thread_id {
            return Err(TimelineStoreError::InvalidCursor(format!(
                "cursor belongs to Thread {}",
                cursor.thread_id
            )));
        }
        if cursor.generation != head.generation {
            return Err(TimelineStoreError::StaleCursor(
                "the timeline index was rebuilt".into(),
            ));
        }
        if cursor.watermark > head.watermark {
            return Err(TimelineStoreError::StaleCursor(format!(
                "cursor watermark {} is ahead of the index watermark {}",
                cursor.watermark, head.watermark
            )));
        }
        if cursor.ordinal > i64::MAX as u64 {
            return Err(TimelineStoreError::InvalidCursor(format!(
                "cursor ordinal {} is out of range",
                cursor.ordinal
            )));
        }
        Ok(())
    }

    fn turn_cursor(
        &self,
        thread_id: &str,
        head: &HeadRow,
        watermark: u64,
        ordinal: u64,
        turn_id: &str,
    ) -> Result<String, TimelineStoreError> {
        TurnCursor {
            version: 1,
            thread_id: thread_id.to_owned(),
            generation: head.generation.clone(),
            watermark,
            ordinal,
            turn_id: turn_id.to_owned(),
        }
        .encode()
    }

    async fn turn_exists(
        &self,
        thread_id: &str,
        watermark: u64,
        below: Option<u64>,
        above: Option<u64>,
    ) -> Result<bool, TimelineStoreError> {
        let mut sql = format!(
            "SELECT 1 AS present FROM {TABLE_TURNS} WHERE thread_id=? AND turn_ordinal IS NOT NULL AND {ADMITTED_FILTER}"
        );
        let mut values: Vec<Value> = vec![thread_id.into(), (watermark as i64).into()];
        if let Some(below) = below {
            sql.push_str(" AND turn_ordinal < ?");
            values.push((below as i64).into());
        }
        if let Some(above) = above {
            sql.push_str(" AND turn_ordinal > ?");
            values.push((above as i64).into());
        }
        sql.push_str(" LIMIT 1");
        let row = self.db.query_one_raw(statement(&sql, values)).await?;
        Ok(row.is_some())
    }

    async fn turn_ordinal_of(
        &self,
        thread_id: &str,
        turn_id: &str,
        watermark: u64,
    ) -> Result<Option<u64>, TimelineStoreError> {
        let sql = format!(
            "SELECT turn_ordinal FROM {TABLE_TURNS} WHERE thread_id=? AND turn_id=? AND turn_ordinal IS NOT NULL AND {ADMITTED_FILTER} LIMIT 1"
        );
        let row = self
            .db
            .query_one_raw(statement(
                &sql,
                vec![
                    thread_id.into(),
                    turn_id.into(),
                    (watermark as i64).into(),
                ],
            ))
            .await?;
        row.map(|row| row.try_get::<i64>("", "turn_ordinal"))
            .transpose()?
            .map(unsigned)
            .transpose()
    }

    async fn turn_around_start(
        &self,
        thread_id: &str,
        watermark: u64,
        position: u64,
        max_items: usize,
    ) -> Result<u64, TimelineStoreError> {
        let half = (max_items / 2).max(1) as i64;
        let sql = format!(
            "SELECT turn_ordinal FROM {TABLE_TURNS} WHERE thread_id=? AND turn_ordinal IS NOT NULL AND {ADMITTED_FILTER} AND turn_ordinal < ? ORDER BY turn_ordinal DESC LIMIT ?"
        );
        let rows = self
            .db
            .query_all_raw(statement(
                &sql,
                vec![
                    thread_id.into(),
                    (watermark as i64).into(),
                    (position as i64).into(),
                    half.into(),
                ],
            ))
            .await?;
        match rows.last() {
            Some(row) => Ok(unsigned(row.try_get::<i64>("", "turn_ordinal")?)?),
            None => Ok(position),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn fetch_turn_rows(
        &self,
        thread_id: &str,
        watermark: u64,
        below: Option<u64>,
        above: Option<u64>,
        from: Option<u64>,
        descending: bool,
        limit: usize,
    ) -> Result<Vec<(TimelineTurnMeta, usize)>, TimelineStoreError> {
        let mut sql = format!(
            "SELECT turn_json, last_item_id, context_disposition, turn_ordinal FROM {TABLE_TURNS} WHERE thread_id=? AND turn_ordinal IS NOT NULL AND {ADMITTED_FILTER}"
        );
        let mut values: Vec<Value> = vec![thread_id.into(), (watermark as i64).into()];
        if let Some(below) = below {
            sql.push_str(" AND turn_ordinal < ?");
            values.push((below as i64).into());
        }
        if let Some(above) = above {
            sql.push_str(" AND turn_ordinal > ?");
            values.push((above as i64).into());
        }
        if let Some(from) = from {
            sql.push_str(" AND turn_ordinal >= ?");
            values.push((from as i64).into());
        }
        sql.push_str(if descending {
            " ORDER BY turn_ordinal DESC LIMIT ?"
        } else {
            " ORDER BY turn_ordinal ASC LIMIT ?"
        });
        values.push((limit as i64).into());
        let rows = self.db.query_all_raw(statement(&sql, values)).await?;
        rows.into_iter().map(turn_from_row).collect()
    }
}

fn turn_from_row(
    row: sea_orm::QueryResult,
) -> Result<(TimelineTurnMeta, usize), TimelineStoreError> {
    let turn_json: String = row.try_get("", "turn_json")?;
    let ordinal: i64 = row
        .try_get::<Option<i64>>("", "turn_ordinal")?
        .ok_or_else(|| TimelineStoreError::Corrupt("Turn row has no sort ordinal".into()))?;
    let label: String = row.try_get("", "context_disposition")?;
    let context_disposition = disposition_from_label(&label)
        .ok_or_else(|| TimelineStoreError::Corrupt(format!("unknown Turn disposition `{label}`")))?;
    let turn: Turn = serde_json::from_str(&turn_json)?;
    let cost = turn_json.len();
    Ok((
        TimelineTurnMeta {
            turn,
            last_item_id: row.try_get("", "last_item_id")?,
            context_disposition,
            ordinal: unsigned(ordinal)?,
        },
        cost,
    ))
}

fn trim_turn_budget(
    rows: Vec<(TimelineTurnMeta, usize)>,
    max_bytes: usize,
) -> (Vec<TimelineTurnMeta>, bool) {
    let mut used = 0usize;
    let mut kept = Vec::with_capacity(rows.len());
    let mut exhausted = false;
    for (turn, cost) in rows {
        if !kept.is_empty() && used + cost > max_bytes {
            exhausted = true;
            break;
        }
        used += cost;
        kept.push(turn);
    }
    (kept, exhausted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::thread_projection::engine::ProjectionState;
    use crate::studio::timeline_store::fixture;
    use crate::studio::timeline_store::{TimelineBudget, TimelineReader, build_index};
    use pl_protocol::ThreadItemState;
    use pretty_assertions::assert_eq;

    fn turn_journal(turns: u64) -> Vec<pl_core::thread::journal::ThreadCommit> {
        (1..=turns)
            .map(|index| {
                let mut commit = fixture::commit(index);
                commit.turn = Some(fixture::turn_named(&format!("t{index}"), None));
                commit
            })
            .collect()
    }

    fn expected_turn_ids(state: &ProjectionState) -> Vec<String> {
        state
            .materialize()
            .into_iter()
            .filter(|item| matches!(item.state(), ThreadItemState::Turn(_)))
            .map(|item| item.turn_id)
            .collect()
    }

    #[tokio::test]
    async fn turn_pages_walk_both_directions_without_gaps_or_repeats() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let journal = turn_journal(120);
        let state = fixture::indexed(&path, "thread", &journal).await;
        let expected = expected_turn_ids(&state);
        assert_eq!(expected.len(), 120);

        let reader = TimelineReader::open(&path).await.unwrap();
        let budget = TimelineBudget::default();
        let mut collected: Vec<String> = Vec::new();
        let mut query = TimelineTurnQuery::Latest;
        loop {
            let window = reader.page_turns("thread", &query, &budget).await.unwrap();
            assert!(window.turns.len() <= budget.max_items);
            assert_eq!(window.read_watermark, 120);
            assert!(!window.byte_budget_exhausted);
            let mut page: Vec<String> =
                window.turns.iter().map(|turn| turn.turn.id.clone()).collect();
            page.extend(collected);
            collected = page;
            match window.older_cursor {
                Some(cursor) => query = TimelineTurnQuery::Before { cursor },
                None => break,
            }
        }
        assert_eq!(collected, expected);

        let anchor = expected[40].clone();
        let first = reader
            .page_turns(
                "thread",
                &TimelineTurnQuery::Around {
                    turn_id: anchor.clone(),
                },
                &budget,
            )
            .await
            .unwrap();
        assert!(first.turns.iter().any(|turn| turn.turn.id == anchor));
        let mut collected: Vec<String> =
            first.turns.iter().map(|turn| turn.turn.id.clone()).collect();
        let mut cursor = first.newer_cursor;
        while let Some(value) = cursor {
            let window = reader
                .page_turns("thread", &TimelineTurnQuery::After { cursor: value }, &budget)
                .await
                .unwrap();
            collected.extend(window.turns.iter().map(|turn| turn.turn.id.clone()));
            cursor = window.newer_cursor;
        }
        assert_eq!(collected, expected);

        // A tiny byte budget ends the page before the item cap does.
        let tiny = TimelineBudget::new(100, 400).unwrap();
        let window = reader
            .page_turns("thread", &TimelineTurnQuery::Latest, &tiny)
            .await
            .unwrap();
        assert!(window.byte_budget_exhausted);
        assert!(window.turns.len() < budget.max_items);
        reader.close().await.unwrap();
    }

    #[tokio::test]
    async fn a_frozen_turn_cursor_is_not_affected_by_later_commits() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let mut journal = turn_journal(4);
        fixture::seed(&path, "thread", &journal).await;
        build_index(&path, "thread", None, &journal_arc(&journal))
            .await
            .unwrap();

        let reader = TimelineReader::open(&path).await.unwrap();
        let small = TimelineBudget::new(2, super::super::MAX_PAGE_BYTES).unwrap();
        let newest = reader
            .page_turns("thread", &TimelineTurnQuery::Latest, &small)
            .await
            .unwrap();
        assert_eq!(newest.read_watermark, 4);
        let ids: Vec<_> = newest.turns.iter().map(|turn| turn.turn.id.clone()).collect();
        assert_eq!(ids, vec!["t3".to_string(), "t4".to_string()]);
        let older = newest.older_cursor.clone().expect("older cursor exists");
        let generation = newest.generation.clone();
        reader.close().await.unwrap();

        // A later commit admits a new Turn under the same index generation.
        journal.push({
            let mut fifth = fixture::commit(5);
            fifth.turn = Some(fixture::turn_named("t5", None));
            fifth
        });
        fixture::seed(&path, "thread", &journal).await;
        build_index(&path, "thread", None, &journal_arc(&journal))
            .await
            .unwrap();

        let reader = TimelineReader::open(&path).await.unwrap();
        let frozen = reader
            .page_turns("thread", &TimelineTurnQuery::Before { cursor: older }, &small)
            .await
            .unwrap();
        assert_eq!(frozen.read_watermark, 4);
        let frozen_ids: Vec<_> = frozen.turns.iter().map(|turn| turn.turn.id.clone()).collect();
        assert_eq!(frozen_ids, vec!["t1".to_string(), "t2".to_string()]);
        assert!(!frozen_ids.contains(&"t5".to_string()));

        // The forward direction is bounded by the same frozen watermark.
        let t2 = frozen.turns.last().unwrap().ordinal;
        let after = TurnCursor {
            version: 1,
            thread_id: "thread".into(),
            generation,
            watermark: 4,
            ordinal: t2,
            turn_id: "t2".into(),
        }
        .encode()
        .unwrap();
        let forward = reader
            .page_turns("thread", &TimelineTurnQuery::After { cursor: after }, &small)
            .await
            .unwrap();
        let forward_ids: Vec<_> = forward.turns.iter().map(|turn| turn.turn.id.clone()).collect();
        assert_eq!(forward_ids, vec!["t3".to_string(), "t4".to_string()]);

        let head = reader
            .page_turns("thread", &TimelineTurnQuery::Latest, &TimelineBudget::default())
            .await
            .unwrap();
        assert!(head.turns.iter().any(|turn| turn.turn.id == "t5"));
        reader.close().await.unwrap();
    }

    #[tokio::test]
    async fn invalid_and_stale_turn_cursors_and_unknown_turns_are_typed_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let journal = turn_journal(3);
        fixture::indexed(&path, "thread", &journal).await;

        let reader = TimelineReader::open(&path).await.unwrap();
        let window = reader
            .page_turns("thread", &TimelineTurnQuery::Latest, &TimelineBudget::default())
            .await
            .unwrap();
        let ordinal = window.turns[0].ordinal;

        let other_thread = TurnCursor {
            version: 1,
            thread_id: "elsewhere".into(),
            generation: window.generation.clone(),
            watermark: 3,
            ordinal,
            turn_id: "t1".into(),
        }
        .encode()
        .unwrap();
        let error = reader
            .page_turns(
                "thread",
                &TimelineTurnQuery::Before { cursor: other_thread },
                &TimelineBudget::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, TimelineStoreError::InvalidCursor(_)));

        let stale = TurnCursor {
            version: 1,
            thread_id: "thread".into(),
            generation: "other-generation".into(),
            watermark: 3,
            ordinal,
            turn_id: "t1".into(),
        }
        .encode()
        .unwrap();
        let error = reader
            .page_turns(
                "thread",
                &TimelineTurnQuery::After { cursor: stale },
                &TimelineBudget::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, TimelineStoreError::StaleCursor(_)));

        let error = reader
            .page_turns(
                "thread",
                &TimelineTurnQuery::Around {
                    turn_id: "ghost".into(),
                },
                &TimelineBudget::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, TimelineStoreError::UnknownTurn { .. }));

        let error = reader
            .page_turns("other", &TimelineTurnQuery::Latest, &TimelineBudget::default())
            .await
            .unwrap_err();
        assert!(matches!(error, TimelineStoreError::ThreadNotIndexed { .. }));
        reader.close().await.unwrap();

        let missing = dir.path().join("absent.sqlite");
        assert!(matches!(
            TimelineReader::open(&missing).await.unwrap_err(),
            TimelineStoreError::MissingDatabase { .. }
        ));
    }

    fn journal_arc(
        commits: &[pl_core::thread::journal::ThreadCommit],
    ) -> Vec<std::sync::Arc<pl_core::thread::journal::ThreadCommit>> {
        commits.iter().cloned().map(std::sync::Arc::new).collect()
    }

    #[tokio::test]
    async fn a_turn_row_without_an_admission_watermark_lands_and_reads_correctly() {
        // A commit may persist only a saved rollback disposition, with no admission watermark to
        // attach. The row must still land (no NOT NULL failure, no fabricated watermark) and stay
        // out of Turn pages until a real admission, after which the real watermark governs.
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let mut first = fixture::commit(1);
        first.replacements = vec![fixture::rewind("t9")].into();
        fixture::indexed(&path, "thread", std::slice::from_ref(&first)).await;

        let reader = TimelineReader::open(&path).await.unwrap();
        let window = reader
            .page_turns("thread", &TimelineTurnQuery::Latest, &TimelineBudget::default())
            .await
            .unwrap();
        assert!(
            window.turns.is_empty(),
            "a rollback-only row is not a readable Turn"
        );
        reader.close().await.unwrap();

        let mut second = fixture::commit(2);
        second.turn = Some(fixture::turn_named("t9", None));
        let commits = vec![first, second];
        fixture::seed(&path, "thread", &commits).await;
        build_index(&path, "thread", None, &journal_arc(&commits))
            .await
            .unwrap();

        let reader = TimelineReader::open(&path).await.unwrap();
        let window = reader
            .page_turns("thread", &TimelineTurnQuery::Latest, &TimelineBudget::default())
            .await
            .unwrap();
        assert_eq!(window.turns.len(), 1);
        assert_eq!(window.turns[0].turn.id, "t9");

        // The admission watermark is the real commit 2, so a watermark-1 read excludes it.
        let cursor = TurnCursor {
            version: 1,
            thread_id: "thread".into(),
            generation: window.generation.clone(),
            watermark: 1,
            ordinal: i64::MAX as u64,
            turn_id: "t9".into(),
}
        .encode()
        .unwrap();
        let frozen = reader
            .page_turns(
                "thread",
                &TimelineTurnQuery::Before { cursor },
                &TimelineBudget::default(),
            )
            .await
            .unwrap();
        assert!(
            frozen.turns.is_empty(),
            "the real admission watermark must bound the frozen page"
        );
        reader.close().await.unwrap();
    }
}
