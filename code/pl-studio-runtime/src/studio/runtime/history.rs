//! History pages derive from the same core journal as the live product subscription.
use crate::studio::StudioRuntime;
use anyhow::Result;
use pl_core::thread::{ContextReplacementReason, ThreadSnapshot};
use pl_protocol::{ThreadContextDisposition, ThreadTurnHistory, ThreadTurnPage};
use std::collections::{BTreeMap, BTreeSet};

impl StudioRuntime {
    pub async fn list_thread_turns(
        &self,
        thread_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ThreadTurnPage> {
        let (snapshot, journal) = self.read_thread_facts(thread_id).await?;
        let turns =
            crate::studio::thread_projection::project_turns(thread_id, &snapshot, &journal)?;
        let items =
            crate::studio::thread_projection::project_items(thread_id, &snapshot, &journal)?;
        let mut by_turn = BTreeMap::<_, Vec<_>>::new();
        for item in items {
            by_turn.entry(item.turn_id.clone()).or_default().push(item);
        }
        let rolled_back = rolled_back_turns(&snapshot);
        let mut history = turns
            .into_iter()
            .rev()
            .map(|turn| ThreadTurnHistory {
                context_disposition: if rolled_back.contains(&turn.id) {
                    ThreadContextDisposition::RolledBack
                } else {
                    ThreadContextDisposition::Active
                },
                items: by_turn.remove(&turn.id).unwrap_or_default(),
                turn,
            })
            .collect::<Vec<_>>();
        let start = match cursor {
            None => 0,
            Some(cursor) => {
                history
                    .iter()
                    .position(|entry| entry.turn.id == cursor)
                    .ok_or_else(|| {
                        anyhow::anyhow!("history cursor does not belong to this Thread")
                    })?
                    + 1
            }
        };
        let limit = limit.clamp(1, 200);
        let has_more = history.len().saturating_sub(start) > limit;
        history = history.into_iter().skip(start).take(limit).collect();
        Ok(ThreadTurnPage {
            next_cursor: has_more
                .then(|| history.last().map(|entry| entry.turn.id.clone()))
                .flatten(),
            turns: history,
        })
    }
}

fn rolled_back_turns(snapshot: &ThreadSnapshot) -> BTreeSet<String> {
    let mut removed = BTreeSet::new();
    for replacement in snapshot.context_replacements.iter() {
        if replacement.reason != ContextReplacementReason::Rewind {
            continue;
        }
        let retained = replacement
            .current
            .records
            .iter()
            .filter_map(|record| record.turn_id.as_ref())
            .collect::<BTreeSet<_>>();
        for record in replacement.previous.records.iter() {
            if let Some(turn) = &record.turn_id
                && !retained.contains(turn)
            {
                removed.insert(turn.clone());
            }
        }
    }
    removed
}
