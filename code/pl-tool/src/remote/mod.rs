//! SSH remote helper 的本地管理、协议客户端与工具宿主。

use crate::environment::ExecutionEnvironment;

mod client;
mod codec;
mod command;
mod execution;
mod lsp;
mod manager;
pub(crate) mod path;
mod skill;
mod tools;
mod workspace;

pub use client::{RemoteClient, RemoteClientError, RemoteReply};
pub use command::RemoteCommandBackend;
pub use execution::RemoteExecutionBackend;
pub use manager::{
    RemoteHelperAssets, RemoteHelperTarget, SshAuth, SshConnectionSnapshot, SshConnectionState,
    SshManager, SshServerProfile,
};
pub use skill::RemoteSkillProvider;
pub use tools::{RemoteMutationKind, RemoteWorkspaceMutationTool};
pub use workspace::{
    RemoteDownloadError, RemoteWorkspaceFileBackend, load_remote_workspace_instructions,
};

/// 一个远端 workspace 的本地后端集合。
///
/// 该类型只暴露环境原语；工具名与 schema 由 pl-tool 提供，权限策略与 Turn 装配由宿主负责。
#[derive(Debug, Clone)]
pub struct RemoteWorkspaceHost {
    pub files: RemoteWorkspaceFileBackend,
    pub commands: RemoteCommandBackend,
    pub git: RemoteExecutionBackend,
    pub execution_environment: ExecutionEnvironment,
}

pub(crate) use lsp::resolve_lsp_query_path;
