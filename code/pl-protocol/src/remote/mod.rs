//! SSH remote helper 的 transport-neutral wire 类型。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const REMOTE_PROTOCOL_VERSION: u32 = 1;
pub const REMOTE_MAX_HEADER_BYTES: usize = 64 * 1024;
pub const REMOTE_MAX_BODY_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFrameHeader {
    pub request_id: Option<u64>,
    pub message: RemoteMessage,
    pub body_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "payload",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RemoteMessage {
    Request(RemoteRequest),
    Response(RemoteResponse),
    Event(RemoteEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "method",
    content = "params",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RemoteRequest {
    Hello { protocol_version: u32 },
    BrowseDirectories { path: Option<String> },
    OpenWorkspace { path: String },
    CloseWorkspace { workspace_id: String },
    Stat(RemotePathRequest),
    ReadBytes(RemoteReadRequest),
    WriteAtomic(RemotePathRequest),
    ListDirectory(RemotePathRequest),
    CreateDirectory(RemotePathRequest),
    RemovePath(RemoteRemoveRequest),
    RenamePath(RemoteRenameRequest),
    CopyPath(RemoteCopyRequest),
    Spawn(RemoteSpawnRequest),
    WriteStdin { process_id: String },
    CloseStdin { process_id: String },
    Terminate { process_id: String },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "result",
    content = "payload",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RemoteResponse {
    Hello(RemoteHello),
    Directories(RemoteDirectoryListing),
    WorkspaceOpened(RemoteWorkspaceOpened),
    Stat(RemoteFileStat),
    Bytes,
    Directory(RemoteDirectoryListing),
    ProcessSpawned { process_id: String },
    Ack,
    Error(RemoteError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "event",
    content = "payload",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RemoteEvent {
    ProcessOutput(RemoteProcessOutput),
    ProcessExit(RemoteProcessExit),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteHello {
    pub protocol_version: u32,
    pub helper_version: String,
    pub os: String,
    pub architecture: String,
    pub capabilities: Vec<RemoteCapability>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteCapability {
    DirectoryBrowse,
    WorkspaceFiles,
    ObservableExec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteWorkspaceOpened {
    pub workspace_id: String,
    pub canonical_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePathRequest {
    pub workspace_id: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteReadRequest {
    pub workspace_id: String,
    pub path: String,
    pub max_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRemoveRequest {
    pub workspace_id: String,
    pub path: String,
    pub recursive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRenameRequest {
    pub workspace_id: String,
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCopyRequest {
    pub workspace_id: String,
    pub source: String,
    pub target: String,
    pub recursive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSpawnRequest {
    pub process_id: String,
    pub workspace_id: String,
    pub command: String,
    pub cwd: String,
    pub environment: BTreeMap<String, String>,
    pub capture_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFileStat {
    pub path: String,
    pub is_file: bool,
    pub is_directory: bool,
    pub len: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDirectoryListing {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<RemoteDirectoryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDirectoryEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub is_symlink: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProcessOutput {
    pub process_id: String,
    pub sequence: u64,
    pub stream: RemoteOutputStream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteOutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProcessExit {
    pub process_id: String,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteError {
    pub code: RemoteErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteErrorCode {
    InvalidRequest,
    ProtocolMismatch,
    WorkspaceNotFound,
    WorkspaceEscape,
    PathNotFound,
    ProcessNotFound,
    Io,
    Unsupported,
    RemoteDisconnected,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_wire_uses_tagged_camel_case_messages() {
        let value = serde_json::to_value(RemoteMessage::Request(RemoteRequest::Hello {
            protocol_version: REMOTE_PROTOCOL_VERSION,
        }))
        .expect("serialize hello");

        assert_eq!(value["kind"], "request");
        assert_eq!(value["payload"]["method"], "hello");
        assert_eq!(value["payload"]["params"]["protocolVersion"], 1);

        let output = serde_json::to_value(RemoteMessage::Event(RemoteEvent::ProcessOutput(
            RemoteProcessOutput {
                process_id: "process-1".to_string(),
                sequence: 2,
                stream: RemoteOutputStream::Stdout,
            },
        )))
        .expect("serialize output");
        assert_eq!(output["payload"]["payload"]["processId"], "process-1");
    }
}
