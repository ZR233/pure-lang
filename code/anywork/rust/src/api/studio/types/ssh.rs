use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SaveSshServerRequest {
    /// Host 别名即服务器身份；创建后不可改名，编辑时必须与现有别名一致。
    pub alias: String,
    pub host_name: String,
    pub port: u16,
    pub username: String,
    pub identity_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SshServerDto {
    pub alias: String,
    pub host_name: String,
    pub port: u16,
    pub username: String,
    pub identity_file: Option<String>,
    /// 是否为 anywork 管理块；手写条目只读，不可编辑或删除。
    pub managed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SshConnectionSnapshotDto {
    pub alias: String,
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
