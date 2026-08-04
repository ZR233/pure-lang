use anyhow::Context;

use crate::api::studio::convert::session_stream::session_event;
use crate::api::studio::runtime::active_bridge;
use crate::api::studio::types::{
    BridgeError, BridgeSessionHistoryItem, BridgeSessionHistoryTurn, LoadSessionHistoryPageRequest,
    LoadSessionHistoryPageResponse,
};

pub async fn load_session_history_page(
    request: LoadSessionHistoryPageRequest,
) -> Result<LoadSessionHistoryPageResponse, BridgeError> {
    let bridge = active_bridge().await?;
    bridge
        .studio
        .store()
        .read_session(&request.session_id)
        .await?
        .context("selected session not found")?;
    let page = bridge
        .studio
        .load_session_history_page(
            &request.session_id,
            request.before_turn_sequence,
            usize::try_from(request.limit).map_err(anyhow::Error::from)?,
        )
        .await?;
    let turns = page
        .turns
        .into_iter()
        .map(|turn| {
            let items = turn
                .items
                .into_iter()
                .map(|item| {
                    Ok(BridgeSessionHistoryItem {
                        sequence: item.sequence,
                        item_id: item.item_id,
                        turn_id: item.turn_id,
                        item_kind: item.item_kind,
                        payload: session_event(item.payload)?,
                        created_at: item.created_at,
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            Ok(BridgeSessionHistoryTurn {
                turn_sequence: turn.turn_sequence,
                turn_id: turn.turn_id,
                status: turn.status,
                model_json: turn.model.map(|model| model.to_string()),
                error_json: turn.error.map(|error| error.to_string()),
                started_at: turn.started_at,
                completed_at: turn.completed_at,
                items,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(LoadSessionHistoryPageResponse {
        turns,
        next_before_turn_sequence: page.next_before_turn_sequence,
        has_more: page.has_more,
    })
}
