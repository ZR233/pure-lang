//! Keyset timeline paging: bounded windows, opaque cursors and related Turn metadata.
//!
//! Pages never use an `OFFSET` scan and never `SELECT` every item; a page walks the
//! `timeline_items` union of stable slot ordinals by key and reads only bounded preview columns.
//! Oversized items are represented by a [`TimelinePreview`] fragment plus a [`ContentRef`], so a
//! caller can never mistake a truncated display payload for a complete decodable DTO.

use super::content::ContentRef;
use super::read::{HeadRow, TimelineReader, statement, unsigned};
use super::schema::{TABLE_ITEMS, TABLE_TURNS, disposition_from_label, slot_kind_from_label};
use super::{
    DEFAULT_PAGE_BYTES, DEFAULT_PAGE_ITEMS, MAX_PAGE_BYTES, MAX_PAGE_ITEMS, TimelineStoreError,
};
use crate::studio::thread_projection::engine::SlotKind;
use base64::Engine as _;
use pl_protocol::{ThreadContextDisposition, ThreadItem, Turn};
use sea_orm::{ConnectionTrait, Value};
use std::collections::BTreeSet;

const ITEM_COLUMNS: &str = "slot_key, item_id, ordinal, kind, revision, created_at, updated_at, turn_id, preview, preview_truncated, content_ref, content_digest, content_total, content_revision";

/// Bounded page request: at most [`MAX_PAGE_ITEMS`] items and [`MAX_PAGE_BYTES`] preview bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimelineBudget {
    pub max_items: usize,
    pub max_bytes: usize,
}

impl Default for TimelineBudget {
    fn default() -> Self {
        Self {
            max_items: DEFAULT_PAGE_ITEMS,
            max_bytes: DEFAULT_PAGE_BYTES,
        }
    }
}

impl TimelineBudget {
    /// Validates an explicit budget against the hard page limits.
    ///
    /// # Errors
    /// Rejects an out-of-range item count or payload budget.
    pub(crate) fn new(max_items: usize, max_bytes: usize) -> Result<Self, TimelineStoreError> {
        if max_items == 0 || max_items > MAX_PAGE_ITEMS {
            return Err(TimelineStoreError::InvalidRequest(format!(
                "timeline page item limit {max_items} must be 1..={MAX_PAGE_ITEMS}"
            )));
        }
        if max_bytes == 0 || max_bytes > MAX_PAGE_BYTES {
            return Err(TimelineStoreError::InvalidRequest(format!(
                "timeline page byte budget {max_bytes} must be 1..={MAX_PAGE_BYTES}"
            )));
        }
        Ok(Self {
            max_items,
            max_bytes,
        })
    }
}

/// Internal keyset anchor. `Before`/`After` carry an opaque [`TimelineCursor`]; `Around` locates a
/// raw item id so the later protocol adapter can keep its existing `itemId` semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TimelinePageQuery {
    Latest,
    Before { cursor: String },
    After { cursor: String },
    Around { item_id: String },
}

/// Opaque keyset cursor bound to one Thread, index generation, read watermark and sort boundary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TimelineCursor {
    pub version: u32,
    pub thread_id: String,
    pub generation: String,
    pub watermark: u64,
    pub ordinal: u64,
    pub slot_key: String,
}

impl TimelineCursor {
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

/// Bounded preview of one item's display payload.
///
/// `Complete` holds the whole display JSON and decodes to a `ThreadItem`. `Truncated` holds a
/// UTF-8 fragment of an oversized display JSON and **must never be parsed as a complete DTO**; the
/// full payload is reached only through the sibling [`ContentRef`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TimelinePreview {
    Complete(String),
    Truncated(String),
}

impl TimelinePreview {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Complete(text) | Self::Truncated(text) => text,
        }
    }

    pub(crate) fn is_complete(&self) -> bool {
        matches!(self, Self::Complete(_))
    }

    pub(crate) fn byte_len(&self) -> usize {
        self.as_str().len()
    }

    /// Decodes the preview into a full `ThreadItem`, but only for a `Complete` preview.
    pub(crate) fn decode_item(&self) -> Option<ThreadItem> {
        match self {
            Self::Complete(text) => serde_json::from_str(text).ok(),
            Self::Truncated(_) => None,
        }
    }
}

