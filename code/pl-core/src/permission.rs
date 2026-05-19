use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionLevel {
    Ask,
    AcceptEdits,
    Plan,
    Auto,
    Bypass,
}
