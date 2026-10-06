//! MCP runtime worker:generation 生命周期编排、lease 发布与命令循环。
//!
//! 按域拆分:`reconcile` 承载 generation 准备与连接,`server` 承载 server/generation
//! 运行态结构,`tools` 承载工具定义过滤与超时解析,`dispatch` 承载工具调用与资源
//! 查询分发,`redaction` 承载 host 错误脱敏;本页保留 `RuntimeWorker` 命令循环、
//! generation 激活/回收与 lease 发布。

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use pl_protocol::{McpAvailabilityDescriptor, McpHealthSnapshot, PureError, Result};
use rmcp::model::Tool;
use tokio::sync::{Notify, broadcast, mpsc};

use super::{LeaseSnapshot, McpGeneration, McpResetScope, RuntimeCommand, ShutdownCompletion};
use crate::mcp::McpConnector;
use crate::mcp::config::EffectiveMcpServerConfig;
use crate::mcp::health::{McpAvailabilityKind, McpAvailabilitySnapshot};

mod dispatch;
mod reconcile;
mod redaction;
mod server;
mod tools;

use reconcile::{
    ActivePreparation, PendingReconcile, await_preparation, prepare_generation, reject_reconciles,
};
use server::{
    RuntimeGeneration, assign_tool_descriptors, reset_failed, server_fingerprint, unique_sessions,
};
use tools::{configured_startup_timeout, filter_tool_definitions};

/// `AcquireLease` 等待进行中 MCP preparation 完成的有界上限。
///
/// 启动路径的 MCP reconcile 已在后台执行，turn 开始时若探测尚未完成，
/// lease 请求最多等待该时长以拿到包含新工具的 generation；超时后回落
/// 当前 generation，避免首个 turn 被慢 server 无限阻塞。
const LEASE_ACQUIRE_WAIT: Duration = Duration::from_secs(20);

pub(super) async fn run(
    connector: McpConnector,
    receiver: mpsc::UnboundedReceiver<RuntimeCommand>,
    commands: mpsc::UnboundedSender<RuntimeCommand>,
    updates: broadcast::Sender<()>,
    shutdown_completion: ShutdownCompletion,
) {
    RuntimeWorker::new(connector, receiver, commands, updates, shutdown_completion)
        .run()
        .await;
}

struct RuntimeWorker {
    connector: McpConnector,
    receiver: mpsc::UnboundedReceiver<RuntimeCommand>,
    commands: mpsc::UnboundedSender<RuntimeCommand>,
    updates: broadcast::Sender<()>,
    generations: BTreeMap<McpGeneration, RuntimeGeneration>,
    current: McpGeneration,
    next_generation: u64,
    /// preparation 完成信号，用于 `AcquireLease` 的有界等待。
    preparation_notify: Arc<Notify>,
    /// Single-owned closing completion shared with every runtime handle.
    shutdown_completion: ShutdownCompletion,
}

struct ActiveToolRefresh {
    future: BoxFuture<'static, ToolRefreshCandidate>,
}

struct ToolRefreshCandidate {
    observed_generation: McpGeneration,
    server_id: String,
    session: Arc<crate::mcp::ConnectedMcp>,
    definitions: std::result::Result<Vec<Tool>, String>,
}

async fn await_tool_refresh(refresh: &mut Option<ActiveToolRefresh>) -> ToolRefreshCandidate {
    refresh
        .as_mut()
        .expect("guarded MCP tool refresh must exist")
        .future
        .as_mut()
        .await
}

impl RuntimeWorker {
    fn new(
        connector: McpConnector,
        receiver: mpsc::UnboundedReceiver<RuntimeCommand>,
        commands: mpsc::UnboundedSender<RuntimeCommand>,
        updates: broadcast::Sender<()>,
        shutdown_completion: ShutdownCompletion,
    ) -> Self {
        let current = McpGeneration(0);
        Self {
            connector,
            receiver,
            commands,
            updates,
            generations: [(current, RuntimeGeneration::empty(current))]
                .into_iter()
                .collect(),
            current,
            next_generation: 1,
            preparation_notify: Arc::new(Notify::new()),
            shutdown_completion,
        }
    }

