use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::records::thread_from_record;
use crate::api::studio::convert::thread_stream::{
    bridge_thread, bridge_thread_item, bridge_thread_snapshot, bridge_turn,
};
use crate::api::studio::types::{
    BridgeError, BridgeThread, BridgeThreadContextDisposition, BridgeThreadSnapshot,
    BridgeThreadTurnHistory, BridgeThreadTurnPage, ListThreadTurnsRequest,
};

pub async fn list_threads(project_id: String) -> Result<Vec<BridgeThread>, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge
        .studio
        .store()
        .list_threads(&project_id)
        .await?
        .into_iter()
        .map(thread_from_record)
        .map(bridge_thread)
        .collect())
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
