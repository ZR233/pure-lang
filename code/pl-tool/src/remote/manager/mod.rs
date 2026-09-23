//! 系统 OpenSSH 连接、重连与 remote workspace handle 的本地 owner。

mod asset;
mod shutdown;
mod ssh;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use pl_protocol::remote::{
    REMOTE_PROTOCOL_VERSION, RemoteDirectoryListing, RemoteHello, RemoteRequest, RemoteResponse,
    RemoteWorkspaceOpened,
};
use serde::{Deserialize, Serialize};
use tokio::process::Child;
use tokio::sync::{Mutex, RwLock, watch};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use super::{
    RemoteClient, RemoteClientError, RemoteCommandBackend, RemoteExecutionBackend,
    RemoteWorkspaceFileBackend, RemoteWorkspaceHost, normalize_remote_absolute_path,
};
use crate::environment::{ExecutionEnvironment, ExecutionOs, ShellDialect};
use pl_protocol::remote::RemoteShellDialect;

pub use self::asset::{RemoteHelperAssets, RemoteHelperTarget};
use self::asset::{file_helper_assets, load_helper, upload_helper};
use self::ssh::{run_ssh_capture, ssh_command, validate_profile};
use super::ssh_config::{SshConfigEntry, SshConfigFile};

/// 不含 secret 的 SSH 服务器配置；别名即身份，对应 `~/.ssh/config` 的 Host。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshServerProfile {
    pub alias: String,
    pub host_name: String,
    pub port: u16,
    pub username: String,
    pub identity_file: Option<String>,
}