/// One display item at the page's frozen watermark; oversized bodies carry a [`ContentRef`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TimelineEntry {
    pub slot_key: String,
    pub item_id: String,
    pub ordinal: u64,
    pub kind: SlotKind,
    pub revision: u64,
    pub turn_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// Metadata above plus this bounded preview; never a partial DTO pretending to be complete.
    pub preview: TimelinePreview,
    pub content: Option<ContentRef>,
}

/// Turn metadata related to a page, independent of whether the admission item is on the page.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TimelineTurnMeta {
    pub turn: Turn,
    pub last_item_id: String,
    pub context_disposition: ThreadContextDisposition,
    /// Stable admission ordinal of the Turn slot; the keyset sort key for Turn paging.
    pub ordinal: u64,
}

/// One consistent item window plus its related Turn metadata and bidirectional cursors.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TimelineWindow {
    pub thread_id: String,
    pub generation: String,
    pub read_watermark: u64,
    pub head_watermark: u64,
    pub entries: Vec<TimelineEntry>,
    pub turns: Vec<TimelineTurnMeta>,
    pub older_cursor: Option<String>,
    pub newer_cursor: Option<String>,
    pub byte_budget_exhausted: bool,
}

impl TimelineReader {
    /// Serves one keyset page without an `OFFSET` scan or a full-item decode.
    ///
    /// # Errors
    /// Fails on an unknown Thread, an invalid or stale cursor, an unknown `Around` item, or a
    /// corrupt index row.
    pub(crate) async fn page(
        &self,
        thread_id: &str,
        query: &TimelinePageQuery,
        budget: &TimelineBudget,
    ) -> Result<TimelineWindow, TimelineStoreError> {
        let head = self.head_row(thread_id).await?;
        let (read_watermark, below, above, from, descending) = match query {
            TimelinePageQuery::Latest => (head.watermark, None, None, None, true),
            TimelinePageQuery::Before { cursor } => {
                let cursor = TimelineCursor::decode(cursor)?;
                self.validate_cursor(thread_id, &head, &cursor)?;
                (cursor.watermark, Some(cursor.ordinal), None, None, true)
            }
            TimelinePageQuery::After { cursor } => {
                let cursor = TimelineCursor::decode(cursor)?;
                self.validate_cursor(thread_id, &head, &cursor)?;
                (cursor.watermark, None, Some(cursor.ordinal), None, false)
            }
            TimelinePageQuery::Around { item_id } => {
                let position = self
                    .item_ordinal(thread_id, item_id, head.watermark)
                    .await?
                    .ok_or_else(|| TimelineStoreError::UnknownItem {
                        item_id: item_id.clone(),
                    })?;
                let start = self
                    .around_start(thread_id, head.watermark, position, budget.max_items)
                    .await?;
                (head.watermark, None, None, Some(start), false)
            }
        };
        let mut entries = self
            .fetch_items(
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
            entries.reverse();
        }
        let (entries, byte_budget_exhausted) = trim_to_budget(entries, budget.max_bytes);

        let older_cursor = match entries.first() {
            Some(first)
                if self
                    .items_exist(thread_id, read_watermark, Some(first.ordinal), None)
                    .await? =>
            {
                Some(self.cursor(
                    thread_id,
                    &head,
                    read_watermark,
                    first.ordinal,
                    &first.slot_key,
                )?)
            }
            _ => None,
        };
        let newer_cursor = match entries.last() {
            Some(last)
                if self
                    .items_exist(thread_id, read_watermark, None, Some(last.ordinal))
                    .await? =>
            {
                Some(self.cursor(
                    thread_id,
                    &head,
                    read_watermark,
                    last.ordinal,
                    &last.slot_key,
                )?)
            }
            _ => None,
        };
        let turn_ids: Vec<String> = entries
            .iter()
            .filter(|entry| !entry.turn_id.is_empty() && entry.kind != SlotKind::Compaction)
            .map(|entry| entry.turn_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let turns = self.load_turns(thread_id, &turn_ids).await?;

        Ok(TimelineWindow {
            thread_id: thread_id.to_owned(),
            generation: head.generation,
            read_watermark,
            head_watermark: head.watermark,
            entries,
            turns,
            older_cursor,
            newer_cursor,
            byte_budget_exhausted,
        })
    }

    fn validate_cursor(
        &self,
        thread_id: &str,
        head: &HeadRow,
        cursor: &TimelineCursor,
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

    fn cursor(
        &self,
        thread_id: &str,
        head: &HeadRow,
        watermark: u64,
        ordinal: u64,
        slot_key: &str,
    ) -> Result<String, TimelineStoreError> {
        TimelineCursor {
            version: 1,
            thread_id: thread_id.to_owned(),
            generation: head.generation.clone(),
            watermark,
            ordinal,
            slot_key: slot_key.to_owned(),
        }
        .encode()
    }

    async fn items_exist(
        &self,
        thread_id: &str,
        watermark: u64,
        below: Option<u64>,
        above: Option<u64>,
    ) -> Result<bool, TimelineStoreError> {
        let mut sql = format!(
            "SELECT 1 AS present FROM {TABLE_ITEMS} WHERE thread_id=? AND kind IS NOT NULL AND generation_from<=? AND (generation_to IS NULL OR generation_to>?)"
        );
        let mut values: Vec<Value> = vec![
            thread_id.into(),
            (watermark as i64).into(),
            (watermark as i64).into(),
        ];
        if let Some(below) = below {
            sql.push_str(" AND ordinal < ?");
            values.push((below as i64).into());
        }
        if let Some(above) = above {
            sql.push_str(" AND ordinal > ?");
            values.push((above as i64).into());
        }
        sql.push_str(" LIMIT 1");
        let row = self.db.query_one_raw(statement(&sql, values)).await?;
        Ok(row.is_some())
    }

    async fn item_ordinal(
        &self,
        thread_id: &str,
        item_id: &str,
        watermark: u64,
    ) -> Result<Option<u64>, TimelineStoreError> {
        let sql = format!(
            "SELECT ordinal FROM {TABLE_ITEMS} WHERE thread_id=? AND item_id=? AND kind IS NOT NULL AND generation_from<=? AND (generation_to IS NULL OR generation_to>?) ORDER BY ordinal ASC LIMIT 1"
        );
        let row = self
            .db
            .query_one_raw(statement(
                &sql,
                vec![
                    thread_id.into(),
                    item_id.into(),
                    (watermark as i64).into(),
                    (watermark as i64).into(),
                ],
            ))
            .await?;
        row.map(|row| row.try_get::<i64>("", "ordinal"))
            .transpose()?
            .map(unsigned)
            .transpose()
    }

    async fn around_start(
        &self,
        thread_id: &str,
        watermark: u64,
        position: u64,
        max_items: usize,
    ) -> Result<u64, TimelineStoreError> {
        let half = (max_items / 2).max(1) as i64;
        let sql = format!(
            "SELECT ordinal FROM {TABLE_ITEMS} WHERE thread_id=? AND kind IS NOT NULL AND ordinal < ? AND generation_from<=? AND (generation_to IS NULL OR generation_to>?) ORDER BY ordinal DESC LIMIT ?"
        );
        let rows = self
            .db
            .query_all_raw(statement(
                &sql,
                vec![
                    thread_id.into(),
                    (position as i64).into(),
                    (watermark as i64).into(),
                    (watermark as i64).into(),
                    half.into(),
                ],
            ))
            .await?;
        match rows.last() {
            Some(row) => Ok(unsigned(row.try_get::<i64>("", "ordinal")?)?),
            None => Ok(position),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn fetch_items(
        &self,
        thread_id: &str,
        watermark: u64,
        below: Option<u64>,
        above: Option<u64>,
        from: Option<u64>,
        descending: bool,
        limit: usize,
    ) -> Result<Vec<TimelineEntry>, TimelineStoreError> {
        let mut sql = format!(
            "SELECT {ITEM_COLUMNS} FROM {TABLE_ITEMS} WHERE thread_id=? AND kind IS NOT NULL AND generation_from<=? AND (generation_to IS NULL OR generation_to>?)"
        );
        let mut values: Vec<Value> = vec![
            thread_id.into(),
            (watermark as i64).into(),
            (watermark as i64).into(),
        ];
        if let Some(below) = below {
            sql.push_str(" AND ordinal < ?");
            values.push((below as i64).into());
        }
        if let Some(above) = above {
            sql.push_str(" AND ordinal > ?");
            values.push((above as i64).into());
        }
        if let Some(from) = from {
            sql.push_str(" AND ordinal >= ?");
            values.push((from as i64).into());
        }
        sql.push_str(if descending {
            " ORDER BY ordinal DESC LIMIT ?"
        } else {
            " ORDER BY ordinal ASC LIMIT ?"
        });
        values.push((limit as i64).into());
        let rows = self.db.query_all_raw(statement(&sql, values)).await?;
        rows.into_iter().map(entry_from_row).collect()
    }

    async fn load_turns(
        &self,
        thread_id: &str,
        turn_ids: &[String],
    ) -> Result<Vec<TimelineTurnMeta>, TimelineStoreError> {
        let sql = format!(
            "SELECT turn_json, last_item_id, context_disposition, turn_ordinal FROM {TABLE_TURNS} WHERE thread_id=? AND turn_id=?"
        );
        let mut turns = Vec::new();
        for turn_id in turn_ids {
            let row = self
                .db
                .query_one_raw(statement(
                    &sql,
                    vec![thread_id.into(), turn_id.as_str().into()],
                ))
                .await?;
            let Some(row) = row else { continue };
            let turn_json: String = row.try_get("", "turn_json")?;
            let ordinal: Option<i64> = row.try_get("", "turn_ordinal")?;
            // A row without a Turn item yet has neither display json nor a sort ordinal.
            let (Some(ordinal), false) = (ordinal, turn_json.is_empty()) else {
                continue;
            };
            let label: String = row.try_get("", "context_disposition")?;
            let context_disposition = disposition_from_label(&label).ok_or_else(|| {
                TimelineStoreError::Corrupt(format!("unknown Turn disposition `{label}`"))
            })?;
            turns.push(TimelineTurnMeta {
                turn: serde_json::from_str(&turn_json)?,
                last_item_id: row.try_get("", "last_item_id")?,
                context_disposition,
                ordinal: unsigned(ordinal)?,
            });
        }
        Ok(turns)
    }
}

fn entry_from_row(row: sea_orm::QueryResult) -> Result<TimelineEntry, TimelineStoreError> {
    let label: String = row.try_get("", "kind")?;
    let kind = slot_kind_from_label(&label)
        .ok_or_else(|| TimelineStoreError::Corrupt(format!("unknown slot kind `{label}`")))?;
    let preview: String = row.try_get("", "preview")?;
    let truncated = row.try_get::<i64>("", "preview_truncated")? != 0;
    let content_ref: Option<String> = row.try_get("", "content_ref")?;
    let content = match content_ref {
        Some(ref_id) => Some(ContentRef {
            ref_id,
            digest: row
                .try_get::<Option<String>>("", "content_digest")?
                .unwrap_or_default(),
            total_bytes: match row.try_get::<Option<i64>>("", "content_total")? {
                Some(total) => unsigned(total)?,
                None => 0,
            },
            revision: match row.try_get::<Option<i64>>("", "content_revision")? {
                Some(revision) => unsigned(revision)?,
                None => 0,
            },
        }),
        None => None,
    };
    let preview = if truncated {
        TimelinePreview::Truncated(preview)
    } else {
        TimelinePreview::Complete(preview)
    };
    Ok(TimelineEntry {
        slot_key: row.try_get("", "slot_key")?,
        item_id: row.try_get("", "item_id")?,
        ordinal: unsigned(row.try_get::<i64>("", "ordinal")?)?,
        kind,
        revision: unsigned(row.try_get::<i64>("", "revision")?)?,
        turn_id: row.try_get("", "turn_id")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
        preview,
        content,
    })
}

fn trim_to_budget(entries: Vec<TimelineEntry>, max_bytes: usize) -> (Vec<TimelineEntry>, bool) {
    let mut used = 0usize;
    let mut kept = Vec::with_capacity(entries.len());
    let mut exhausted = false;
    for entry in entries {
        let cost = entry.preview.byte_len();
        if !kept.is_empty() && used + cost > max_bytes {
            exhausted = true;
            break;
        }
        used += cost;
        kept.push(entry);
    }
    (kept, exhausted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::timeline_store::content::PREVIEW_BYTES;
    use crate::studio::timeline_store::fixture;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn pages_walk_a_large_turn_without_gaps_or_repeats() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let mut journal = vec![{
            let mut first = fixture::commit(1);
            first.turn = Some(fixture::turn(None));
            first
        }];
        for index in 0..60u64 {
            let mut step = fixture::commit(index + 2);
            step.attempt = Some(fixture::committed(
                &format!("a{index}"),
                &format!("answer {index}"),
            ));
            journal.push(step);
        }
        let state = fixture::indexed(&path, "thread", &journal).await;
        let expected: Vec<String> = state
            .materialize()
            .into_iter()
            .map(|item| item.id)
            .collect();
        assert!(
            expected.len() > 100,
            "fixture must span pages: {}",
            expected.len()
        );

        let reader = TimelineReader::open(&path).await.unwrap();
        let budget = TimelineBudget::default();
        let mut collected: Vec<String> = Vec::new();
        let mut query = TimelinePageQuery::Latest;
        loop {
            let window = reader.page("thread", &query, &budget).await.unwrap();
            assert!(window.entries.len() <= budget.max_items);
            assert!(
                !window.turns.is_empty(),
                "related Turn metadata is returned"
            );
            assert_eq!(window.turns[0].turn.id, "t1");
            let mut page: Vec<String> = window.entries.iter().map(|e| e.item_id.clone()).collect();
            page.extend(collected);
            collected = page;
            match window.older_cursor {
                Some(cursor) => query = TimelinePageQuery::Before { cursor },
                None => break,
            }
        }
        assert_eq!(collected, expected);

        let anchor = expected[40].clone();
        let first = reader
            .page(
                "thread",
                &TimelinePageQuery::Around {
                    item_id: anchor.clone(),
                },
                &budget,
            )
            .await
            .unwrap();
        assert!(first.entries.iter().any(|entry| entry.item_id == anchor));
        let mut collected: Vec<String> = first.entries.iter().map(|e| e.item_id.clone()).collect();
        let mut cursor = first.newer_cursor;
        while let Some(cursor_value) = cursor {
            let window = reader
                .page(
                    "thread",
                    &TimelinePageQuery::After {
                        cursor: cursor_value,
                    },
                    &budget,
                )
                .await
                .unwrap();
            collected.extend(window.entries.iter().map(|e| e.item_id.clone()));
            cursor = window.newer_cursor;
        }
        assert_eq!(collected, expected);
        reader.close().await.unwrap();
    }

    #[tokio::test]
    async fn page_byte_budget_bounds_one_page_and_every_preview() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let large = "内容".repeat(8_000);
        let mut journal = vec![{
            let mut first = fixture::commit(1);
            first.turn = Some(fixture::turn(None));
            first
        }];
        for index in 0..40u64 {
            let mut step = fixture::commit(index + 2);
            step.attempt = Some(fixture::committed(&format!("a{index}"), &large));
            journal.push(step);
        }
        fixture::indexed(&path, "thread", &journal).await;

        let reader = TimelineReader::open(&path).await.unwrap();
        let budget = TimelineBudget::new(100, 64 * 1024).unwrap();
        let window = reader
            .page("thread", &TimelinePageQuery::Latest, &budget)
            .await
            .unwrap();
        assert!(
            window.byte_budget_exhausted,
            "the byte budget, not the item cap, must end this page"
        );
        assert!(window.entries.len() < budget.max_items);
        for entry in &window.entries {
            assert!(entry.preview.byte_len() <= PREVIEW_BYTES);
            if entry.content.is_some() {
                assert!(!entry.preview.is_complete());
                assert!(entry.preview.decode_item().is_none());
            }
        }
        assert!(fixture::preview_bytes(&window.entries) <= budget.max_bytes + PREVIEW_BYTES);
        reader.close().await.unwrap();
    }

    #[tokio::test]
    async fn a_frozen_cursor_excludes_later_items_and_later_item_versions() {
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
        let mut first_answer = fixture::commit(3);
        first_answer.attempt = Some(fixture::committed("a1", "first answer"));
        journal.push(first_answer);
        fixture::indexed(&path, "thread", &journal).await;

        let reader = TimelineReader::open(&path).await.unwrap();
        let frozen = reader
            .page(
                "thread",
                &TimelinePageQuery::Latest,
                &TimelineBudget::default(),
            )
            .await
            .unwrap();
        assert_eq!(frozen.read_watermark, 3);
        let cursor = TimelineCursor {
            version: 1,
            thread_id: "thread".into(),
            generation: frozen.generation.clone(),
            watermark: 3,
            ordinal: i64::MAX as u64,
            slot_key: String::new(),
        }
        .encode()
        .unwrap();
        reader.close().await.unwrap();

        let mut later = fixture::commit(4);
        later.inputs = vec![fixture::accepted("input2", 2, "later")].into();
        later.attempt = Some(fixture::committed("a1", "second answer"));
        let mut extended = journal.clone();
        extended.push(later.clone());
        fixture::seed(&path, "thread", &extended).await;
        fixture::resume_and_persist(&path, "thread", None, &later).await;

        let reader = TimelineReader::open(&path).await.unwrap();
        let frozen_page = reader
            .page(
                "thread",
                &TimelinePageQuery::Before {
                    cursor: cursor.clone(),
                },
                &TimelineBudget::default(),
            )
            .await
            .unwrap();
        assert_eq!(frozen_page.read_watermark, 3);
        assert!(
            frozen_page
                .entries
                .iter()
                .all(|entry| entry.item_id != "input2"),
            "items admitted after the frozen watermark must not appear"
        );
        let texts = fixture::answer_texts(&frozen_page.entries);
        assert!(texts.contains(&"first answer".to_string()), "saw {texts:?}");
        assert!(!texts.contains(&"second answer".to_string()));

        let head_page = reader
            .page(
                "thread",
                &TimelinePageQuery::Latest,
                &TimelineBudget::default(),
            )
            .await
            .unwrap();
        let head_texts = fixture::answer_texts(&head_page.entries);
        assert!(head_texts.contains(&"second answer".to_string()));
        reader.close().await.unwrap();
    }

    #[tokio::test]
    async fn invalid_and_stale_cursors_and_unknown_items_are_typed_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let mut journal = vec![{
            let mut first = fixture::commit(1);
            first.turn = Some(fixture::turn(None));
            first
        }];
        let mut second = fixture::commit(2);
        second.attempt = Some(fixture::committed("a1", "answer"));
        journal.push(second);
        fixture::indexed(&path, "thread", &journal).await;

        let reader = TimelineReader::open(&path).await.unwrap();
        let window = reader
            .page(
                "thread",
                &TimelinePageQuery::Latest,
                &TimelineBudget::default(),
            )
            .await
            .unwrap();

        let other_thread = TimelineCursor {
            version: 1,
            thread_id: "elsewhere".into(),
            generation: window.generation.clone(),
            watermark: 2,
            ordinal: 10,
            slot_key: String::new(),
        }
        .encode()
        .unwrap();
        let error = reader
            .page(
                "thread",
                &TimelinePageQuery::Before {
                    cursor: other_thread,
                },
                &TimelineBudget::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, TimelineStoreError::InvalidCursor(_)));

        let stale = TimelineCursor {
            version: 1,
            thread_id: "thread".into(),
            generation: "other-generation".into(),
            watermark: 2,
            ordinal: 10,
            slot_key: String::new(),
        }
        .encode()
        .unwrap();
        let error = reader
            .page(
                "thread",
                &TimelinePageQuery::After { cursor: stale },
                &TimelineBudget::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, TimelineStoreError::StaleCursor(_)));

        let error = reader
            .page(
                "thread",
                &TimelinePageQuery::Around {
                    item_id: "ghost".into(),
                },
                &TimelineBudget::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, TimelineStoreError::UnknownItem { .. }));

        let error = reader
            .page(
                "other",
                &TimelinePageQuery::Latest,
                &TimelineBudget::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, TimelineStoreError::ThreadNotIndexed { .. }));
        reader.close().await.unwrap();
    }
}
