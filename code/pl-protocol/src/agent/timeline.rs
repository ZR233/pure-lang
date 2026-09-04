//! 子 Agent durable Timeline 的分页查询协议。

use serde::{Deserialize, Serialize};

use crate::{ThreadId, ThreadItem};

/// `read_agent_session` 返回 Item 的排列方向。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentSessionReadOrder {
    Ascending,
    #[default]
    Descending,
}

/// `read_agent_session` 的 Timeline 详细级别。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentSessionReadDetail {
    #[default]
    Text,
    Full,
}

/// `read_agent_session` 的稳定 keyset 分页结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentSessionPage {
    pub agent_id: ThreadId,
    pub path: Vec<ThreadId>,
    pub through_sequence: u64,
    pub order: AgentSessionReadOrder,
    pub detail: AgentSessionReadDetail,
    pub items: Vec<ThreadItem>,
    pub has_more: bool,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::*;

    #[test]
    fn read_defaults_are_latest_text_items() {
        assert_eq!(
            serde_json::to_value(AgentSessionReadOrder::default()).unwrap(),
            json!("descending")
        );
        assert_eq!(
            serde_json::to_value(AgentSessionReadDetail::default()).unwrap(),
            json!("text")
        );
    }
}
