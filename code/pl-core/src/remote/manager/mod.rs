//! 系统 OpenSSH 连接、重连与 remote workspace handle 的本地 owner。

mod asset;
mod ssh;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use pl_protocol::remote::{
    REMOTE_PROTOCOL_VERSION, RemoteDirectoryListing, RemoteHello, RemoteRequest, RemoteResponse,
    RemoteWorkspaceOpened,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tokio::process::Child;
use tokio::sync::{Mutex, RwLock, watch};

use super::{
    RemoteClient, RemoteClientError, RemoteCommandBackend, RemoteExecutionBackend,
    RemoteWorkspaceFileBackend, RemoteWorkspaceHost,
};
use crate::execution_environment::{ExecutionEnvironment, ExecutionOs, ShellDialect};
use pl_protocol::remote::RemoteShellDialect;

pub use self::asset::{RemoteHelperAssets, RemoteHelperTarget};
use self::asset::{file_helper_assets, load_helper, upload_helper};
use self::ssh::{run_ssh_capture, ssh_command, validate_profile};

/// 不含 secret 的 SSH 服务器配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshServerProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: SshAuth,
}

/// SSH 认证来源；密码值始终由独立的进程内 lease 提供。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SshAuth {
    AgentOrKey { identity_file: Option<String> },
    Password,
}

/// 单个 SSH 服务器的 canonical 连接快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshConnectionSnapshot {
    pub server_id: String,
    pub state: SshConnectionState,
}

/// SSH transport 与 helper bootstrap 的穷尽状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SshConnectionState {
    Disconnected,
    Connecting,
    WaitingForInput,
    Ready {
        helper_version: String,
        architecture: String,
    },
    Reconnecting {
        attempt: u32,
        delay_seconds: u64,
    },
    Failed {
        code: String,
        message: String,
    },
}

struct SshConnection {
    client: RemoteClient,
    process: Arc<Mutex<Child>>,
    _askpass: Option<tempfile::TempPath>,
    execution_environment: ExecutionEnvironment,
}

impl std::fmt::Debug for SshConnection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SshConnection")
            .field("client", &self.client)
            .finish_non_exhaustive()
    }
}

/// 系统 OpenSSH、helper bootstrap、远端 workspace 与自动重连的本地 owner。
#[derive(Debug, Clone)]
pub struct SshManager {
    servers: Arc<RwLock<HashMap<String, SshServerProfile>>>,
    helper_assets: Option<Arc<dyn RemoteHelperAssets>>,
    connections: Arc<Mutex<HashMap<String, SshConnection>>>,
    connection_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    workspaces: Arc<Mutex<HashMap<(String, String), RemoteWorkspaceHost>>>,
    workspace_paths: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    states: Arc<RwLock<HashMap<String, watch::Sender<SshConnectionState>>>>,
    desired_connections: Arc<RwLock<HashSet<String>>>,
    password_leases: Arc<RwLock<HashMap<String, SecretString>>>,
}

impl SshManager {
    /// 使用可选的开发 helper 资产创建 manager。
    ///
    /// 每个资产仍必须带有相邻的 `.sha256` 文件；生产环境应使用
    /// [`Self::with_helper_assets`] 提供随应用嵌入的压缩资产。
    pub fn new(aarch64_helper: Option<PathBuf>, x86_64_helper: Option<PathBuf>) -> Self {
        Self::with_optional_helper_assets(file_helper_assets(aarch64_helper, x86_64_helper))
    }

    /// 使用宿主提供的按需解压资产创建 manager。
    pub fn with_helper_assets(helper_assets: Arc<dyn RemoteHelperAssets>) -> Self {
        Self::with_optional_helper_assets(Some(helper_assets))
    }

    fn with_optional_helper_assets(helper_assets: Option<Arc<dyn RemoteHelperAssets>>) -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            helper_assets,
            connections: Arc::new(Mutex::new(HashMap::new())),
            connection_locks: Arc::new(Mutex::new(HashMap::new())),
            workspaces: Arc::new(Mutex::new(HashMap::new())),
            workspace_paths: Arc::new(RwLock::new(HashMap::new())),
            states: Arc::new(RwLock::new(HashMap::new())),
            desired_connections: Arc::new(RwLock::new(HashSet::new())),
            password_leases: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 返回按名称和 id 稳定排序的服务器配置。
    pub async fn list_servers(&self) -> Vec<SshServerProfile> {
        let mut servers = self
            .servers
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        servers.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
        servers
    }

