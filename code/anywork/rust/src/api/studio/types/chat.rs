use super::BridgeThreadItem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeChatFocus {
    Latest,
    Around { item_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeChatDirection {
    Older,
    Newer,
}

/// 条目的执行终态事实，与保存水位 `saved` 严格独立。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeChatLifecycle {
    Streaming,
    Terminal,
}

/// 一帧是否需要立即应用，还是普通文本可以合帧。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeUpdatePriority {
    Immediate,
    Coalesced,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeChatItem {
    pub item: BridgeThreadItem,
    pub saved: bool,
    pub lifecycle: BridgeChatLifecycle,
    /// Bytes omitted from the bounded preview; `> 0` 时用 `readComplete` 按身份取回完整正文。
    pub omitted_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeChatSnapshot {
    pub focus: BridgeChatFocus,
    pub version: u64,
    pub items: Vec<BridgeChatItem>,
    pub has_older: bool,
    pub has_newer: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeViewChange {
    Splice {
        index: u64,
        remove: u64,
        items: Vec<BridgeChatItem>,
    },
    /// 同一身份的全部内容字段，在**一次** `expectedRevision`/`revision` 提交里原子应用。
    ///
    /// 应用条件：本地该 `itemId` 的 content revision 等于 `expectedRevision`；否则按宿主策略重同步。
    /// 全部字段应用完成后**一次**提交 `revision`、`omittedBytes` 与保存水位 `saved`——即使字段全为
    /// `unchanged`（仅版本推进、仅保存确认）也必须提交，不能当成空 Patch 丢弃。
    UpdateItem {
        item_id: String,
        expected_revision: u64,
        revision: u64,
        omitted_bytes: u64,
        saved: bool,
        fields: Vec<BridgeFieldUpdate>,
    },
}

/// 一个内容字段的变化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeFieldChange {
    /// 正文不变，但所属 item 的 revision/omitted 仍要提交（版本帧不是空 Patch）。
    Unchanged,
    /// 该字段仍以前缀方式扩展本地已交付末尾；`text` 是新增字节。
    Append { text: String },
    /// 本地基线已失效（权威替换、预览升级）；`text` 是整段权威正文。
    Replace { text: String },
    /// 该字段已从条目移除，消费者丢弃本地副本。
    Remove,
}

/// 同一 item 的一个字段变化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeFieldUpdate {
    pub field: BridgeContentField,
    pub change: BridgeFieldChange,
}

/// 内容增量落在哪个字段上（只由字段身份决定，不从内容字符串推断）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeContentField {
    Text,
    ThinkingSummary { chunk_index: u32 },
    ThinkingContent { chunk_index: u32 },
    ToolArguments,
    ToolResult,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeChatUpdate {
    Reset {
        snapshot: BridgeChatSnapshot,
    },
    Patch {
        from: u64,
        to: u64,
        changes: Vec<BridgeViewChange>,
        has_newer: bool,
        priority: BridgeUpdatePriority,
    },
}