    async fn run(mut self) {
        let mut pending = VecDeque::new();
        let mut preparation = None;
        let mut pending_tool_refreshes = BTreeSet::new();
        let mut tool_refresh = None;
        loop {
            if preparation.is_none() && tool_refresh.is_none() {
                if let Some(request) = pending.pop_front() {
                    preparation = Some(self.start_preparation(request));
                } else if let Some(server_id) = pending_tool_refreshes.pop_first() {
                    tool_refresh = self.start_tool_refresh(server_id);
                }
            }
            tokio::select! {
                generation = await_preparation(&mut preparation), if preparation.is_some() => {
                    let active = preparation
                        .take()
                        .expect("completed MCP preparation must exist");
                    let result = if active.reset_scope.as_ref().is_some_and(|scope| {
                        reset_failed(scope, &generation)
                    }) {
                        self.close_unpublished_generation(generation).await;
                        Err(PureError::ToolExecutionFailed {
                            tool: "mcp".to_string(),
                            error: "MCP reset candidate failed; current generation was preserved".to_string(),
                        })
                    } else {
                        self.activate_generation(generation).await;
                        Ok(())
                    };
                    self.preparation_notify.notify_waiters();
                    let _ = active.reply.send(result);
                }
                candidate = await_tool_refresh(&mut tool_refresh), if tool_refresh.is_some() => {
                    tool_refresh = None;
                    self.apply_tool_refresh(candidate, &mut pending_tool_refreshes).await;
                    self.preparation_notify.notify_waiters();
                }
                command = self.receiver.recv() => {
                    let Some(command) = command else {
                        reject_reconciles(preparation.take(), &mut pending);
                        break;
                    };
                    match command {
                RuntimeCommand::Reconcile {
                    servers,
                    reply,
                } => {
                    if self.configuration_matches(&servers) {
                        let _ = reply.send(Ok(()));
                        continue;
                    }
                    pending.push_back(PendingReconcile {
                        servers,
                        reset_scope: None,
                        reply,
                    });
                }
                RuntimeCommand::Reset {
                    servers,
                    scope,
                    reply,
                } => {
                    pending.push_back(PendingReconcile {
                        servers,
                        reset_scope: Some(scope),
                        reply,
                    });
                }
                RuntimeCommand::AcquireLease { reply } => {
                    if preparation.is_some()
                        || tool_refresh.is_some()
                        || !pending.is_empty()
                        || !pending_tool_refreshes.is_empty()
                    {
                        let notify = self.preparation_notify.clone();
                        let commands = self.commands.clone();
                        tokio::spawn(async move {
                            let _ =
                                tokio::time::timeout(LEASE_ACQUIRE_WAIT, notify.notified()).await;
                            let _ =
                                commands.send(RuntimeCommand::AcquireLeaseImmediate { reply });
                        });
                    } else {
                        let _ = reply.send(self.acquire_lease());
                    }
                }
                RuntimeCommand::AcquireLeaseImmediate { reply } => {
                    let _ = reply.send(self.acquire_lease());
                }
                RuntimeCommand::ReleaseLease { generation } => {
                    self.release_lease(generation).await;
                }
                RuntimeCommand::Snapshots { reply } => {
                    let _ = reply.send(self.snapshots());
                }
                RuntimeCommand::HealthSnapshot { reply } => {
                    let _ = reply.send(self.health_snapshot());
                }
                RuntimeCommand::AvailableServerNames { reply } => {
                    let _ = reply.send(self.available_server_names());
                }
                RuntimeCommand::CallTool {
                    generation,
                    server_id,
                    raw_name,
                    arguments,
                    reply,
                } => self.spawn_tool_call(generation, server_id, raw_name, arguments, reply),
                RuntimeCommand::ResourceQuery {
                    generation,
                    server_id,
                    operation,
                    reply,
                } => self.spawn_resource_query(generation, server_id, operation, reply),
                RuntimeCommand::MarkUnavailable {
                    generation,
                    server_id,
                    error,
                } => self.mark_unavailable(generation, &server_id, error),
                RuntimeCommand::ToolListChanged { server_id } => {
                    pending_tool_refreshes.insert(server_id);
                }
                RuntimeCommand::Shutdown { reply } => {
                    reject_reconciles(preparation.take(), &mut pending);
                    let result = self.shutdown().await;
                    let recorded = match &result {
                        Ok(()) => Ok(()),
                        Err(error) => Err(error.to_string()),
                    };
                    self.shutdown_completion.record(recorded).await;
                    let _ = reply.send(result);
                    return;
                }
                    }
                }
            }
        }
        let result = self.shutdown().await;
        let recorded = match &result {
            Ok(()) => Ok(()),
            Err(error) => Err(error.to_string()),
        };
        self.shutdown_completion.record(recorded).await;
    }

