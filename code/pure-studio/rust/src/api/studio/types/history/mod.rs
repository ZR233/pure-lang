use super::BridgeSessionEventEnvelope;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadSessionHistoryPageRequest {
    pub session_id: String,
    pub before_turn_sequence: Option<i64>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadSessionHistoryPageResponse {
    pub turns: Vec<BridgeSessionHistoryTurn>,
    pub next_before_turn_sequence: Option<i64>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeSessionHistoryTurn {
    pub turn_sequence: i64,
    pub turn_id: String,
    pub status: String,
    pub model_json: Option<String>,
    pub error_json: Option<String>,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub items: Vec<BridgeSessionHistoryItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeSessionHistoryItem {
    pub sequence: i64,
    pub item_id: String,
    pub turn_id: String,
    pub item_kind: String,
    pub payload: BridgeSessionEventEnvelope,
    pub created_at: i64,
}
