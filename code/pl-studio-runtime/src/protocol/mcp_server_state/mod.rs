//! MCP server 可用性状态；variant payload 只包含该状态合法的字段。

mod available;
mod checking;
mod disabled;
mod missing_credential;
mod unavailable;

pub use available::McpAvailable;
pub use checking::McpChecking;
pub use disabled::McpDisabled;
pub use missing_credential::McpMissingCredential;
pub use unavailable::McpUnavailable;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioMcpServerState {
    Disabled(McpDisabled),
    MissingCredential(McpMissingCredential),
    Checking(McpChecking),
    Available(McpAvailable),
    Unavailable(McpUnavailable),
}

#[cfg(test)]
mod tests {
    use pl_protocol::StateError;
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn all_mcp_server_states_round_trip() {
        let states = [
            StudioMcpServerState::Disabled(McpDisabled::new("disabled")),
            StudioMcpServerState::MissingCredential(McpMissingCredential::new("missing key")),
            StudioMcpServerState::Checking(McpChecking::new("checking")),
            StudioMcpServerState::Available(McpAvailable::new(10, 4)),
            StudioMcpServerState::Unavailable(McpUnavailable::new(
                11,
                StateError {
                    code: "mcpUnavailable".to_string(),
                    message: "connection refused".to_string(),
                    retryable: true,
                },
            )),
        ];

        for state in states {
            let json = serde_json::to_string(&state).expect("serialize MCP state");
            let restored = serde_json::from_str(&json).expect("deserialize MCP state");
            assert_eq!(state, restored);
        }
    }

    #[test]
    fn mcp_server_state_rejects_legacy_or_cross_state_fields() {
        let legacy = serde_json::json!({
            "availability": "available",
            "message": "ready",
            "toolCount": 4
        });
        let illegal_disabled = serde_json::json!({
            "kind": "disabled",
            "data": {"message": "disabled", "toolCount": 4}
        });

        assert!(serde_json::from_value::<StudioMcpServerState>(legacy).is_err());
        assert!(serde_json::from_value::<StudioMcpServerState>(illegal_disabled).is_err());
    }
}