    fn start_preparation(&mut self, request: PendingReconcile) -> ActivePreparation {
        let generation_id = McpGeneration(self.next_generation);
        self.next_generation += 1;
        let mut reusable = self.current_generation().servers.clone();
        match request.reset_scope.as_ref() {
            Some(McpResetScope::Server { server_id }) => {
                reusable.remove(server_id);
            }
            Some(McpResetScope::All) => reusable.clear(),
            None => {}
        }
        let future = prepare_generation(
            self.connector.clone(),
            generation_id,
            request.servers,
            reusable,
        );
        ActivePreparation {
            future: future.boxed(),
            reply: request.reply,
            reset_scope: request.reset_scope,
        }
    }

    fn start_tool_refresh(&self, server_id: String) -> Option<ActiveToolRefresh> {
        let generation = self.current;
        let server = self.current_generation().servers.get(&server_id)?;
        if server.availability != McpAvailabilityKind::Available {
            return None;
        }
        let session = server.session.clone()?;
        let timeout = configured_startup_timeout(server.config.startup_timeout_secs);
        let config = server.config.clone();
        let redactor = server.redactor.clone();
        Some(ActiveToolRefresh {
            future: async move {
                let definitions = match tokio::time::timeout(timeout, session.list_tools()).await {
                    Ok(Ok(tools)) => Ok(filter_tool_definitions(tools, &config)),
                    Ok(Err(error)) => Err(redactor.redact(error.to_string())),
                    Err(_) => Err(format!(
                        "MCP tool refresh timed out after {} seconds",
                        timeout.as_secs()
                    )),
                };
                ToolRefreshCandidate {
                    observed_generation: generation,
                    server_id,
                    session,
                    definitions,
                }
            }
            .boxed(),
        })
    }

    async fn apply_tool_refresh(
        &mut self,
        candidate: ToolRefreshCandidate,
        pending: &mut BTreeSet<String>,
    ) {
        if candidate.observed_generation != self.current {
            if self
                .current_generation()
                .servers
                .contains_key(&candidate.server_id)
            {
                pending.insert(candidate.server_id);
            }
            return;
        }
        let Some(current_server) = self.current_generation().servers.get(&candidate.server_id)
        else {
            return;
        };
        if current_server.availability != McpAvailabilityKind::Available
            || current_server
                .session
                .as_ref()
                .is_none_or(|session| !Arc::ptr_eq(session, &candidate.session))
        {
            return;
        }
        let definitions = match candidate.definitions {
            Ok(definitions) => definitions,
            Err(error) => {
                tracing::warn!(
                    server_id = %candidate.server_id,
                    %error,
                    "MCP tools/list_changed refresh failed; preserving the current generation"
                );
                return;
            }
        };
        let generation_id = McpGeneration(self.next_generation);
        self.next_generation += 1;
        let mut next = RuntimeGeneration::empty(generation_id);
        next.servers = self.current_generation().servers.clone();
        let Some(server) = next.servers.get_mut(&candidate.server_id) else {
            return;
        };
        server.message = Some(format!("Available with {} tools", definitions.len()));
        server.last_checked_at = Some(crate::unix_seconds());
        server.definitions = definitions;
        assign_tool_descriptors(&mut next.servers);
        self.activate_generation(next).await;
    }

    fn configuration_matches(&self, servers: &BTreeMap<String, EffectiveMcpServerConfig>) -> bool {
        let current = self.current_generation();
        current.servers.len() == servers.len()
            && servers.iter().all(|(server_id, config)| {
                current
                    .servers
                    .get(server_id)
                    .is_some_and(|server| server.fingerprint == server_fingerprint(config))
            })
    }