    /// 校验并保存不含 secret 的服务器配置。
    pub async fn save_server(
        &self,
        profile: SshServerProfile,
    ) -> Result<SshServerProfile, RemoteClientError> {
        validate_profile(&profile)?;
        let changed = self
            .servers
            .read()
            .await
            .get(&profile.id)
            .is_some_and(|existing| existing != &profile);
        if changed {
            self.disconnect_server(&profile.id).await;
        }
        self.ensure_state(&profile.id).await;
        self.servers
            .write()
            .await
            .insert(profile.id.clone(), profile.clone());
        Ok(profile)
    }

    /// 在 core 内存中更新一次 SSH 密码 lease；密码不会进入 profile 或持久化层。
    pub async fn lease_password(&self, server_id: &str, password: String) {
        let changed = self
            .password_leases
            .read()
            .await
            .get(server_id)
            .is_none_or(|current| current.expose_secret() != password);
        if password.is_empty() {
            self.password_leases.write().await.remove(server_id);
        } else {
            self.password_leases
                .write()
                .await
                .insert(server_id.to_string(), SecretString::from(password));
        }
        if changed {
            self.disconnect_server(server_id).await;
        }
    }

    /// 删除服务器配置、secret lease、连接与 workspace cache。
    pub async fn delete_server(&self, server_id: &str) -> Result<(), RemoteClientError> {
        self.disconnect_server(server_id).await;
        self.servers.write().await.remove(server_id);
        self.password_leases.write().await.remove(server_id);
        self.states.write().await.remove(server_id);
        self.connection_locks.lock().await.remove(server_id);
        self.workspaces
            .lock()
            .await
            .retain(|(id, _), _| id != server_id);
        self.workspace_paths.write().await.remove(server_id);
        Ok(())
    }

    /// 读取服务器的 canonical 连接状态。
    pub async fn connection_snapshot(
        &self,
        server_id: &str,
    ) -> Result<SshConnectionSnapshot, RemoteClientError> {
        let sender = self.ensure_state(server_id).await;
        let state = sender.borrow().clone();
        Ok(SshConnectionSnapshot {
            server_id: server_id.to_string(),
            state,
        })
    }

    /// 订阅服务器连接状态；订阅本身不会建立连接。
    pub async fn subscribe_state(&self, server_id: &str) -> watch::Receiver<SshConnectionState> {
        self.ensure_state(server_id).await.subscribe()
    }

    /// 完成架构探测、helper bootstrap 与协议握手。
    pub async fn test_connection(
        &self,
        server_id: &str,
    ) -> Result<SshConnectionSnapshot, RemoteClientError> {
        self.connect_server(server_id).await?;
        self.connection_snapshot(server_id).await
    }

