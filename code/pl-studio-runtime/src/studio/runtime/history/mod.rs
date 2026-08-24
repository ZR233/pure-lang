use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Result;
use pl_core::ThreadHotHistory;
use pl_protocol::{ThreadContextDisposition, ThreadItem, ThreadTurnHistory, ThreadTurnPage, Turn};

use crate::studio::StudioRuntime;

impl StudioRuntime {
    pub async fn list_thread_turns(
        &self,
        thread_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ThreadTurnPage> {
        let limit = limit.clamp(1, 200);
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
        let mut merged = Vec::new();
        let mut positions = HashMap::new();
        let hot_turns = hot
            .turns
            .iter()
            .cloned()
            .map(|turn| (turn.id.clone(), turn))
            .collect::<HashMap<_, _>>();
        let hot_items = hot_items_by_turn(hot.items);

        for turn in selection.turns {
            push_or_overlay(
                &mut merged,
                &mut positions,
                ThreadTurnHistory {
                    items: hot_items.get(&turn.id).cloned().unwrap_or_default(),
                    turn,
                    context_disposition: ThreadContextDisposition::Active,
                },
                &hot_turns,
                &hot_items,
            );
        }

        let mut has_more = merged.len() > limit;
        if !has_more {
            let mut cold_cursor = selection.cold_cursor;
            loop {
                let page = self
                    .store
                    .list_thread_turns(thread_id, cold_cursor.as_deref(), 200)
                    .await?;
                let next_cursor = page.next_cursor;
                for history in page.turns {
                    if selection.excluded_turn_ids.contains(&history.turn.id) {
                        continue;
                    }
                    push_or_overlay(&mut merged, &mut positions, history, &hot_turns, &hot_items);
                    if merged.len() > limit {
                        has_more = true;
                        break;
                    }
                }
                if has_more {
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

fn push_or_overlay(
    merged: &mut Vec<ThreadTurnHistory>,
    positions: &mut HashMap<String, usize>,
    mut history: ThreadTurnHistory,
    hot_turns: &HashMap<String, Turn>,
    hot_items: &HashMap<String, Vec<ThreadItem>>,
) {
    if let Some(position) = positions.get(&history.turn.id).copied() {
        // 热事实已经占位；冷记录只补充恢复上下文归属，不能覆盖业务状态。
        merged[position].context_disposition = history.context_disposition;
        return;
    }
    if let Some(turn) = hot_turns.get(&history.turn.id) {
        history.turn = turn.clone();
    }
    history.items = overlay_items(
        history.items,
        hot_items.get(&history.turn.id).map(Vec::as_slice),
    );
    positions.insert(history.turn.id.clone(), merged.len());
    merged.push(history);
}

fn overlay_items(cold: Vec<ThreadItem>, hot: Option<&[ThreadItem]>) -> Vec<ThreadItem> {
    let Some(hot) = hot else {
        return cold;
    };
    let mut by_id = cold
        .into_iter()
        .map(|item| (item.id.clone(), item))
        .collect::<BTreeMap<_, _>>();
    for item in hot {
        by_id.insert(item.id.clone(), item.clone());
    }
    let mut items = by_id.into_values().collect::<Vec<_>>();
    items.sort_by_key(|item| item.ordinal);
    items
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
    fn hot_turn_and_item_replace_same_identity_without_losing_cold_context() {
        let hot_turn = turn("turn-2", 20);
        let hot_item = item("item-2", "turn-2", 2, "hot");
        let hot_turns = HashMap::from([("turn-2".to_string(), hot_turn.clone())]);
        let hot_items = HashMap::from([("turn-2".to_string(), vec![hot_item.clone()])]);
        let mut merged = Vec::new();
        let mut positions = HashMap::new();

        push_or_overlay(
            &mut merged,
            &mut positions,
            ThreadTurnHistory {
                turn: turn("turn-2", 2),
                items: vec![item("item-2", "turn-2", 2, "cold")],
                context_disposition: ThreadContextDisposition::RolledBack,
            },
            &hot_turns,
            &hot_items,
        );

        assert_eq!(merged[0].turn, hot_turn);
        assert_eq!(merged[0].items, vec![hot_item]);
        assert_eq!(
            merged[0].context_disposition,
            ThreadContextDisposition::RolledBack
        );
    }
}
