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

/// Reads a bounded, bidirectional item page from the durable per-Thread history database.
pub async fn list_timeline_items(
    request: super::super::types::ListTimelineItemsRequest,
) -> Result<super::super::types::BridgeTimelinePage, BridgeError> {
    use super::super::types::{
        BridgeTimelineItemPreview, BridgeTimelinePage, BridgeTimelineQuery, BridgeTimelineTurn,
    };
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
        database_id: page.database_id,
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
        truncated: page.truncated,
        previews: page
            .previews
            .into_iter()
            .map(|entry| BridgeTimelineItemPreview {
                item_id: entry.item_id,
                ordinal: entry.ordinal,
                revision: entry.revision,
                total_bytes: entry.total_bytes,
                preview_bytes: entry.preview_bytes,
                omitted_bytes: entry.omitted_bytes,
            })
            .collect(),
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

/// 按 item identity 直接读取一条完整条目正文，绕过页面的单条预览预算。
///
/// 返回与分页一致的 item、database identity 与 watermark，使客户端能按 identity 把
/// 完整载荷合并进既有窗口并替换同身份的预览条目。
pub async fn read_timeline_item(
    thread_id: String,
    item_id: String,
) -> Result<super::super::types::BridgeTimelinePage, BridgeError> {
    let bridge = active_bridge().await?;
    let read = bridge
        .studio
        .read_timeline_item(&thread_id, &item_id)
        .await?;
    let item = bridge_thread_item(read.item)?;
    Ok(super::super::types::BridgeTimelinePage {
        thread_id: read.thread_id,
        database_id: read.database_id,
        watermark: read.watermark,
        first_item_id: item.as_ref().map(|item| item.id.clone()),
        last_item_id: item.as_ref().map(|item| item.id.clone()),
        items: item.into_iter().collect(),
        older_cursor: None,
        newer_cursor: None,
        truncated: false,
        previews: Vec::new(),
        turns: Vec::new(),
    })
}

/// Searches the full project/session directory, including archived history.
pub async fn query_threads(
    request: crate::api::studio::types::BridgeDirectoryQuery,
) -> Result<BridgeThreadDirectoryPage, BridgeError> {
    let bridge = active_bridge().await?;
    let filter = match request.filter {
        crate::api::studio::types::BridgeDirectoryFilter::All => {
            pl_protocol::studio::ThreadDirectoryFilter::All
        }
        crate::api::studio::types::BridgeDirectoryFilter::Running => {
            pl_protocol::studio::ThreadDirectoryFilter::Running
        }
        crate::api::studio::types::BridgeDirectoryFilter::Attention => {
            pl_protocol::studio::ThreadDirectoryFilter::Attention
        }
    };
    let page = bridge
        .studio
        .query_threads(
            &pl_protocol::studio::ThreadDirectoryQuery {
                project_id: request.project_id,
                search: request.search,
                archived: request.archived,
                filter,
            },
            request.cursor.as_deref(),
            request.limit as usize,
        )
        .await?;
    Ok(bridge_thread_directory_page(page.state))
}