    async fn activate_generation(&mut self, mut next: RuntimeGeneration) {
        self.propagate_pending_session_failures(&mut next);
        if let Some(previous) = self.generations.get_mut(&self.current) {
            previous.retired = true;
        }
        self.current = next.id;
        self.generations.insert(next.id, next);
        self.cleanup_retired().await;
        self.emit_update();
    }

    async fn close_unpublished_generation(&self, generation: RuntimeGeneration) {
        let current_sessions = self
            .current_generation()
            .servers
            .values()
            .filter_map(|server| server.session.as_ref())
            .map(|session| Arc::as_ptr(session) as usize)
            .collect::<BTreeSet<_>>();
        let sessions = unique_sessions(generation.servers)
            .into_iter()
            .filter(|session| !current_sessions.contains(&(Arc::as_ptr(session) as usize)))
            .collect::<Vec<_>>();
        // Independent owners: cancel and await them concurrently so one slow server cannot
        // consume the budget of the others.
        let outcomes =
            futures::future::join_all(sessions.iter().map(|session| session.close())).await;
        for outcome in outcomes {
            if let Err(error) = outcome {
                tracing::warn!(
                    %error,
                    "MCP session did not confirm closure while discarding an unpublished generation"
                );
            }
        }
    }

    fn propagate_pending_session_failures(&self, next: &mut RuntimeGeneration) {
        for pending in next.servers.values_mut() {
            let Some(session) = pending.session.as_ref() else {
                continue;
            };
            let failed = self.generations.values().find_map(|generation| {
                generation.servers.values().find(|active| {
                    active.availability == McpAvailabilityKind::Unavailable
                        && active
                            .session
                            .as_ref()
                            .is_some_and(|active| Arc::ptr_eq(active, session))
                })
            });
            if let Some(failed) = failed {
                pending.availability = McpAvailabilityKind::Unavailable;
                pending.message = failed.message.clone();
                pending.last_checked_at = failed.last_checked_at;
            }
        }
    }

    fn acquire_lease(&mut self) -> Result<LeaseSnapshot> {
        let generation = self.generations.get_mut(&self.current).ok_or_else(|| {
            PureError::ToolExecutionFailed {
                tool: "mcp".to_string(),
                error: "MCP current generation is unavailable".to_string(),
            }
        })?;
        generation.leases += 1;
        let tools = generation
            .servers
            .values()
            .filter(|server| server.availability == McpAvailabilityKind::Available)
            .flat_map(|server| server.tools.clone())
            .collect();
        let server_ids = generation
            .servers
            .values()
            .filter(|server| server.availability == McpAvailabilityKind::Available)
            .map(|server| server.descriptor.id.clone())
            .collect();
        let resource_server_ids = generation
            .servers
            .values()
            .filter(|server| {
                server.availability == McpAvailabilityKind::Available && server.supports_resources
            })
            .map(|server| server.descriptor.id.clone())
            .collect();
        Ok(LeaseSnapshot {
            generation: generation.id,
            tools,
            server_ids,
            resource_server_ids,
        })
    }

    async fn release_lease(&mut self, generation: McpGeneration) {
        if let Some(state) = self.generations.get_mut(&generation) {
            state.leases = state.leases.saturating_sub(1);
        }
        self.cleanup_retired().await;
    }

    fn snapshots(&self) -> BTreeMap<String, McpAvailabilitySnapshot> {
        self.current_generation()
            .servers
            .iter()
            .map(|(server_id, server)| {
                (
                    server_id.clone(),
                    McpAvailabilitySnapshot {
                        server_id: server_id.clone(),
                        availability_kind: server.availability,
                        availability_message: server.message.clone(),
                        last_checked_at: server.last_checked_at,
                        tool_count: (server.availability == McpAvailabilityKind::Available)
                            .then_some(server.tools.len()),
                    },
                )
            })
            .collect()
    }

