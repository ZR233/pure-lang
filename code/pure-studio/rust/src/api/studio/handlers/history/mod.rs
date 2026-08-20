use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::thread_stream::{
    bridge_thread, bridge_thread_item, bridge_thread_snapshot, bridge_turn,
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
    Ok(BridgeThreadDirectoryPage {
        meta: page.meta.into(),
        threads: page.threads.into_iter().map(bridge_thread).collect(),
        next_cursor: page.next_cursor,
    })
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
