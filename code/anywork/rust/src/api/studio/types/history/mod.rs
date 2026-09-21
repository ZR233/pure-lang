use super::{BridgeThreadItem, BridgeTurn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeThreadContextDisposition {
    Active,
    RolledBack,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListThreadTurnsRequest {
    pub thread_id: String,
    pub cursor: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeThreadTurnPage {
    pub turns: Vec<BridgeThreadTurnHistory>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeThreadTurnHistory {
    pub turn: BridgeTurn,
    pub items: Vec<BridgeThreadItem>,
    pub context_disposition: BridgeThreadContextDisposition,
}

/// Item-oriented Timeline query, independent from complete-Turn history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeTimelineQuery {
    Latest,
    Before { item_id: String },
    After { item_id: String },
    Around { item_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListTimelineItemsRequest {
    pub thread_id: String,
    pub query: BridgeTimelineQuery,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeTimelinePage {
    pub thread_id: String,
    /// 本页来自哪个 history 数据库实体；用于校验游标身份。
    pub database_id: String,
    pub watermark: u64,
    pub items: Vec<BridgeThreadItem>,
    pub older_cursor: Option<String>,
    pub newer_cursor: Option<String>,
    pub first_item_id: Option<String>,
    pub last_item_id: Option<String>,
    /// 是否因为总字节预算而在条目上限之前截断。
    pub truncated: bool,
    /// 因超过单条预览预算而只以预览返回的条目引用；身份与 ordinal 不变。
    pub previews: Vec<BridgeTimelineItemPreview>,
    pub turns: Vec<BridgeTimelineTurn>,
}

/// 一条超大条目在页面中只以预览呈现时的显式引用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeTimelineItemPreview {
    pub item_id: String,
    pub ordinal: u64,
    pub revision: u64,
    pub total_bytes: u64,
    pub preview_bytes: u64,
    pub omitted_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeTimelineTurn {
    pub turn: BridgeTurn,
    pub last_item_id: String,
    pub context_disposition: BridgeThreadContextDisposition,
}