    fn health_snapshot(&self) -> McpHealthSnapshot {
        let generation = self.current_generation();
        McpHealthSnapshot {
            generation: generation.id.0,
            servers: generation
                .servers
                .values()
                .map(|server| McpAvailabilityDescriptor {
                    server: server.descriptor.clone(),
                    availability: server.availability.as_str().to_string(),
                    message: server.message.clone(),
                    last_checked_at: server.last_checked_at,
                    tool_count: (server.availability == McpAvailabilityKind::Available)
                        .then_some(server.tools.len()),
                })
                .collect(),
        }
    }

    fn available_server_names(&self) -> Vec<String> {
        self.current_generation()
            .servers
            .values()
            .filter(|server| server.availability == McpAvailabilityKind::Available)
            .map(|server| server.descriptor.id.clone())
            .collect()
    }

    async fn cleanup_retired(&mut self) {
        let retired = self
            .generations
            .iter()
            .filter(|(_, generation)| generation.retired && generation.leases == 0)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        let mut sessions = Vec::new();
        for id in retired {
            let Some(generation) = self.generations.remove(&id) else {
                continue;
            };
            for session in unique_sessions(generation.servers) {
                // A session still referenced by another generation is owned elsewhere; the
                // generation that still holds it retires it.
                if Arc::strong_count(&session) == 1 {
                    sessions.push(session);
                }
            }
        }
        // Independent owners: cancel and await them concurrently so one slow server cannot
        // consume the budget of the others.
        let outcomes =
            futures::future::join_all(sessions.iter().map(|session| session.close())).await;
        for outcome in outcomes {
            if let Err(error) = outcome {
                tracing::warn!(
                    %error,
                    "MCP session did not confirm closure while retiring a generation"
                );
            }
        }
    }

    /// 关闭全部 generation 的 session 并确认整树回收。
    ///
    /// 独立 session 的关闭并发发出，任一 hang 不会阻止其它 session 收到 cancel；只有
    /// 确认回收的 session 才从 generation 中移除，未确认的 owner 由 `ConnectedMcp`
    /// 自有的 close task 保留到任务结束或进程终止。返回 typed `Result`，失败带明确
    /// server id，让 runtime 能定位到具体未回收的 server 而不是泛化的 "mcp"。
    async fn shutdown(&mut self) -> Result<()> {
        let sessions = {
            let mut seen = BTreeSet::new();
            let mut sessions = Vec::new();
            for generation in self.generations.values() {
                for (server_id, server) in &generation.servers {
                    if let Some(session) = &server.session
                        && seen.insert(Arc::as_ptr(session) as usize)
                    {
                        sessions.push((server_id.clone(), session.clone()));
                    }
                }
            }
            sessions
        };
        let results = futures::future::join_all(sessions.iter().map(|(server_id, session)| {
            let server_id = server_id.clone();
            let session = session.clone();
            async move {
                let outcome = session.close().await;
                (server_id, session, outcome)
            }
        }))
        .await;
        // Only drop the owner of a session whose process tree was confirmed reclaimed. An
        // unconfirmed owner stays in its generation so it is never cleared (and dropped)
        // while the tree may still be alive; it is retained until the worker — and its
        // `ConnectedMcp` close task — actually ends.
        let mut reclaimed = BTreeSet::new();
        let mut failures = Vec::new();
        for (server_id, session, outcome) in results {
            match outcome {
                Ok(()) => {
                    reclaimed.insert(Arc::as_ptr(&session) as usize);
                }
                Err(error) => failures.push(format!(
                    "MCP server '{server_id}' did not confirm process-tree reclamation: {error}"
                )),
            }
        }
        if !reclaimed.is_empty() {
            for generation in self.generations.values_mut() {
                for server in generation.servers.values_mut() {
                    if server
                        .session
                        .as_ref()
                        .is_some_and(|session| reclaimed.contains(&(Arc::as_ptr(session) as usize)))
                    {
                        server.session = None;
                    }
                }
            }
        }
        self.emit_update();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(PureError::ToolExecutionFailed {
                tool: "mcp".to_string(),
                error: failures.join("; "),
            })
        }
    }

    fn current_generation(&self) -> &RuntimeGeneration {
        self.generations
            .get(&self.current)
            .expect("MCP current generation must exist")
    }

    fn emit_update(&self) {
        let _ = self.updates.send(());
    }
}
