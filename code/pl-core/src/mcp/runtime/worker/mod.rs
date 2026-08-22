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
use pl_protocol::{McpAvailabilityDescriptor, McpHealthSnapshot, PureError, Result};
use tokio::sync::{Notify, broadcast, mpsc};

use super::{LeaseSnapshot, McpGeneration, McpResetScope, McpTurnLease, RuntimeCommand};
use crate::config::EffectiveMcpServerConfig;
use crate::mcp::McpConnector;
use crate::mcp::health::{McpAvailabilityKind, McpAvailabilitySnapshot};
use crate::tool::{PublishGuard, ToolRegistry, ToolSourceId};

mod dispatch;
mod reconcile;
mod redaction;
mod server;
mod tools;

use reconcile::{
    ActivePreparation, PendingReconcile, await_preparation, prepare_generation, reject_reconciles,
};
use server::{RuntimeGeneration, reset_failed, server_fingerprint, unique_sessions};

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
    shared_registry: Option<Arc<ToolRegistry>>,
) {
    RuntimeWorker::new(connector, receiver, commands, updates, shared_registry)
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
    /// 共享工具注册表；generation 激活或 server 可用性变化后整组发布。
    shared_registry: Option<Arc<ToolRegistry>>,
    /// 当前发布的 master lease：钉扎 generation，条目 handler 持有它的 clone。
    published_lease: Option<McpTurnLease>,
    publish_guard: Option<PublishGuard>,
}

impl RuntimeWorker {
    fn new(
        connector: McpConnector,
        receiver: mpsc::UnboundedReceiver<RuntimeCommand>,
        commands: mpsc::UnboundedSender<RuntimeCommand>,
        updates: broadcast::Sender<()>,
        shared_registry: Option<Arc<ToolRegistry>>,
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
            shared_registry,
            published_lease: None,
            publish_guard: None,
        }
    }

    async fn run(mut self) {
        let mut pending = VecDeque::new();
        let mut preparation = None;
        loop {
            if preparation.is_none()
                && let Some(request) = pending.pop_front()
            {
                preparation = Some(self.start_preparation(request));
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
                    if preparation.is_some() || !pending.is_empty() {
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
                RuntimeCommand::Shutdown { reply } => {
                    reject_reconciles(preparation.take(), &mut pending);
                    self.shutdown().await;
                    let _ = reply.send(());
                    return;
                }
                    }
                }
            }
        }
        self.shutdown().await;
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
        self.publish_current_generation();
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
        for session in unique_sessions(generation.servers) {
            if !current_sessions.contains(&(Arc::as_ptr(&session) as usize)) {
                session.close().await;
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
        Ok(LeaseSnapshot {
            generation: generation.id,
            tools,
            server_ids,
        })
    }

    /// 把当前 generation 的全部工具与 resource façade 整组发布到共享注册表。
    ///
    /// 发布持有 master lease：旧 Turn 持有旧代条目时 generation 不会被回收；
    /// 新一代发布后旧 master lease drop，旧 generation 等最后一个引用释放。
    fn publish_current_generation(&mut self) {
        let Some(registry) = self.shared_registry.clone() else {
            return;
        };
        let snapshot = match self.acquire_lease() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(target: "pl_core::mcp", error = %error, "failed to freeze MCP lease for publication");
                return;
            }
        };
        let lease = McpTurnLease {
            generation: snapshot.generation,
            tools: Arc::new(snapshot.tools),
            server_ids: Arc::new(snapshot.server_ids),
            guard: Arc::new(super::McpLeaseGuard {
                generation: snapshot.generation,
                handle: self.handle_clone(),
            }),
        };
        let entries = lease.registry_entries();
        match registry.publish(ToolSourceId::mcp(), entries) {
            Ok(guard) => {
                self.published_lease = Some(lease);
                self.publish_guard = Some(guard);
            }
            Err(error) => {
                tracing::warn!(target: "pl_core::mcp", error = %error, "failed to publish MCP tools to shared registry");
            }
        }
    }

    fn handle_clone(&self) -> super::McpRuntimeHandle {
        super::McpRuntimeHandle {
            commands: self.commands.clone(),
            updates: self.updates.clone(),
        }
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
        for id in retired {
            let Some(generation) = self.generations.remove(&id) else {
                continue;
            };
            for session in unique_sessions(generation.servers) {
                if Arc::strong_count(&session) == 1 {
                    session.close().await;
                }
            }
        }
    }

    async fn shutdown(&mut self) {
        self.published_lease = None;
        self.publish_guard = None;
        let generations = std::mem::take(&mut self.generations);
        let mut sessions = Vec::new();
        let mut seen = BTreeSet::new();
        for generation in generations.into_values() {
            for session in generation
                .servers
                .into_values()
                .filter_map(|server| server.session)
            {
                let identity = Arc::as_ptr(&session) as usize;
                if seen.insert(identity) {
                    sessions.push(session);
                }
            }
        }
        for session in sessions {
            session.close().await;
        }
        self.emit_update();
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
