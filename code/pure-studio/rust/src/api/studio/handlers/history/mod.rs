use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::runtime::bridge_thread_directory_page;
use crate::api::studio::convert::thread_stream::{
    bridge_thread_item, bridge_thread_snapshot, bridge_turn,
};
use crate::api::studio::types::{
    BridgeError, BridgeListThreadsPageRequest, BridgeThreadContextDisposition,
    BridgeThreadDirectoryPage, BridgeThreadSnapshot, BridgeThreadTurnHistory, BridgeThreadTurnPage,
    ListThreadTurnsRequest,
};

/// 从内存目录索引按 `(updatedAt, id)` 倒序 keyset 分页；GUI 触底加载使用。
pub async fn list_threads_page(
    request: BridgeListThreadsPageRequest,
) -> Result<BridgeThreadDirectoryPage, BridgeError> {
    let bridge = active_bridge().await?;
    let page = bridge
        .studio
        .list_threads_page(
            request.cursor.as_deref(),
            usize::try_from(request.limit).map_err(anyhow::Error::from)?,
        )
        .await?;
    Ok(bridge_thread_directory_page(page.state))
}

pub async fn read_thread(thread_id: String) -> Result<BridgeThreadSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_thread_snapshot(
        bridge.studio.thread_snapshot(&thread_id).await?,
    )?)
}

pub async fn list_thread_turns(
    request: ListThreadTurnsRequest,
) -> Result<BridgeThreadTurnPage, BridgeError> {
    let bridge = active_bridge().await?;
    let page = bridge
        .studio
        .list_thread_turns(
            &request.thread_id,
            request.cursor.as_deref(),
            usize::try_from(request.limit).map_err(anyhow::Error::from)?,
        )
        .await?;
    let turns = page
        .turns
        .into_iter()
        .map(|history| {
            let items = history
                .items
                .into_iter()
                .map(bridge_thread_item)
                .collect::<anyhow::Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect();
            Ok(BridgeThreadTurnHistory {
                turn: bridge_turn(history.turn),
                items,
                context_disposition: match history.context_disposition {
                    pl_protocol::ThreadContextDisposition::Active => {
                        BridgeThreadContextDisposition::Active
                    }
                    pl_protocol::ThreadContextDisposition::RolledBack => {
                        BridgeThreadContextDisposition::RolledBack
                    }
                },
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(BridgeThreadTurnPage {
        turns,
        next_cursor: page.next_cursor,
    })
}

/// Reads a bounded, bidirectional item page from the canonical journal projection.
pub async fn list_timeline_items(
    request: super::super::types::ListTimelineItemsRequest,
) -> Result<super::super::types::BridgeTimelinePage, BridgeError> {
    use super::super::types::{BridgeTimelinePage, BridgeTimelineQuery, BridgeTimelineTurn};
    let bridge = active_bridge().await?;
    let query = match request.query {
        BridgeTimelineQuery::Latest => pl_protocol::TimelineQuery::Latest,
        BridgeTimelineQuery::Before { item_id } => pl_protocol::TimelineQuery::Before { item_id },
        BridgeTimelineQuery::After { item_id } => pl_protocol::TimelineQuery::After { item_id },
        BridgeTimelineQuery::Around { item_id } => pl_protocol::TimelineQuery::Around { item_id },
    };
    let page = bridge
        .studio
        .list_timeline_items(&request.thread_id, query, request.limit as usize)
        .await?;
    Ok(BridgeTimelinePage {
        thread_id: page.thread_id,
        watermark: page.watermark,
        items: page
            .items
            .into_iter()
            .map(bridge_thread_item)
            .collect::<anyhow::Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect(),
        first_item_id: page.first_item_id,
        last_item_id: page.last_item_id,
        older_cursor: page.older_cursor,
        newer_cursor: page.newer_cursor,
        turns: page
            .turns
            .into_iter()
            .map(|entry| BridgeTimelineTurn {
                turn: bridge_turn(entry.turn),
                last_item_id: entry.last_item_id,
                context_disposition: match entry.context_disposition {
                    pl_protocol::ThreadContextDisposition::Active => {
                        BridgeThreadContextDisposition::Active
                    }
                    pl_protocol::ThreadContextDisposition::RolledBack => {
                        BridgeThreadContextDisposition::RolledBack
                    }
                },
            })
            .collect(),
    })
}
