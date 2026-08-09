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