    /// 确保服务器已连接；并发调用按服务器串行化。
    pub async fn connect_server(&self, server_id: &str) -> Result<(), RemoteClientError> {
        self.desired_connections
            .write()
            .await
            .insert(server_id.to_string());
        let connection_lock = {
            let mut locks = self.connection_locks.lock().await;
            locks
                .entry(server_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _connection_guard = connection_lock.lock().await;
        {
            let mut connections = self.connections.lock().await;
            if let Some(connection) = connections.get(server_id) {
                if !connection.client.is_disconnected() {
                    return Ok(());
                }
                connections.remove(server_id);
            }
        }
        let profile = self.profile(server_id).await?;
        self.set_state(server_id, SshConnectionState::Connecting)
            .await;
        let result = self.connect(&profile).await;
        match result {
            Ok((connection, hello)) => {
                let client = connection.client.clone();
                let execution_environment = connection.execution_environment.clone();
                self.connections
                    .lock()
                    .await
                    .insert(server_id.to_string(), connection);
                self.reopen_known_workspaces(server_id, &client, &execution_environment)
                    .await;
                self.set_state(
                    server_id,
                    SshConnectionState::Ready {
                        helper_version: hello.helper_version,
                        architecture: hello.architecture,
                    },
                )
                .await;
                self.spawn_disconnect_monitor(server_id.to_string(), client);
                Ok(())
            }
            Err(error) => {
                let state = if matches!(error, RemoteClientError::CredentialRequired) {
                    SshConnectionState::WaitingForInput
                } else {
                    SshConnectionState::Failed {
                        code: "sshConnectionFailed".to_string(),
                        message: error.to_string(),
                    }
                };
                self.set_state(server_id, state).await;
                Err(error)
            }
        }
    }

    /// 关闭连接并取消该服务器的自动重连意图。
    pub async fn disconnect_server(&self, server_id: &str) {
        self.desired_connections.write().await.remove(server_id);
        if let Some(connection) = self.connections.lock().await.remove(server_id) {
            let _ = connection
                .client
                .request(RemoteRequest::Shutdown, &[])
                .await;
            let _ = connection.process.lock().await.kill().await;
        }
        self.workspaces
            .lock()
            .await
            .retain(|(id, _), _| id != server_id);
        self.set_state(server_id, SshConnectionState::Disconnected)
            .await;
    }

    /// 主动中断当前 SSH transport，并保留自动重连意图。
    ///
    /// # Errors
    ///
    /// 当服务器不存在、初次连接失败或 SSH 子进程无法终止时返回错误。
    pub async fn reconnect_server(&self, server_id: &str) -> Result<(), RemoteClientError> {
        self.desired_connections
            .write()
            .await
            .insert(server_id.to_string());
        let process = self
            .connections
            .lock()
            .await
            .get(server_id)
            .map(|connection| connection.process.clone());
        if let Some(process) = process {
            self.set_state(
                server_id,
                SshConnectionState::Reconnecting {
                    attempt: 1,
                    delay_seconds: 1,
                },
            )
            .await;
            process.lock().await.kill().await.map_err(|error| {
                RemoteClientError::Protocol(format!("failed to interrupt ssh: {error}"))
            })?;
            return Ok(());
        }
        self.connect_server(server_id).await
    }

    /// 浏览远端目录；该功能只用于宿主 UI，不注册给模型。
    pub async fn browse_directories(
        &self,
        server_id: &str,
        path: Option<String>,
    ) -> Result<RemoteDirectoryListing, RemoteClientError> {
        let client = self.client(server_id).await?;
        let reply = client
            .request(RemoteRequest::BrowseDirectories { path }, &[])
            .await?;
        match reply.response {
            RemoteResponse::Directories(listing) => Ok(listing),
            response => Err(RemoteClientError::Protocol(format!(
                "unexpected directory response: {response:?}"
            ))),
        }
    }

    /// 打开远端 workspace 并返回文件 backend。
    pub async fn open_workspace(
        &self,
        server_id: &str,
        path: String,
    ) -> Result<RemoteWorkspaceFileBackend, RemoteClientError> {
        Ok(self.open_workspace_host(server_id, path).await?.files)
    }

    /// 打开或复用远端 workspace 的完整本地 backend 集合。
    pub async fn open_workspace_host(
        &self,
        server_id: &str,
        path: String,
    ) -> Result<RemoteWorkspaceHost, RemoteClientError> {
        let client = self.client(server_id).await?;
        if let Some(host) = self
            .workspaces
            .lock()
            .await
            .get(&(server_id.to_string(), path.clone()))
            .filter(|host| host.files.client().is_same_connection(&client))
            .cloned()
        {
            return Ok(host);
        }
        let files = open_workspace(&client, path).await?;
        let client = files.client().clone();
        let workspace_id = files.workspace_id().to_string();
        let canonical_path = files.canonical_path().to_string();
        let commands = RemoteCommandBackend::new(client.clone(), workspace_id.clone());
        let git = RemoteExecutionBackend::new(client, workspace_id, canonical_path);
        let execution_environment = self
            .connections
            .lock()
            .await
            .get(server_id)
            .map(|connection| connection.execution_environment.clone())
            .ok_or_else(|| RemoteClientError::Protocol("SSH connection disappeared".to_string()))?;
        let host = RemoteWorkspaceHost {
            files,
            commands,
            git,
            execution_environment,
        };
        let canonical_path = host.files.canonical_path().to_string();
        self.workspace_paths
            .write()
            .await
            .entry(server_id.to_string())
            .or_default()
            .insert(canonical_path.clone());
        self.workspaces
            .lock()
            .await
            .insert((server_id.to_string(), canonical_path), host.clone());
        Ok(host)
    }

    async fn connect(
        &self,
        profile: &SshServerProfile,
    ) -> Result<(SshConnection, RemoteHello), RemoteClientError> {
        let password = match profile.auth {
            SshAuth::Password => Some(
                self.password_leases
                    .read()
                    .await
                    .get(&profile.id)
                    .cloned()
                    .ok_or(RemoteClientError::CredentialRequired)?,
            ),
            SshAuth::AgentOrKey { .. } => None,
        };
        let password = password.as_ref().map(|secret| secret.expose_secret());
        let platform = run_ssh_capture(profile, password, "uname -s; uname -m").await?;
        let target = RemoteHelperTarget::from_uname(&platform)?;
        let assets = self.helper_assets.clone().ok_or_else(|| {
            RemoteClientError::Protocol(format!(
                "helper artifact for {} is not available",
                target.triple()
            ))
        })?;
        let helper = load_helper(assets, target).await?;
        let remote_path = upload_helper(profile, password, &helper).await?;
        let mut prepared = ssh_command(profile, password)?;
        prepared
            .command
            .arg(format!("exec {remote_path}"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = prepared.command.spawn().map_err(|error| {
            RemoteClientError::Protocol(format!("failed to start ssh: {error}"))
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| RemoteClientError::Protocol("ssh process has no stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| RemoteClientError::Protocol("ssh process has no stdout".to_string()))?;
        let client = RemoteClient::from_streams(stdout, stdin);
        let reply = client
            .request(
                RemoteRequest::Hello {
                    protocol_version: REMOTE_PROTOCOL_VERSION,
                },
                &[],
            )
            .await?;
        let hello = match reply.response {
            RemoteResponse::Hello(hello) => hello,
            response => {
                return Err(RemoteClientError::Protocol(format!(
                    "unexpected helper hello response: {response:?}"
                )));
            }
        };
        if hello.protocol_version != REMOTE_PROTOCOL_VERSION {
            return Err(RemoteClientError::Protocol(format!(
                "helper negotiated protocol version {}, expected {}",
                hello.protocol_version, REMOTE_PROTOCOL_VERSION
            )));
        }
        Ok((
            SshConnection {
                client,
                process: Arc::new(Mutex::new(child)),
                _askpass: prepared.askpass,
                execution_environment: execution_environment_from_hello(&hello)?,
            },
            hello,
        ))
    }

    async fn client(&self, server_id: &str) -> Result<RemoteClient, RemoteClientError> {
        self.connect_server(server_id).await?;
        self.connections
            .lock()
            .await
            .get(server_id)
            .map(|connection| connection.client.clone())
            .ok_or(RemoteClientError::Disconnected)
    }

    async fn profile(&self, server_id: &str) -> Result<SshServerProfile, RemoteClientError> {
        self.servers
            .read()
            .await
            .get(server_id)
            .cloned()
            .ok_or_else(|| RemoteClientError::Protocol(format!("unknown SSH server '{server_id}'")))
    }

    async fn ensure_state(&self, server_id: &str) -> watch::Sender<SshConnectionState> {
        if let Some(sender) = self.states.read().await.get(server_id).cloned() {
            return sender;
        }
        let mut states = self.states.write().await;
        states
            .entry(server_id.to_string())
            .or_insert_with(|| watch::channel(SshConnectionState::Disconnected).0)
            .clone()
    }

    async fn set_state(&self, server_id: &str, state: SshConnectionState) {
        self.ensure_state(server_id).await.send_replace(state);
    }

    fn spawn_disconnect_monitor(&self, server_id: String, client: RemoteClient) {
        let manager = self.clone();
        tokio::spawn(async move {
            client.wait_disconnected().await;
            if !manager
                .desired_connections
                .read()
                .await
                .contains(&server_id)
            {
                return;
            }
            let removed = {
                let mut connections = manager.connections.lock().await;
                if connections
                    .get(&server_id)
                    .is_some_and(|connection| connection.client.is_same_connection(&client))
                {
                    connections.remove(&server_id)
                } else {
                    None
                }
            };
            if let Some(connection) = removed {
                let _ = connection.process.lock().await.kill().await;
                manager
                    .workspaces
                    .lock()
                    .await
                    .retain(|(id, _), _| id != &server_id);
                manager.reconnect_with_backoff(server_id).await;
            }
        });
    }

    async fn reconnect_with_backoff(&self, server_id: String) {
        const DELAYS: [u64; 6] = [1, 2, 4, 8, 15, 30];
        let mut attempt = 0_u32;
        loop {
            if !self.desired_connections.read().await.contains(&server_id) {
                return;
            }
            attempt = attempt.saturating_add(1);
            let delay_seconds = DELAYS[(attempt as usize - 1).min(DELAYS.len() - 1)];
            self.set_state(
                &server_id,
                SshConnectionState::Reconnecting {
                    attempt,
                    delay_seconds,
                },
            )
            .await;
            tokio::time::sleep(std::time::Duration::from_secs(delay_seconds)).await;
            match self.connect_server(&server_id).await {
                Ok(()) => return,
                Err(RemoteClientError::CredentialRequired) => return,
                Err(_) => {}
            }
        }
    }

    async fn reopen_known_workspaces(
        &self,
        server_id: &str,
        client: &RemoteClient,
        execution_environment: &ExecutionEnvironment,
    ) {
        let paths = self
            .workspace_paths
            .read()
            .await
            .get(server_id)
            .cloned()
            .unwrap_or_default();
        for path in paths {
            let Ok(files) = open_workspace(client, path.clone()).await else {
                continue;
            };
            let workspace_id = files.workspace_id().to_string();
            let canonical_path = files.canonical_path().to_string();
            let commands = RemoteCommandBackend::new(client.clone(), workspace_id.clone());
            let git =
                RemoteExecutionBackend::new(client.clone(), workspace_id, canonical_path.clone());
            self.workspaces.lock().await.insert(
                (server_id.to_string(), canonical_path),
                RemoteWorkspaceHost {
                    files,
                    commands,
                    git,
                    execution_environment: execution_environment.clone(),
                },
            );
        }
    }
}

fn execution_environment_from_hello(
    hello: &RemoteHello,
) -> Result<ExecutionEnvironment, RemoteClientError> {
    let os = match hello.os.to_ascii_lowercase().as_str() {
        "windows" => ExecutionOs::Windows,
        "linux" => ExecutionOs::Linux,
        "macos" | "darwin" => ExecutionOs::Macos,
        value => ExecutionOs::Other(value.to_string()),
    };
    let shell = match hello.shell.dialect {
        RemoteShellDialect::Bash => ShellDialect::Bash,
        RemoteShellDialect::Sh => ShellDialect::Sh,
        RemoteShellDialect::Pwsh => ShellDialect::Pwsh,
        RemoteShellDialect::PowerShell => ShellDialect::PowerShell,
        RemoteShellDialect::Cmd => ShellDialect::Cmd,
    };
    if hello.shell.path.trim().is_empty() {
        return Err(RemoteClientError::Protocol(
            "helper hello returned an empty shell path".to_string(),
        ));
    }
    Ok(ExecutionEnvironment::for_ssh(
        os,
        shell,
        hello.shell.path.clone(),
    ))
}

async fn open_workspace(
    client: &RemoteClient,
    path: String,
) -> Result<RemoteWorkspaceFileBackend, RemoteClientError> {
    let reply = client
        .request(RemoteRequest::OpenWorkspace { path }, &[])
        .await?;
    match reply.response {
        RemoteResponse::WorkspaceOpened(RemoteWorkspaceOpened {
            workspace_id,
            canonical_path,
        }) => Ok(RemoteWorkspaceFileBackend::new(
            client.clone(),
            workspace_id,
            canonical_path,
        )),
        response => Err(RemoteClientError::Protocol(format!(
            "unexpected open workspace response: {response:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> SshServerProfile {
        SshServerProfile {
            id: "server-1".to_string(),
            name: "Development".to_string(),
            host: "example.test".to_string(),
            port: 22,
            username: "dev".to_string(),
            auth: SshAuth::AgentOrKey {
                identity_file: None,
            },
        }
    }

    #[tokio::test]
    async fn server_crud_is_canonical_and_sorted() {
        let manager = SshManager::new(None, None);
        let mut later = profile();
        later.id = "later".to_string();
        later.name = "Zed".to_string();
        manager.save_server(later).await.expect("save later");
        manager.save_server(profile()).await.expect("save profile");

        let servers = manager.list_servers().await;
        assert_eq!(servers[0].name, "Development");
        assert_eq!(servers[1].name, "Zed");
    }
}
