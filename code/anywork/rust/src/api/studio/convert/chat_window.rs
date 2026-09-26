//! ChatView 内容窗口 → FRB DTO 的映射。
//!
//! 窗口差量本身（基线一致性、失同步回退成权威 `Reset`、typed 字段增量）只有一处实现：runtime 的
//! `pl_studio_runtime::ChatWindowStream`。这个模块**只**做 typed 结果到 FRB DTO 的映射：不复制任何
//! 内容差量逻辑，不持有第二份内容索引，也不自己重建窗口基线。HTTP 与 FRB 因此共享同一条转换责任与
//! 同一套版本语义。

use pl_protocol::{
    ChatWindowChange, ChatWindowDirection, ChatWindowFocus, ChatWindowItem, ChatWindowLifecycle,
    ChatWindowPriority, ChatWindowQuery, ChatWindowSnapshot, ChatWindowUpdate, ThreadContentField,
    ThreadFieldChange, ThreadFieldUpdate,
};

use crate::api::studio::convert::thread_stream::bridge_chat_thread_item;
use crate::api::studio::types::{
    BridgeChatDirection, BridgeChatFocus, BridgeChatItem, BridgeChatLifecycle, BridgeChatSnapshot,
    BridgeChatUpdate, BridgeContentField, BridgeError, BridgeFieldChange, BridgeFieldUpdate,
    BridgeUpdatePriority, BridgeViewChange,
};

pub(crate) fn snapshot(value: ChatWindowSnapshot) -> Result<BridgeChatSnapshot, BridgeError> {
    Ok(BridgeChatSnapshot {
        focus: focus(value.focus),
        version: value.version,
        items: value
            .items
            .into_iter()
            .map(item)
            .collect::<Result<_, _>>()?,
        has_older: value.has_older,
        has_newer: value.has_newer,
    })
}

pub(crate) fn update(value: ChatWindowUpdate) -> Result<BridgeChatUpdate, BridgeError> {
    Ok(match value {
        ChatWindowUpdate::Reset { window } => BridgeChatUpdate::Reset {
            snapshot: snapshot(window)?,
        },
        ChatWindowUpdate::Patch {
            from,
            to,
            changes,
            has_newer,
            priority,
        } => BridgeChatUpdate::Patch {
            from,
            to,
            changes: changes.into_iter().map(change).collect::<Result<_, _>>()?,
            has_newer,
            priority: priority_dto(priority),
        },
    })
}

pub(crate) fn item(value: ChatWindowItem) -> Result<BridgeChatItem, BridgeError> {
    Ok(BridgeChatItem {
        item: bridge_chat_thread_item(value.item)?,
        saved: value.saved,
        lifecycle: lifecycle_dto(value.lifecycle),
        omitted_bytes: value.omitted_bytes,
    })
}

/// FRB 窗口焦点 → 共享窗口请求。
///
/// FRB 与 HTTP 用同一个请求形状：`focus` 只表达“跳到某条历史或最新”，翻页交给 `load` 携带的方向。
pub(crate) fn query(value: BridgeChatFocus) -> ChatWindowQuery {
    ChatWindowQuery {
        anchor: match value {
            BridgeChatFocus::Latest => None,
            BridgeChatFocus::Around { item_id } => Some(item_id),
        },
        direction: None,
    }
}

pub(crate) fn direction(value: BridgeChatDirection) -> ChatWindowDirection {
    match value {
        BridgeChatDirection::Older => ChatWindowDirection::Older,
        BridgeChatDirection::Newer => ChatWindowDirection::Newer,
    }
}

fn change(value: ChatWindowChange) -> Result<BridgeViewChange, BridgeError> {
    Ok(match value {
        ChatWindowChange::Splice {
            index,
            remove,
            items,
        } => BridgeViewChange::Splice {
            index,
            remove,
            items: items.into_iter().map(item).collect::<Result<_, _>>()?,
        },
        ChatWindowChange::UpdateItem {
            item_id,
            expected_revision,
            revision,
            omitted_bytes,
            saved,
            fields,
        } => BridgeViewChange::UpdateItem {
            item_id,
            expected_revision,
            revision,
            omitted_bytes,
            saved,
            fields: fields.into_iter().map(field_update).collect(),
        },
    })
}

fn field_update(value: ThreadFieldUpdate) -> BridgeFieldUpdate {
    BridgeFieldUpdate {
        field: content_field(value.field),
        change: field_change(value.change),
    }
}

fn field_change(value: ThreadFieldChange) -> BridgeFieldChange {
    match value {
        ThreadFieldChange::Unchanged => BridgeFieldChange::Unchanged,
        ThreadFieldChange::Append { text } => BridgeFieldChange::Append { text },
        ThreadFieldChange::Replace { text } => BridgeFieldChange::Replace { text },
        ThreadFieldChange::Remove => BridgeFieldChange::Remove,
    }
}

fn focus(value: ChatWindowFocus) -> BridgeChatFocus {
    match value {
        ChatWindowFocus::Latest => BridgeChatFocus::Latest,
        ChatWindowFocus::Around { item_id } => BridgeChatFocus::Around { item_id },
    }
}

fn lifecycle_dto(value: ChatWindowLifecycle) -> BridgeChatLifecycle {
    match value {
        ChatWindowLifecycle::Streaming => BridgeChatLifecycle::Streaming,
        ChatWindowLifecycle::Terminal => BridgeChatLifecycle::Terminal,
    }
}

fn priority_dto(value: ChatWindowPriority) -> BridgeUpdatePriority {
    match value {
        ChatWindowPriority::Immediate => BridgeUpdatePriority::Immediate,
        ChatWindowPriority::Coalesced => BridgeUpdatePriority::Coalesced,
    }
}

fn content_field(field: ThreadContentField) -> BridgeContentField {
    match field {
        ThreadContentField::Text => BridgeContentField::Text,
        ThreadContentField::ThinkingSummary { chunk_index } => {
            BridgeContentField::ThinkingSummary { chunk_index }
        }
        ThreadContentField::ThinkingContent { chunk_index } => {
            BridgeContentField::ThinkingContent { chunk_index }
        }
        ThreadContentField::ToolArguments => BridgeContentField::ToolArguments,
        ThreadContentField::ToolResult => BridgeContentField::ToolResult,
    }
}
