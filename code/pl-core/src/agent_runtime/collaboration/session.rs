//! `read_agent_session` 的 cursor 与 durable Timeline 页投影。

use base64::Engine as _;
use pl_protocol::{AgentSessionPage, AgentSessionReadDetail, AgentSessionReadOrder, PureError};
use serde::{Deserialize, Serialize};

use super::super::{
    AgentSessionTimelineKey, AgentSessionTimelineQuery, AgentSessionTimelineRepositoryPage,
    ThreadId,
};
use super::TOOL_READ_AGENT_SESSION;
use crate::tool::tool_error;

const CURSOR_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionCursor {
    version: u8,
    target: ThreadId,
    order: AgentSessionReadOrder,
    detail: AgentSessionReadDetail,
    through_sequence: u64,
    watermark: AgentSessionTimelineKey,
    anchor: AgentSessionTimelineKey,
}

pub(super) fn query(
    target: ThreadId,
    order: AgentSessionReadOrder,
    detail: AgentSessionReadDetail,
    limit: usize,
    cursor: Option<&str>,
) -> Result<AgentSessionTimelineQuery, PureError> {
    let cursor = cursor
        .map(str::trim)
        .filter(|cursor| !cursor.is_empty())
        .map(decode_cursor)
        .transpose()?;
    if let Some(cursor) = &cursor
        && (cursor.target != target || cursor.order != order || cursor.detail != detail)
    {
        return Err(tool_error(
            TOOL_READ_AGENT_SESSION,
            "cursor does not belong to this target, order, and detail query".to_string(),
        ));
    }
    Ok(AgentSessionTimelineQuery {
        target,
        order,
        detail,
        limit,
        through_sequence: cursor.as_ref().map(|cursor| cursor.through_sequence),
        watermark: cursor.as_ref().map(|cursor| cursor.watermark.clone()),
        anchor: cursor.map(|cursor| cursor.anchor),
    })
}

pub(super) fn page(
    query: &AgentSessionTimelineQuery,
    repository: AgentSessionTimelineRepositoryPage,
) -> Result<AgentSessionPage, PureError> {
    if repository.identity.id != query.target {
        return Err(tool_error(
            TOOL_READ_AGENT_SESSION,
            format!(
                "repository returned agent `{}` for requested target `{}`",
                repository.identity.id, query.target
            ),
        ));
    }
    let next_cursor = match (
        repository.has_more,
        repository.watermark.as_ref(),
        repository.next_anchor.as_ref(),
    ) {
        (true, Some(watermark), Some(anchor)) => Some(encode_cursor(&SessionCursor {
            version: CURSOR_VERSION,
            target: query.target.clone(),
            order: query.order,
            detail: query.detail,
            through_sequence: repository.through_sequence,
            watermark: watermark.clone(),
            anchor: anchor.clone(),
        })?),
        (true, _, _) => {
            return Err(tool_error(
                TOOL_READ_AGENT_SESSION,
                "repository returned an incomplete Timeline continuation".to_string(),
            ));
        }
        (false, _, _) => None,
    };
    Ok(AgentSessionPage {
        agent_id: repository.identity.id,
        path: repository.path,
        through_sequence: repository.through_sequence,
        order: query.order,
        detail: query.detail,
        items: repository.items,
        has_more: repository.has_more,
        next_cursor,
    })
}

pub(super) fn page_with_budget(
    query: &AgentSessionTimelineQuery,
    mut repository: AgentSessionTimelineRepositoryPage,
    max_bytes: usize,
) -> Result<AgentSessionPage, PureError> {
    loop {
        let candidate = page(query, repository.clone())?;
        let serialized_bytes = serde_json::to_vec(&candidate)
            .map_err(|error| {
                tool_error(
                    TOOL_READ_AGENT_SESSION,
                    format!("failed to measure Timeline page: {error}"),
                )
            })?
            .len();
        if serialized_bytes <= max_bytes {
            return Ok(candidate);
        }
        if repository.items.len() <= 1 {
            let item_id = repository
                .items
                .first()
                .map_or("unknown", |item| item.id.as_str());
            return Err(tool_error(
                TOOL_READ_AGENT_SESSION,
                format!(
                    "Timeline item `{item_id}` exceeds the single-page output budget; use detail=text when possible"
                ),
            ));
        }
        repository.items.pop();
        repository.has_more = true;
        repository.next_anchor = repository.items.last().map(|item| AgentSessionTimelineKey {
            ordinal: item.ordinal,
            item_id: item.id.clone(),
        });
    }
}

fn decode_cursor(value: &str) -> Result<SessionCursor, PureError> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| invalid_cursor())?;
    let cursor: SessionCursor = serde_json::from_slice(&bytes).map_err(|_| invalid_cursor())?;
    if cursor.version != CURSOR_VERSION || cursor.anchor.ordinal > cursor.watermark.ordinal {
        return Err(invalid_cursor());
    }
    Ok(cursor)
}

fn encode_cursor(cursor: &SessionCursor) -> Result<String, PureError> {
    let bytes = serde_json::to_vec(cursor).map_err(|error| {
        tool_error(
            TOOL_READ_AGENT_SESSION,
            format!("failed to encode Timeline cursor: {error}"),
        )
    })?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

fn invalid_cursor() -> PureError {
    tool_error(
        TOOL_READ_AGENT_SESSION,
        "invalid cursor; omit cursor on the first page or pass the exact nextCursor returned by the previous read_agent_session call".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use pl_protocol::AgentIdentity;

    use super::*;
    use crate::AgentRoleId;

    #[test]
    fn cursor_is_bound_to_target_order_and_detail() {
        let target = ThreadId::new("child").unwrap();
        let request = AgentSessionTimelineQuery {
            target: target.clone(),
            order: AgentSessionReadOrder::Descending,
            detail: AgentSessionReadDetail::Text,
            limit: 1,
            through_sequence: None,
            watermark: None,
            anchor: None,
        };
        let first = page(
            &request,
            AgentSessionTimelineRepositoryPage {
                identity: AgentIdentity {
                    id: target.clone(),
                    parent_id: Some(ThreadId::new("root").unwrap()),
                    role: AgentRoleId::new("executor").unwrap(),
                    depth: 1,
                },
                path: vec![ThreadId::new("root").unwrap(), target.clone()],
                through_sequence: 9,
                watermark: Some(AgentSessionTimelineKey {
                    ordinal: 8,
                    item_id: "item-8".to_string(),
                }),
                items: Vec::new(),
                has_more: true,
                next_anchor: Some(AgentSessionTimelineKey {
                    ordinal: 7,
                    item_id: "item-7".to_string(),
                }),
            },
        )
        .unwrap();
        let cursor = first.next_cursor.expect("next cursor");

        assert!(
            query(
                target.clone(),
                AgentSessionReadOrder::Descending,
                AgentSessionReadDetail::Text,
                1,
                Some(&cursor),
            )
            .is_ok()
        );
        assert!(
            query(
                target,
                AgentSessionReadOrder::Ascending,
                AgentSessionReadDetail::Text,
                1,
                Some(&cursor),
            )
            .unwrap_err()
            .to_string()
            .contains("does not belong")
        );
    }

    #[test]
    fn malformed_cursor_has_actionable_guidance() {
        let error = query(
            ThreadId::new("child").unwrap(),
            AgentSessionReadOrder::default(),
            AgentSessionReadDetail::default(),
            20,
            Some("not-a-cursor"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("exact nextCursor"));
    }
}