/// 单个 SSH 服务器的 canonical 连接快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshConnectionSnapshot {
    pub alias: String,
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
    servers: Arc<RwLock<HashMap<String, SshConfigEntry>>>,
    ssh_config: SshConfigFile,
    helper_assets: Option<Arc<dyn RemoteHelperAssets>>,
    connections: Arc<Mutex<HashMap<String, Arc<SshConnection>>>>,
    admission: Arc<Mutex<bool>>,
    closing: CancellationToken,
    operations: TaskTracker,
    connection_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    workspaces: Arc<Mutex<HashMap<(String, String), RemoteWorkspaceHost>>>,
    workspace_paths: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    states: Arc<RwLock<HashMap<String, watch::Sender<SshConnectionState>>>>,
    ready_revisions: watch::Sender<BTreeMap<String, u64>>,
    desired_connections: Arc<RwLock<HashSet<String>>>,
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

    /// 显式指定 ssh config 文件；默认使用用户 `~/.ssh/config`。
    ///
    /// 测试与启动协调器用它隔离或收敛配置写入范围。
    pub fn with_ssh_config(mut self, ssh_config: SshConfigFile) -> Self {
        self.ssh_config = ssh_config;
        self
    }

    fn with_optional_helper_assets(helper_assets: Option<Arc<dyn RemoteHelperAssets>>) -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            ssh_config: SshConfigFile::user_default().unwrap_or_else(|error| {
                tracing::warn!(%error, "falling back to the default ssh config location");
                SshConfigFile::default_location(".ssh/config")
            }),
            helper_assets,
            connections: Arc::new(Mutex::new(HashMap::new())),
            admission: Arc::new(Mutex::new(true)),
            closing: CancellationToken::new(),
            operations: TaskTracker::new(),
            connection_locks: Arc::new(Mutex::new(HashMap::new())),
            workspaces: Arc::new(Mutex::new(HashMap::new())),
            workspace_paths: Arc::new(RwLock::new(HashMap::new())),
            states: Arc::new(RwLock::new(HashMap::new())),
            ready_revisions: watch::channel(BTreeMap::new()).0,
            desired_connections: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// 从 ssh config 文件重新加载服务器集合；文件是该数据的唯一事实源。
    pub async fn reload_servers(&self) -> Result<(), RemoteClientError> {
        let entries = self.ssh_config.read().await?;
        let mut servers = self.servers.write().await;
        servers.clear();
        for entry in entries {
            servers.insert(entry.profile.alias.clone(), entry);
        }
        Ok(())
    }

    /// 返回按别名稳定排序的服务器条目（含是否为 anywork 管理块）。
    pub async fn list_servers(&self) -> Vec<SshConfigEntry> {
        let mut servers = self
            .servers
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        servers.sort_by(|left, right| left.profile.alias.cmp(&right.profile.alias));
        servers
    }

    /// 校验并把服务器配置写入 ssh config 管理块。
    ///
    /// 别名被手写条目占用、或文件写入失败时返回错误且不改变内存集合。
    pub async fn save_server(
        &self,
        profile: SshServerProfile,
    ) -> Result<SshServerProfile, RemoteClientError> {
        validate_profile(&profile)?;
        let previous = self
            .servers
            .read()
            .await
            .get(&profile.alias)
            .map(|entry| entry.profile.clone());
        self.ssh_config
            .upsert_managed(std::slice::from_ref(&profile))
            .await?;
        if previous.as_ref() != Some(&profile) {
            self.disconnect_server(&profile.alias).await?;
        }
        self.ensure_state(&profile.alias).await;
        self.servers.write().await.insert(
            profile.alias.clone(),
            SshConfigEntry {
                profile: profile.clone(),
                managed: true,
            },
        );
        Ok(profile)
    }

    /// 删除服务器配置、连接与 workspace cache；仅 anywork 管理块可删除。
    pub async fn delete_server(&self, server_id: &str) -> Result<(), RemoteClientError> {
        self.ssh_config.remove_managed(server_id).await?;
        self.disconnect_server(server_id).await?;
        self.servers.write().await.remove(server_id);
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
            alias: server_id.to_string(),
            state,
        })
    }

    /// 订阅服务器连接状态；订阅本身不会建立连接。
    pub async fn subscribe_state(&self, server_id: &str) -> watch::Receiver<SshConnectionState> {
        self.ensure_state(server_id).await.subscribe()
    }

    /// Subscribes to completed connections, including automatic reconnections.
    ///
    /// Revisions are local to this manager and retained across disconnects. Consumers
    /// must compare revisions, since multiple connections can complete between reads.
    /// Subscribing does not connect and carries no credentials or remote environment.
    pub fn subscribe_ready(&self) -> watch::Receiver<BTreeMap<String, u64>> {
        self.ready_revisions.subscribe()
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
        let _operation = self.admit_connection().await?;
        self.desired_connections
            .write()
            .await
            .insert(server_id.to_string());
        let connection_lock = self.connection_lock(server_id).await;
        let _connection_guard = connection_lock.lock().await;
        let existing = self.connections.lock().await.get(server_id).cloned();
        if let Some(connection) = existing {
            if !connection.client.is_disconnected() {
                return Ok(());
            }
            self.close_connection(server_id).await?;
        }
        let profile = self.profile(server_id).await?;
        self.set_state(server_id, SshConnectionState::Connecting)
            .await;
        let result = tokio::select! {
            () = self.closing.cancelled() => Err(RemoteClientError::ManagerClosing),
            result = self.connect(&profile) => result,
        };
        match result {
            Ok((connection, hello)) => {
                let client = connection.client.clone();
                let execution_environment = connection.execution_environment.clone();
                self.connections
                    .lock()
                    .await
                    .insert(server_id.to_string(), Arc::new(connection));
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
                self.set_state(
                    server_id,
                    SshConnectionState::Failed {
                        code: "sshConnectionFailed".to_string(),
                        message: error.to_string(),
                    },
                )
                .await;
                Err(error)
            }
        }
    }

    /// 关闭连接并取消该服务器的自动重连意图。
    ///
    /// # Errors
    /// Returns closing, transport, or process cleanup errors; failed resources remain owned.
    pub async fn disconnect_server(&self, server_id: &str) -> Result<(), RemoteClientError> {
        let _operation = self.admit_connection().await?;
        self.desired_connections.write().await.remove(server_id);
        let connection_lock = self.connection_lock(server_id).await;
        let _connection_guard = connection_lock.lock().await;
        self.close_connection(server_id).await
    }

    /// 主动中断当前 SSH transport，并保留自动重连意图。
    ///
    /// # Errors
    ///
    /// 当服务器不存在、初次连接失败或 SSH 子进程无法终止时返回错误。
    pub async fn reconnect_server(&self, server_id: &str) -> Result<(), RemoteClientError> {
        let _operation = self.admit_connection().await?;
        self.desired_connections
            .write()
            .await
            .insert(server_id.to_string());
        {
            let connection_lock = self.connection_lock(server_id).await;
            let _connection_guard = connection_lock.lock().await;
            self.close_connection(server_id).await?;
        }
        self.connect_server(server_id).await
    }

    /// 浏览远端目录；该功能只用于宿主 UI，不注册给模型。
    pub async fn browse_directories(
        &self,
        server_id: &str,
        path: Option<String>,
    ) -> Result<RemoteDirectoryListing, RemoteClientError> {
        let _operation = self.admit_connection().await?;
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
        // 跨端入口统一归一化为 POSIX：宿主形态（含 Windows 分隔符）的同一远端目录必须
        // 映射到同一个缓存键与同一个 helper workspace，不得因 canonicalize 失败或重复
        // 打开而产生两个 workspace handle。缓存键语义保持既有形状（请求路径用于查找、
        // helper canonical 路径用于存放）。
        let path = normalize_remote_absolute_path(&path)
            .map_err(|error| RemoteClientError::Protocol(error.to_string()))?;
        let _operation = self.admit_connection().await?;
        let client = self.client(server_id).await?;
        let connection_lock = self.connection_lock(server_id).await;
        let _connection_guard = connection_lock.lock().await;
        // Keep transport, workspace and environment from one connection, even if
        // reconnect raced the initial lookup. Never publish a mixed-generation host.
        let connection = self
            .connections
            .lock()
            .await
            .get(server_id)
            .cloned()
            .filter(|connection| {
                connection.client.is_same_connection(&client) && !client.is_disconnected()
            })
            .ok_or(RemoteClientError::Disconnected)?;
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
        let execution_environment = connection.execution_environment.clone();
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
        let platform = run_ssh_capture(profile, &self.ssh_config, "uname -s; uname -m").await?;
        let target = RemoteHelperTarget::from_uname(&platform)?;
        let assets = self.helper_assets.clone().ok_or_else(|| {
            RemoteClientError::Protocol(format!(
                "helper artifact for {} is not available",
                target.triple()
            ))
        })?;
        let helper = load_helper(assets, target).await?;
        let remote_path = upload_helper(profile, &self.ssh_config, &helper).await?;
        let mut prepared = ssh_command(profile, &self.ssh_config).await?;
        prepared
            .command
            .arg(ssh::posix_remote_command(&format!("exec {remote_path}")))
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
        let handshake = tokio::time::timeout(
            std::time::Duration::from_secs(25),
            client.request(
                RemoteRequest::Hello {
                    protocol_version: REMOTE_PROTOCOL_VERSION,
                },
                &[],
            ),
        )
        .await
        .unwrap_or_else(|_| {
            Err(RemoteClientError::Protocol(
                "SSH helper handshake timed out".to_string(),
            ))
        });
        let reply = match handshake {
            Ok(reply) => reply,
            Err(reason) => {
                // A failed bootstrap never becomes a managed connection. Close and reap the
                // local SSH transport before reporting its bounded initialization diagnostic.
                child.kill().await.map_err(|error| {
                    RemoteClientError::Protocol(format!("failed to reap SSH bootstrap: {error}"))
                })?;
                let diagnostic = ssh::initialization_diagnostic(child.stderr.take()).await?;
                return Err(RemoteClientError::Protocol(format!(
                    "SSH helper initialization failed: {reason}; {diagnostic}"
                )));
            }
        };
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
        if let Some(mut stderr) = child.stderr.take() {
            let client = client.clone();
            let closing = self.closing.clone();
            let server_id = profile.alias.clone();
            self.operations.spawn(async move {
                use tokio::io::AsyncReadExt;
                let mut retained = Vec::new();
                let mut buffer = [0; 1024];
                loop {
                    tokio::select! {
                        () = closing.cancelled() => break,
                        () = client.wait_disconnected() => break,
                        read = stderr.read(&mut buffer) => match read {
                            Ok(0) | Err(_) => break,
                            Ok(count) => {
                                retained.extend_from_slice(&buffer[..count]);
                                if retained.len() > 4096 { retained.drain(..retained.len() - 4096); }
                            }
                        }
                    }
                }
                if !retained.is_empty() {
                    let diagnostic = String::from_utf8_lossy(&retained).into_owned();
                    tracing::warn!(%server_id, %diagnostic, "SSH process diagnostic");
                }
            });
        }
        Ok((
            SshConnection {
                client,
                process: Arc::new(Mutex::new(child)),
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
            .map(|entry| entry.profile.clone())
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
        let ready = matches!(state, SshConnectionState::Ready { .. });
        self.ensure_state(server_id).await.send_replace(state);
        if ready {
            self.ready_revisions.send_modify(|revisions| {
                let revision = revisions.entry(server_id.to_owned()).or_default();
                *revision = revision.saturating_add(1);
            });
        }
    }

    fn spawn_disconnect_monitor(&self, server_id: String, client: RemoteClient) {
        let manager = self.clone();
        // The admitting connect operation is still tracked while this task is added.
        self.operations.spawn(async move {
            tokio::select! {
                biased;
                _ = manager.closing.cancelled() => return,
                _ = client.wait_disconnected() => {},
            }
            tracing::warn!(%server_id, reason = ?client.disconnect_reason(), "SSH transport disconnected");
            if !manager
                .desired_connections
                .read()
                .await
                .contains(&server_id)
            {
                return;
            }
            {
                let connection_lock = manager.connection_lock(&server_id).await;
                let _connection_guard = connection_lock.lock().await;
                let current = manager
                    .connections
                    .lock()
                    .await
                    .get(&server_id)
                    .is_some_and(|connection| connection.client.is_same_connection(&client));
                if !current {
                    return;
                }
                if let Err(error) = manager.close_connection(&server_id).await {
                    manager
                        .set_state(
                            &server_id,
                            SshConnectionState::Failed {
                                code: "sshCleanupFailed".to_string(),
                                message: error.to_string(),
                            },
                        )
                        .await;
                    return;
                }
            }
            manager.reconnect_with_backoff(server_id).await;
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
            tokio::select! {
                biased;
                _ = self.closing.cancelled() => return,
                _ = tokio::time::sleep(std::time::Duration::from_secs(delay_seconds)) => {},
            }
            match self.connect_server(&server_id).await {
                Ok(()) => return,
                Err(RemoteClientError::ManagerClosing) => return,
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
            let Ok(Ok(files)) = tokio::time::timeout(
                std::time::Duration::from_secs(15),
                open_workspace(client, path.clone()),
            )
            .await
            else {
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
    let reply = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        client.request(RemoteRequest::OpenWorkspace { path }, &[]),
    )
    .await
    .map_err(|_| RemoteClientError::Protocol("SSH workspace open timed out".into()))??;
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
