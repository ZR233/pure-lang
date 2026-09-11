//! Rebuildable item index owned by Thread residency; pages never replay a hot journal.
use super::StudioRuntime;
use anyhow::{Result, bail};
use pl_protocol::{ThreadItem, TimelinePage, TimelineQuery, TimelineTurn};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct TimelineIndex {
    watermark: u64,
    items: BTreeMap<u64, ThreadItem>,
    positions: BTreeMap<String, u64>,
    turns: Vec<TimelineTurn>,
}

impl TimelineIndex {
    pub(super) fn update(
        &mut self,
        watermark: u64,
        items: &[ThreadItem],
        turns: Vec<TimelineTurn>,
    ) {
        if watermark < self.watermark {
            return;
        }
        // Items are immutable identities. A new projection updates admitted/changed
        // entries only; absence in a preview is not a deletion.
        for item in items {
            // Ephemeral preview order is subscription-local. History pages do
            // not arbitrate competing previews at the same committed watermark.
            if watermark == self.watermark && self.items.contains_key(&item.ordinal) {
                continue;
            }
            if self.items.get(&item.ordinal) != Some(item) {
                self.positions.insert(item.id.clone(), item.ordinal);
                self.items.insert(item.ordinal, item.clone());
            }
        }
        self.watermark = watermark;
        self.turns = turns;
    }

    fn page(&self, thread_id: &str, query: &TimelineQuery, limit: usize) -> Result<TimelinePage> {
        let limit = limit.clamp(1, 100);
        let cursor = match query {
            TimelineQuery::Latest => None,
            TimelineQuery::Before { item_id } | TimelineQuery::After { item_id } | TimelineQuery::Around { item_id } => {
                Some(*self.positions.get(item_id).ok_or_else(|| anyhow::anyhow!("timeline query.itemId does not belong to this Thread; reload latest or choose an available item"))?)
            }
        };
        let items: Vec<_> = match (query, cursor) {
            (TimelineQuery::Latest, _) => self
                .items
                .values()
                .rev()
                .take(limit)
                .rev()
                .cloned()
                .collect(),
            (TimelineQuery::Before { .. }, Some(position)) => {
                let mut items: Vec<_> = self
                    .items
                    .range(..position)
                    .rev()
                    .take(limit)
                    .map(|(_, item)| item.clone())
                    .collect();
                items.reverse();
                items
            }
            (TimelineQuery::After { .. }, Some(position)) => self
                .items
                .range((
                    std::ops::Bound::Excluded(position),
                    std::ops::Bound::Unbounded,
                ))
                .take(limit)
                .map(|(_, item)| item.clone())
                .collect(),
            (TimelineQuery::Around { .. }, Some(position)) => {
                let start = self
                    .items
                    .range(..position)
                    .rev()
                    .take(limit / 2)
                    .last()
                    .map_or(position, |(ordinal, _)| *ordinal);
                self.items
                    .range(start..)
                    .take(limit)
                    .map(|(_, item)| item.clone())
                    .collect()
            }
            _ => bail!("timeline query has no cursor"),
        };
        let ids: BTreeSet<_> = items.iter().map(|item| item.turn_id.as_str()).collect();
        Ok(TimelinePage {
            thread_id: thread_id.into(),
            watermark: self.watermark,
            older_cursor: items
                .first()
                .filter(|item| self.items.range(..item.ordinal).next().is_some())
                .map(|item| item.id.clone()),
            newer_cursor: items
                .last()
                .filter(|item| {
                    self.items
                        .range((
                            std::ops::Bound::Excluded(item.ordinal),
                            std::ops::Bound::Unbounded,
                        ))
                        .next()
                        .is_some()
                })
                .map(|item| item.id.clone()),
            first_item_id: items.first().map(|item| item.id.clone()),
            last_item_id: items.last().map(|item| item.id.clone()),
            turns: self
                .turns
                .iter()
                .filter(|entry| ids.contains(entry.turn.id.as_str()))
                .cloned()
                .collect(),
            items,
        })
    }
}

impl StudioRuntime {
    /// Returns an item page, including related Turn metadata, without executing the Thread.
    ///
    /// # Errors
    /// Fails on unknown Thread/item identity or canonical storage/projection failure.
    pub async fn list_timeline_items(
        &self,
        thread_id: &str,
        query: TimelineQuery,
        limit: usize,
    ) -> Result<TimelinePage> {
        self.read_owned_thread(thread_id).await?;
        let hot = self
            .threads
            .observed_threads()
            .into_iter()
            .find(|(id, _)| id == thread_id);
        let watermark = hot
            .as_ref()
            .map(|(_, handle)| handle.snapshot().commit_sequence);
        {
            let indexes = self.residency.timelines.lock().await;
            if let Some(index) = indexes.get(thread_id)
                && watermark.is_none_or(|watermark| watermark == index.watermark)
            {
                return index.page(thread_id, &query, limit);
            }
        }
        let (state, journal) = self.read_thread_facts(thread_id).await?;
        let items = crate::studio::thread_projection::project_items(thread_id, &state, &journal)?;
        self.index_timeline(thread_id, &state, &items).await;
        let mut indexes = self.residency.timelines.lock().await;
        if hot.is_none() && !self.residency.is_pinned(thread_id) {
            // A cold read has no resident owner: release its rebuildable index
            // with this request instead of accumulating cold Threads forever.
            return indexes
                .remove(thread_id)
                .ok_or_else(|| anyhow::anyhow!("Thread timeline was evicted; retry the page"))?
                .page(thread_id, &query, limit);
        }
        indexes
            .get(thread_id)
            .ok_or_else(|| anyhow::anyhow!("Thread timeline was evicted; retry the page"))?
            .page(thread_id, &query, limit)
    }

