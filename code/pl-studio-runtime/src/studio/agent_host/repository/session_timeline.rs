//! durable Agent Timeline 的稳定 keyset 查询。

use std::collections::BTreeSet;

use pl_core::{
    AgentIdentity, AgentRoleId, AgentSessionTimelineKey, AgentSessionTimelineQuery,
    AgentSessionTimelineRepositoryPage, ThreadId,
};
use pl_protocol::{AgentSessionReadDetail, AgentSessionReadOrder, ThreadItem};
use sea_orm::{ColumnTrait, Condition, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::PureError;
use crate::studio::StudioStore;
use crate::studio::entity::{item, thread};

use super::{i64_from_u64, store_error, u64_from_i64};

const MAX_SESSION_PAGE_LIMIT: usize = 50;

pub(super) async fn list_agent_session(
    store: &StudioStore,
    query: AgentSessionTimelineQuery,
) -> Result<AgentSessionTimelineRepositoryPage, PureError> {
    if !(1..=MAX_SESSION_PAGE_LIMIT).contains(&query.limit) {
        return Err(store_error(format!(
            "Agent Timeline limit must be between 1 and {MAX_SESSION_PAGE_LIMIT}"
        )));
    }
    if query.anchor.is_some() && (query.watermark.is_none() || query.through_sequence.is_none()) {
        return Err(store_error(
            "Agent Timeline continuation requires a watermark and through sequence",
        ));
    }
    let target = thread::Entity::find_by_id(query.target.to_string())
        .one(store.database())
        .await
        .map_err(store_error)?
        .ok_or_else(|| store_error(format!("Agent {} was not found", query.target)))?;
    let (identity, path) = agent_lineage(store, &target).await?;
    if identity.id != query.target {
        return Err(store_error(format!(
            "Thread {} belongs to Agent {}, not {}",
            target.id, identity.id, query.target
        )));
    }
    let current_sequence = u64_from_i64(target.revision)?;
    let through_sequence = query.through_sequence.unwrap_or(current_sequence);
    if through_sequence > current_sequence {
        return Err(store_error(format!(
            "Agent Timeline cursor sequence {through_sequence} is ahead of current sequence {current_sequence}"
        )));
    }

    if let Some(watermark) = &query.watermark {
        validate_cursor_key(store, &target.id, watermark, query.detail).await?;
    }
    if let Some(anchor) = &query.anchor {
        validate_cursor_key(store, &target.id, anchor, query.detail).await?;
    }
    let watermark = match query.watermark.clone() {
        Some(watermark) => Some(watermark),
        None => latest_key(store, &target.id, query.detail).await?,
    };
    let Some(watermark) = watermark else {
        return Ok(AgentSessionTimelineRepositoryPage {
            identity,
            path,
            through_sequence,
            watermark: None,
            items: Vec::new(),
            has_more: false,
            next_anchor: None,
        });
    };

    let mut page = item_query(&target.id, query.detail).filter(key_at_or_before(&watermark)?);
    if let Some(anchor) = &query.anchor {
        page = page.filter(match query.order {
            AgentSessionReadOrder::Ascending => key_after(anchor)?,
            AgentSessionReadOrder::Descending => key_before(anchor)?,
        });
    }
    page = match query.order {
        AgentSessionReadOrder::Ascending => page
            .order_by_asc(item::Column::Ordinal)
            .order_by_asc(item::Column::Id),
        AgentSessionReadOrder::Descending => page
            .order_by_desc(item::Column::Ordinal)
            .order_by_desc(item::Column::Id),
    };
    let mut rows = page
        .limit(u64::try_from(query.limit.saturating_add(1)).map_err(store_error)?)
        .all(store.database())
        .await
        .map_err(store_error)?;
    let has_more = rows.len() > query.limit;
    if has_more {
        rows.pop();
    }
    let items = rows
        .into_iter()
        .map(ThreadItem::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    let next_anchor = has_more.then(|| items.last().map(timeline_key)).flatten();
    Ok(AgentSessionTimelineRepositoryPage {
        identity,
        path,
        through_sequence,
        watermark: Some(watermark),
        items,
        has_more,
        next_anchor,
    })
}

fn item_query(thread_id: &str, detail: AgentSessionReadDetail) -> sea_orm::Select<item::Entity> {
    let query = item::Entity::find().filter(item::Column::ThreadId.eq(thread_id));
    match detail {
        AgentSessionReadDetail::Text => query.filter(item::Column::StateKind.eq("text")),
        AgentSessionReadDetail::Full => query,
    }
}

async fn latest_key(
    store: &StudioStore,
    thread_id: &str,
    detail: AgentSessionReadDetail,
) -> Result<Option<AgentSessionTimelineKey>, PureError> {
    item_query(thread_id, detail)
        .order_by_desc(item::Column::Ordinal)
        .order_by_desc(item::Column::Id)
        .one(store.database())
        .await
        .map_err(store_error)?
        .map(|row| {
            Ok(AgentSessionTimelineKey {
                ordinal: u64_from_i64(row.ordinal)?,
                item_id: row.id,
            })
        })
        .transpose()
}

async fn validate_cursor_key(
    store: &StudioStore,
    thread_id: &str,
    key: &AgentSessionTimelineKey,
    detail: AgentSessionReadDetail,
) -> Result<(), PureError> {
    let row = item::Entity::find_by_id(key.item_id.clone())
        .one(store.database())
        .await
        .map_err(store_error)?
        .ok_or_else(|| {
            store_error(format!(
                "unknown Agent Timeline cursor item {}",
                key.item_id
            ))
        })?;
    let detail_matches = match detail {
        AgentSessionReadDetail::Text => row.state_kind == "text",
        AgentSessionReadDetail::Full => true,
    };
    if row.thread_id != thread_id || u64_from_i64(row.ordinal)? != key.ordinal || !detail_matches {
        return Err(store_error(format!(
            "Agent Timeline cursor item {} does not belong to this query",
            key.item_id
        )));
    }
    Ok(())
}

fn key_at_or_before(key: &AgentSessionTimelineKey) -> Result<Condition, PureError> {
    let ordinal = i64_from_u64(key.ordinal)?;
    Ok(Condition::any().add(item::Column::Ordinal.lt(ordinal)).add(
        Condition::all()
            .add(item::Column::Ordinal.eq(ordinal))
            .add(item::Column::Id.lte(key.item_id.clone())),
    ))
}

fn key_before(key: &AgentSessionTimelineKey) -> Result<Condition, PureError> {
    let ordinal = i64_from_u64(key.ordinal)?;
    Ok(Condition::any().add(item::Column::Ordinal.lt(ordinal)).add(
        Condition::all()
            .add(item::Column::Ordinal.eq(ordinal))
            .add(item::Column::Id.lt(key.item_id.clone())),
    ))
}

fn key_after(key: &AgentSessionTimelineKey) -> Result<Condition, PureError> {
    let ordinal = i64_from_u64(key.ordinal)?;
    Ok(Condition::any().add(item::Column::Ordinal.gt(ordinal)).add(
        Condition::all()
            .add(item::Column::Ordinal.eq(ordinal))
            .add(item::Column::Id.gt(key.item_id.clone())),
    ))
}

fn timeline_key(item: &ThreadItem) -> AgentSessionTimelineKey {
    AgentSessionTimelineKey {
        ordinal: item.ordinal,
        item_id: item.id.clone(),
    }
}

async fn agent_lineage(
    store: &StudioStore,
    target: &thread::Model,
) -> Result<(AgentIdentity, Vec<ThreadId>), PureError> {
    let mut seen = BTreeSet::new();
    let mut path = vec![ThreadId::new(target.agent_path.clone())?];
    let mut parent_thread_id = target.parent_thread_id.clone();
    let parent_id = match &parent_thread_id {
        Some(parent_thread_id) => {
            let parent = thread::Entity::find_by_id(parent_thread_id.clone())
                .one(store.database())
                .await
                .map_err(store_error)?
                .ok_or_else(|| {
                    store_error(format!("Agent parent {parent_thread_id} is missing"))
                })?;
            Some(ThreadId::new(parent.agent_path)?)
        }
        None => None,
    };
    while let Some(thread_id) = parent_thread_id {
        if !seen.insert(thread_id.clone()) {
            return Err(store_error("Agent parent graph contains a cycle"));
        }
        let parent = thread::Entity::find_by_id(thread_id.clone())
            .one(store.database())
            .await
            .map_err(store_error)?
            .ok_or_else(|| store_error(format!("Agent parent {thread_id} is missing")))?;
        path.push(ThreadId::new(parent.agent_path)?);
        parent_thread_id = parent.parent_thread_id;
    }
    path.reverse();
    let depth = u32::try_from(path.len().saturating_sub(1)).map_err(store_error)?;
    Ok((
        AgentIdentity {
            id: ThreadId::new(target.agent_path.clone())?,
            parent_id,
            role: AgentRoleId::new(target.role.clone())?,
            depth,
        },
        path,
    ))
}

#[cfg(test)]
mod tests {
    use pl_protocol::{
        CompletedTurnState, ThreadContentLifecycle, ThreadFileItem, ThreadItemState, ThreadModeId,
        ThreadTextChannel, ThreadTextItem, TurnCompletion, TurnState,
    };
    use sea_orm::{ActiveModelTrait, ActiveValue::Set};

    use super::*;

    #[tokio::test]
    async fn timeline_pages_freeze_watermark_and_filter_detail() {
        let store = StudioStore::open_memory().await.unwrap();
        let workspace = std::env::temp_dir().join("agent-session-timeline-test");
        std::fs::create_dir_all(&workspace).unwrap();
        let project = store.upsert_project(&workspace).await.unwrap();
        let root = store
            .create_thread(&project.id, "Root", ThreadModeId::simple())
            .await
            .unwrap();
        let child_id = "child-timeline";
        insert_child(&store, &root.id, &project.id, child_id).await;
        insert_turn(&store, child_id, "turn-1").await;
        for ordinal in 1..=25 {
            insert_item(
                &store,
                child_id,
                "turn-1",
                ordinal,
                ThreadItemState::Text(ThreadTextItem::new(
                    ThreadTextChannel::Commentary,
                    format!("text-{ordinal}"),
                    Vec::new(),
                    ThreadContentLifecycle::completed(ordinal),
                )),
            )
            .await;
        }
        insert_item(
            &store,
            child_id,
            "turn-1",
            26,
            ThreadItemState::File(ThreadFileItem::new(
                "artifact.txt".to_string(),
                Some("text/plain".to_string()),
                26,
            )),
        )
        .await;

        let first = list_agent_session(
            &store,
            AgentSessionTimelineQuery {
                target: ThreadId::new(child_id).unwrap(),
                order: AgentSessionReadOrder::Descending,
                detail: AgentSessionReadDetail::Text,
                limit: 20,
                through_sequence: None,
                watermark: None,
                anchor: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            first.path,
            [
                ThreadId::new(root.id.clone()).unwrap(),
                ThreadId::new(child_id).unwrap()
            ]
        );
        assert_eq!(first.items.len(), 20);
        assert_eq!(first.items[0].ordinal, 25);
        assert_eq!(first.items[19].ordinal, 6);
        assert!(first.has_more);

        insert_item(
            &store,
            child_id,
            "turn-1",
            27,
            ThreadItemState::Text(ThreadTextItem::new(
                ThreadTextChannel::Final,
                "new-after-first-page".to_string(),
                Vec::new(),
                ThreadContentLifecycle::completed(27),
            )),
        )
        .await;
        let second = list_agent_session(
            &store,
            AgentSessionTimelineQuery {
                target: ThreadId::new(child_id).unwrap(),
                order: AgentSessionReadOrder::Descending,
                detail: AgentSessionReadDetail::Text,
                limit: 20,
                through_sequence: Some(first.through_sequence),
                watermark: first.watermark.clone(),
                anchor: first.next_anchor.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            second
                .items
                .iter()
                .map(|item| item.ordinal)
                .collect::<Vec<_>>(),
            [5, 4, 3, 2, 1]
        );
        assert!(!second.has_more);

        let ascending = list_agent_session(
            &store,
            AgentSessionTimelineQuery {
                target: ThreadId::new(child_id).unwrap(),
                order: AgentSessionReadOrder::Ascending,
                detail: AgentSessionReadDetail::Full,
                limit: 50,
                through_sequence: None,
                watermark: None,
                anchor: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(ascending.items.first().unwrap().ordinal, 1);
        assert_eq!(ascending.items.last().unwrap().ordinal, 27);
        assert!(
            ascending
                .items
                .iter()
                .any(|item| item.kind() == pl_protocol::ThreadItemKind::File)
        );
    }

    async fn insert_child(store: &StudioStore, root_id: &str, project_id: &str, child_id: &str) {
        thread::ActiveModel {
            id: Set(child_id.to_string()),
            project_id: Set(project_id.to_string()),
            title: Set("Child".to_string()),
            mode: Set(ThreadModeId::simple().label().to_string()),
            root_thread_id: Set(root_id.to_string()),
            parent_thread_id: Set(Some(root_id.to_string())),
            role: Set("executor".to_string()),
            agent_path: Set(child_id.to_string()),
            state_json: Set(serde_json::to_string(&pl_core::AgentState::idle()).unwrap()),
            revision: Set(30),
            runtime_revision: Set(Some(1)),
            event_sequence: Set(1),
            metadata_json: Set("{}".to_string()),
            usage_json: Set(
                serde_json::to_string(&pl_protocol::InferenceTokenUsage::default()).unwrap(),
            ),
            last_context_tokens: Set(None),
            trace_sequence: Set(0),
            created_at: Set(1),
            updated_at: Set(1),
            archived: Set(0),
            ..Default::default()
        }
        .insert(store.database())
        .await
        .unwrap();
    }

    async fn insert_turn(store: &StudioStore, thread_id: &str, turn_id: &str) {
        let state =
            TurnState::Completed(CompletedTurnState::new(Some(1), 2, TurnCompletion::Normal));
        crate::studio::entity::turn::ActiveModel {
            id: Set(turn_id.to_string()),
            thread_id: Set(thread_id.to_string()),
            ordinal: Set(1),
            revision: Set(1),
            state_json: Set(serde_json::to_string(&state).unwrap()),
            state_kind: sea_orm::ActiveValue::NotSet,
            model_json: Set(None),
            usage_json: Set(
                serde_json::to_string(&pl_protocol::InferenceTokenUsage::default()).unwrap(),
            ),
            metadata_json: Set(None),
            updated_at: Set(2),
        }
        .insert(store.database())
        .await
        .unwrap();
    }

    async fn insert_item(
        store: &StudioStore,
        thread_id: &str,
        turn_id: &str,
        ordinal: i64,
        state: ThreadItemState,
    ) {
        item::ActiveModel {
            id: Set(format!("item-{ordinal}")),
            thread_id: Set(thread_id.to_string()),
            turn_id: Set(turn_id.to_string()),
            ordinal: Set(ordinal),
            revision: Set(1),
            state_json: Set(serde_json::to_string(&state).unwrap()),
            state_kind: sea_orm::ActiveValue::NotSet,
            created_at: Set(ordinal),
            updated_at: Set(ordinal),
        }
        .insert(store.database())
        .await
        .unwrap();
    }
}
