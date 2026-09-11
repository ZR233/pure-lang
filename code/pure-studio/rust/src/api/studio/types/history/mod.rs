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
    pub watermark: u64,
    pub items: Vec<BridgeThreadItem>,
    pub older_cursor: Option<String>,
    pub newer_cursor: Option<String>,
    pub first_item_id: Option<String>,
    pub last_item_id: Option<String>,
    pub turns: Vec<BridgeTimelineTurn>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeTimelineTurn {
    pub turn: BridgeTurn,
    pub last_item_id: String,
    pub context_disposition: BridgeThreadContextDisposition,
}
