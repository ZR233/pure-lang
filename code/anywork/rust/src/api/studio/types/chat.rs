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

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeChatItem {
    pub item: BridgeThreadItem,
    pub saved: bool,
    /// Bytes omitted from the canonical body in a bounded window preview.
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
    AppendText {
        item_id: String,
        part_id: String,
        expected_revision: u64,
        revision: u64,
        text: String,
    },
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
    },
}
