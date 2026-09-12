use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SshAuthKindDto {
    AgentOrKey,
    Password,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SaveSshServerRequest {
    pub id: Option<String>,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_kind: SshAuthKindDto,
    pub identity_file: Option<String>,
    /// 仅用于更新 core 内存 secret lease，不会进入返回 DTO 或 SQLite。
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SshServerDto {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_kind: SshAuthKindDto,
    pub identity_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SshConnectionSnapshotDto {
    pub server_id: String,
    pub state: String,
    pub helper_version: Option<String>,
    pub architecture: Option<String>,
    pub attempt: Option<u32>,
    pub delay_seconds: Option<u64>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDirectoryListingDto {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<RemoteDirectoryEntryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDirectoryEntryDto {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
}
