//! 活动会话的内存历史分页；冷存储不参与实时查询。

use pl_protocol::{AgentSessionReadDetail, AgentSessionReadOrder, ThreadItemState};

use super::super::{AgentIdentity, AgentSessionTimelineKey};
use super::*;

pub(super) fn read_memory_session(
    runtime: &AgentRuntimeHandle,
    identity: AgentIdentity,
    query: AgentSessionTimelineQuery,
) -> AgentRuntimeResult<AgentSessionTimelineRepositoryPage> {
    if !(1..=50).contains(&query.limit)
        || (query.anchor.is_some()
            && (query.watermark.is_none() || query.through_sequence.is_none()))
    {
        return Err(AgentRuntimeError::InvalidInput(
            "invalid timeline page limit or continuation".into(),
        ));
    }
    let snapshot = runtime
        .thread_events
        .snapshot(query.target.as_str())
        .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
    let through_sequence = query.through_sequence.unwrap_or(snapshot.revision);
    if through_sequence > snapshot.revision {
        return Err(AgentRuntimeError::InvalidInput(
            "timeline cursor is ahead of memory".into(),
        ));
    }
    let key = |item: &pl_protocol::ThreadItem| AgentSessionTimelineKey {
        ordinal: item.ordinal,
        item_id: item.id.clone(),
    };
    let mut items = snapshot
        .items
        .into_iter()
        .filter(|item| match query.detail {
            AgentSessionReadDetail::Full => true,
            AgentSessionReadDetail::Text => matches!(item.state(), ThreadItemState::Text(_)),
        })
        .collect::<Vec<_>>();
    items.sort_by_key(&key);
    for cursor in query.watermark.iter().chain(query.anchor.iter()) {
        if !items.iter().any(|item| key(item) == *cursor) {
            return Err(AgentRuntimeError::InvalidInput(
                "timeline cursor does not belong to this query".into(),
            ));
        }
    }
    let watermark = query.watermark.or_else(|| items.last().map(&key));
    items.retain(|item| {
        watermark
            .as_ref()
            .is_some_and(|watermark| key(item) <= *watermark)
            && query
                .anchor
                .as_ref()
                .is_none_or(|anchor| match query.order {
                    AgentSessionReadOrder::Ascending => key(item) > *anchor,
                    AgentSessionReadOrder::Descending => key(item) < *anchor,
                })
    });
    if query.order == AgentSessionReadOrder::Descending {
        items.reverse();
    }
    let has_more = items.len() > query.limit;
    items.truncate(query.limit);
    let next_anchor = has_more.then(|| items.last().map(&key)).flatten();
    let directory = runtime.directory_snapshot();
    let mut path = vec![identity.id.clone()];
    let mut parent = identity.parent_id.clone();
    while let Some(id) = parent {
        if path.contains(&id) {
            return Err(AgentRuntimeError::InvalidInput(
                "agent lineage contains a cycle".into(),
            ));
        }
        path.push(id.clone());
        parent = directory
            .agents
            .iter()
            .find(|agent| agent.identity.id == id)
            .and_then(|agent| agent.identity.parent_id.clone());
    }
    path.reverse();
    Ok(AgentSessionTimelineRepositoryPage {
        identity,
        path,
        through_sequence,
        watermark,
        items,
        has_more,
        next_anchor,
    })
}
