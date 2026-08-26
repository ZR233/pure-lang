use std::collections::{HashMap, HashSet};

use anyhow::Result;
use pl_core::ThreadHotHistory;
use pl_protocol::{ThreadContextDisposition, ThreadItem, ThreadTurnHistory, ThreadTurnPage, Turn};

use crate::studio::StudioRuntime;
use crate::studio::merged_page::{HotColdEntry, overlay_cold_page};

impl HotColdEntry for ThreadTurnHistory {
    type Key = (i64, String);

    fn page_key(&self) -> Self::Key {
        (self.turn.updated_at, self.turn.id.clone())
    }

    fn entry_id(&self) -> &str {
        &self.turn.id
    }
}

impl StudioRuntime {
    pub async fn list_thread_turns(
        &self,
        thread_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ThreadTurnPage> {
        let limit = limit.clamp(1, 200);
        // 冷历史仍需先经过 root recovery gate；坏 payload 只能由显式 cleanup 处置，
        // 不得直接落入 serde 错误或在读取时跳过/升级。
        self.read_owned_thread(thread_id).await?;
        let Some((handle, agent_id)) = self.try_get_thread_handle(thread_id).await? else {
            return self.store.list_thread_turns(thread_id, cursor, limit).await;
        };
        let hot = handle
            .thread_hot_history(&agent_id)
            .map_err(|error| anyhow::anyhow!(error))?;
        self.list_resident_thread_turns(thread_id, cursor, limit, hot)
            .await
    }

    async fn list_resident_thread_turns(
        &self,
        thread_id: &str,
        cursor: Option<&str>,
        limit: usize,
        hot: ThreadHotHistory,
    ) -> Result<ThreadTurnPage> {
        let selection = select_hot_turns(&hot.turns, cursor);
        let hot_items = hot_items_by_turn(hot.items);
        // 热窗口是最新后缀：合并核心按追加顺序保留热选择，冷页随后补齐更早
        // 历史；同 id 热事实胜出，冷记录只补充 rolled-back disposition。
        let mut merged = selection
            .turns
            .into_iter()
            .map(|turn| ThreadTurnHistory {
                items: hot_items.get(&turn.id).cloned().unwrap_or_default(),
                turn,
                context_disposition: ThreadContextDisposition::Active,
            })
            .collect::<Vec<_>>();

        let mut has_more = merged.len() > limit;
        if !has_more {
            let mut cold_cursor = selection.cold_cursor;
            loop {
                let page = self
                    .store
                    .list_thread_turns(thread_id, cold_cursor.as_deref(), 200)
                    .await?;
                let next_cursor = page.next_cursor;
                let cold = page
                    .turns
                    .into_iter()
                    .filter(|history| !selection.excluded_turn_ids.contains(&history.turn.id))
                    .collect::<Vec<_>>();
                merged = overlay_cold_page(merged, cold, None, |cold, hot| {
                    hot.context_disposition = cold.context_disposition;
                });
                if merged.len() > limit {
                    has_more = true;
                    break;
                }
                let Some(next_cursor) = next_cursor else {
                    break;
                };
                cold_cursor = Some(next_cursor);
            }
        }

        if merged.len() > limit {
            merged.truncate(limit);
        }
        let next_cursor = has_more
            .then(|| merged.last().map(|history| history.turn.id.clone()))
            .flatten();
        Ok(ThreadTurnPage {
            turns: merged,
            next_cursor,
        })
    }
}

struct HotTurnSelection {
    turns: Vec<Turn>,
    excluded_turn_ids: HashSet<String>,
    cold_cursor: Option<String>,
}

fn select_hot_turns(turns: &[Turn], cursor: Option<&str>) -> HotTurnSelection {
    let descending = turns.iter().rev().cloned().collect::<Vec<_>>();
    let Some(cursor) = cursor else {
        return HotTurnSelection {
            turns: descending,
            excluded_turn_ids: HashSet::new(),
            cold_cursor: None,
        };
    };
    let Some(position) = descending.iter().position(|turn| turn.id == cursor) else {
        return HotTurnSelection {
            turns: Vec::new(),
            excluded_turn_ids: HashSet::new(),
            cold_cursor: Some(cursor.to_string()),
        };
    };
    HotTurnSelection {
        turns: descending.iter().skip(position + 1).cloned().collect(),
        excluded_turn_ids: descending
            .iter()
            .take(position + 1)
            .map(|turn| turn.id.clone())
            .collect(),
        // 热 cursor 可能尚未进入 SQLite；从冷历史最新端开始，再排除 cursor 及
        // 其后的热 Turn，才能稳定跨越冷热边界。
        cold_cursor: None,
    }
}

fn hot_items_by_turn(items: Vec<ThreadItem>) -> HashMap<String, Vec<ThreadItem>> {
    let mut by_turn = HashMap::<String, Vec<ThreadItem>>::new();
    for item in items {
        by_turn.entry(item.turn_id.clone()).or_default().push(item);
    }
    for items in by_turn.values_mut() {
        items.sort_by_key(|item| item.ordinal);
    }
    by_turn
}

#[cfg(test)]
mod tests {
    use pl_protocol::{ThreadContentLifecycle, ThreadItemState, ThreadTextChannel, ThreadTextItem};