    pub(super) async fn index_timeline(
        &self,
        thread_id: &str,
        state: &pl_core::thread::ThreadSnapshot,
        items: &[ThreadItem],
    ) {
        let ends: BTreeMap<_, _> = items
            .iter()
            .filter(|item| item.kind() != pl_protocol::ThreadItemKind::ContextCompaction)
            .map(|item| (item.turn_id.as_str(), item.id.as_str()))
            .collect();
        let turns = items.iter().filter_map(|item| {
            let pl_protocol::ThreadItemState::Turn(turn) = item.state() else {
                return None;
            };
            Some(pl_protocol::Turn {
                id: item.turn_id.clone(),
                thread_id: thread_id.into(),
                input_id: turn.input_id().map(str::to_owned),
                revision: item.revision,
                state: turn.state().clone(),
                updated_at: item.updated_at,
            })
        });
        let rolled_back = super::history::rolled_back_turns(state);
        let turns = turns
            .filter_map(|turn| {
                let last = ends.get(turn.id.as_str())?;
                Some(TimelineTurn {
                    context_disposition: if rolled_back.contains(&turn.id) {
                        pl_protocol::ThreadContextDisposition::RolledBack
                    } else {
                        pl_protocol::ThreadContextDisposition::Active
                    },
                    turn,
                    last_item_id: (*last).into(),
                })
            })
            .collect();
        self.residency
            .timelines
            .lock()
            .await
            .entry(thread_id.into())
            .or_default()
            .update(state.commit_sequence, items, turns);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_protocol::{ThreadContentLifecycle, ThreadItemState, ThreadTextChannel, ThreadTextItem};
    use pretty_assertions::assert_eq;

    fn items(count: u64) -> Vec<ThreadItem> {
        (0..count)
            .map(|ordinal| {
                ThreadItem::new(
                    format!("item-{ordinal}"),
                    "thread".into(),
                    "one-large-turn".into(),
                    ordinal,
                    1,
                    1,
                    1,
                    ThreadItemState::Text(ThreadTextItem::new(
                        ThreadTextChannel::Final,
                        format!("line {ordinal}\n代码 \\n"),
                        Vec::new(),
                        ThreadContentLifecycle::completed(1),
                    )),
                )
            })
            .collect()
    }

    #[test]
    fn large_turn_can_be_read_in_both_directions_without_gaps_or_repeated_pages() {
        let expected = items(1103);
        let mut index = TimelineIndex::default();
        index.update(1, &expected, Vec::new());
        let mut query = TimelineQuery::Latest;
        let mut collected = Vec::new();
        loop {
            let page = index.page("thread", &query, 100).unwrap();
            assert!(page.items.len() <= 100);
            let mut previous = page.items.clone();
            previous.extend(collected);
            collected = previous;
            let Some(item_id) = page.older_cursor else {
                break;
            };
            query = TimelineQuery::Before { item_id };
        }
        assert_eq!(collected, expected);
        let mut collected = Vec::new();
        let mut query = TimelineQuery::Around {
            item_id: "item-0".into(),
        };
        loop {
            let page = index.page("thread", &query, 100).unwrap();
            collected.extend(page.items);
            let Some(item_id) = page.newer_cursor else {
                break;
            };
            query = TimelineQuery::After { item_id };
        }
        assert_eq!(collected, expected);
        assert!(
            index
                .page(
                    "thread",
                    &TimelineQuery::Before {
                        item_id: "other-thread-item".into()
                    },
                    100
                )
                .is_err()
        );
        assert!(
            index
                .page(
                    "thread",
                    &TimelineQuery::Before {
                        item_id: "item-0".into()
                    },
                    100
                )
                .unwrap()
                .items
                .is_empty()
        );
        let around = index
            .page(
                "thread",
                &TimelineQuery::Around {
                    item_id: "item-500".into(),
                },
                100,
            )
            .unwrap();
        assert_eq!(around.items, expected[450..550]);
    }

    #[test]
    fn incremental_updates_keep_older_items_and_reject_an_older_watermark() {
        let mut index = TimelineIndex::default();
        let expected = items(4);
        index.update(2, &expected[..3], Vec::new());
        index.update(3, &expected[3..], Vec::new());
        index.update(1, &items(8), Vec::new());
        let page = index.page("thread", &TimelineQuery::Latest, 100).unwrap();
        assert_eq!(page.watermark, 3);
        assert_eq!(page.items, expected);
        assert_eq!(
            TimelineIndex::default()
                .page("thread", &TimelineQuery::Latest, 100)
                .unwrap()
                .items,
            Vec::new()
        );
    }
}
