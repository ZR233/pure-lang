//! LSP server 可用性与 Available 内活动状态。

mod available;
mod checking;
mod disabled;
mod unavailable;

pub use available::{LspAvailable, LspAvailableActivity, LspBusy, LspIdle, LspIndexing};
pub use checking::LspChecking;
pub use disabled::LspDisabled;
pub use unavailable::LspUnavailable;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioLspServerState {
    Checking(LspChecking),
    Available(LspAvailable),
    Unavailable(LspUnavailable),
    Disabled(LspDisabled),
}

#[cfg(test)]
mod tests {
    use pl_protocol::StateError;
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn all_lsp_server_states_round_trip() {
        let states = [
            StudioLspServerState::Checking(LspChecking::new("checking")),
            StudioLspServerState::Available(LspAvailable::new(
                10,
                3,
                LspAvailableActivity::Idle(LspIdle),
            )),
            StudioLspServerState::Available(LspAvailable::new(
                11,
                4,
                LspAvailableActivity::Busy(LspBusy::new(
                    Some("Rust Analyzer".to_string()),
                    Some("checking".to_string()),
                    Some(50),
                )),
            )),
            StudioLspServerState::Available(LspAvailable::new(
                12,
                5,
                LspAvailableActivity::Indexing(LspIndexing::new(
                    None,
                    Some("indexing".to_string()),
                    Some(75),
                )),
            )),
            StudioLspServerState::Unavailable(LspUnavailable::new(
                13,
                StateError {
                    code: "lspCommandMissing".to_string(),
                    message: "rust-analyzer missing".to_string(),
                    retryable: false,
                },
            )),
            StudioLspServerState::Disabled(LspDisabled::new("disabled")),
        ];

        for state in states {
            let json = serde_json::to_string(&state).expect("serialize LSP state");
            let restored = serde_json::from_str(&json).expect("deserialize LSP state");
            assert_eq!(state, restored);
        }
    }

    #[test]
    fn lsp_server_state_rejects_legacy_or_cross_state_fields() {
        let legacy = serde_json::json!({
            "availability": "available",
            "activity": "busy",
            "progressTitle": "checking"
        });
        let illegal_unavailable = serde_json::json!({
            "kind": "unavailable",
            "data": {
                "checkedAt": 10,
                "error": {"code": "offline", "message": "offline", "retryable": true},
                "activity": {"kind": "idle", "data": {}}
            }
        });

        assert!(serde_json::from_value::<StudioLspServerState>(legacy).is_err());
        assert!(serde_json::from_value::<StudioLspServerState>(illegal_unavailable).is_err());
    }
}