    use super::*;

    fn turn(id: &str, updated_at: i64) -> Turn {
        Turn::queued(id, "thread-1", updated_at)
    }

    fn item(id: &str, turn_id: &str, ordinal: u64, text: &str) -> ThreadItem {
        ThreadItem::new(
            id.to_string(),
            "thread-1".to_string(),
            turn_id.to_string(),
            ordinal,
            1,
            1,
            1,
            ThreadItemState::Text(ThreadTextItem::new(
                ThreadTextChannel::Final,
                text.to_string(),
                Vec::new(),
                ThreadContentLifecycle::completed(1),
            )),
        )
    }

    #[test]
    fn hot_cursor_excludes_newer_turns_and_continues_toward_cold_history() {
        let turns = vec![turn("turn-1", 1), turn("turn-2", 2), turn("turn-3", 3)];

        let selection = select_hot_turns(&turns, Some("turn-2"));

        assert_eq!(
            selection
                .turns
                .iter()
                .map(|turn| turn.id.as_str())
                .collect::<Vec<_>>(),
            ["turn-1"]
        );
        assert_eq!(
            selection.excluded_turn_ids,
            HashSet::from(["turn-3".to_string(), "turn-2".to_string()])
        );
        assert_eq!(selection.cold_cursor, None);
    }

    #[test]
    fn cold_refines_hot_disposition_without_overriding_business_state() {
        let hot = ThreadTurnHistory {
            turn: turn("turn-2", 20),
            items: vec![item("item-2", "turn-2", 2, "hot")],
            context_disposition: ThreadContextDisposition::Active,
        };
        let cold = vec![
            ThreadTurnHistory {
                turn: turn("turn-2", 2),
                items: vec![item("item-2", "turn-2", 2, "cold")],
                context_disposition: ThreadContextDisposition::RolledBack,
            },
            ThreadTurnHistory {
                turn: turn("turn-1", 1),
                items: vec![item("item-1", "turn-1", 1, "cold-only")],
                context_disposition: ThreadContextDisposition::Active,
            },
        ];

        let merged = overlay_cold_page(
            vec![hot],
            cold,
            None,
            |cold, hot: &mut ThreadTurnHistory| {
                hot.context_disposition = cold.context_disposition;
            },
        );

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].turn.updated_at, 20);
        assert_eq!(merged[0].items, vec![item("item-2", "turn-2", 2, "hot")]);
        assert_eq!(
            merged[0].context_disposition,
            ThreadContextDisposition::RolledBack
        );
        assert_eq!(merged[1].turn.id, "turn-1");
        // 冷记录独有的 rolled-back item 与热 timeline 合并保留。
        assert_eq!(merged[1].items[0].id, "item-1");
    }
}
