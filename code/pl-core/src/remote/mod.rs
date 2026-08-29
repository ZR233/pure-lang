//! SSH remote helper 的本地管理、协议客户端与工具宿主。

mod client;
mod codec;
mod command;
mod execution;
mod lsp;
mod manager;
mod path;
mod skill;
mod tools;
mod workspace;

pub use client::{RemoteClient, RemoteClientError, RemoteReply};
pub use command::RemoteCommandBackend;
pub use execution::RemoteExecutionBackend;
pub use manager::{
    SshAuth, SshConnectionSnapshot, SshConnectionState, SshManager, SshServerProfile,
};
pub use skill::RemoteSkillProvider;
pub use tools::remote_workspace_mutation_tools;
pub use workspace::{RemoteWorkspaceFileBackend, load_remote_workspace_instructions};

/// 一个远端 workspace 的本地后端集合。
///
/// 该类型只暴露环境原语；工具名、schema、权限与 Turn 编排仍由本地 core 拥有。
#[derive(Debug, Clone)]
pub struct RemoteWorkspaceHost {
    pub files: RemoteWorkspaceFileBackend,
    pub commands: RemoteCommandBackend,
    pub git: RemoteExecutionBackend,
}
