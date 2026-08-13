use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use futures::future::BoxFuture;
use futures::stream::{FuturesUnordered, StreamExt};
use pl_protocol::{PureError, Result};

use super::super::{McpGeneration, ReconcilePolicy};
use super::redaction::McpErrorRedactor;
use super::{
    RuntimeGeneration, RuntimeServer, assign_tool_descriptors, configured_startup_timeout,
    configured_tool_timeout, filter_tool_definitions, server_descriptor, server_fingerprint,
};
use crate::config::{EffectiveMcpServerConfig, McpServerStatusKind};
use crate::mcp::health::McpAvailabilityKind;
use crate::mcp::{McpConnectRequest, McpConnector};

pub(super) struct PendingReconcile {
    pub(super) servers: BTreeMap<String, EffectiveMcpServerConfig>,
    pub(super) policy: ReconcilePolicy,
    pub(super) reply: tokio::sync::oneshot::Sender<Result<()>>,
}

pub(super) struct ActivePreparation {
    pub(super) future: BoxFuture<'static, RuntimeGeneration>,
    pub(super) reply: tokio::sync::oneshot::Sender<Result<()>>,
}

pub(super) async fn await_preparation(
    preparation: &mut Option<ActivePreparation>,
) -> RuntimeGeneration {
    preparation
        .as_mut()
        .expect("guarded MCP preparation must exist")
        .future
        .as_mut()
        .await
}

pub(super) fn reject_reconciles(
    preparation: Option<ActivePreparation>,
    pending: &mut VecDeque<PendingReconcile>,
) {
    if let Some(preparation) = preparation {
        let _ = preparation.reply.send(Err(runtime_stopping_error()));
    }
    for request in pending.drain(..) {
        let _ = request.reply.send(Err(runtime_stopping_error()));
    }
}

fn runtime_stopping_error() -> PureError {
    PureError::ToolExecutionFailed {
        tool: "mcp".to_string(),
        error: "MCP runtime stopped while preparing a generation".to_string(),
    }
}

pub(super) async fn prepare_generation(
    connector: McpConnector,
    generation_id: McpGeneration,
    servers: BTreeMap<String, EffectiveMcpServerConfig>,
    mut reusable: BTreeMap<String, RuntimeServer>,
) -> RuntimeGeneration {
    let mut next = RuntimeGeneration::empty(generation_id);
    let mut connecting = FuturesUnordered::new();
    for (server_id, config) in servers {
        let fingerprint = server_fingerprint(&config);
        let server = match config.status_kind {
            McpServerStatusKind::Disabled => Some(RuntimeServer::terminal(
                &config,
                fingerprint,
                McpAvailabilityKind::Disabled,
                Some("MCP server is disabled in configuration".to_string()),
            )),
            McpServerStatusKind::MissingCredential => Some(RuntimeServer::terminal(
                &config,
                fingerprint,
                McpAvailabilityKind::MissingCredential,
                config.status_message.clone(),
            )),
            McpServerStatusKind::Enabled => reusable.remove(&server_id).filter(|server| {
                server.fingerprint == fingerprint
                    && server.availability == McpAvailabilityKind::Available
            }),
        };
        if let Some(server) = server {
            next.servers.insert(server.descriptor.id.clone(), server);
            continue;
        }
        let connector = connector.clone();
        connecting.push(async move {
            let server = connect_server(&connector, server_id, config, fingerprint).await;
            (server.descriptor.id.clone(), server)
        });
    }
    while let Some((server_id, server)) = connecting.next().await {
        next.servers.insert(server_id, server);
    }
    assign_tool_descriptors(&mut next.servers);
    next
}

async fn connect_server(
    connector: &McpConnector,
    server_id: String,
    config: EffectiveMcpServerConfig,
    fingerprint: u64,
) -> RuntimeServer {
    let descriptor = server_descriptor(&config);
    let redactor = McpErrorRedactor::new(&config);
    let startup_timeout = configured_startup_timeout(config.config.startup_timeout_secs);
    let request_timeout = configured_tool_timeout(config.config.tool_timeout_secs);
    let connected = tokio::time::timeout(
        startup_timeout,
        connector.connect(McpConnectRequest {
            server_id,
            server: config.clone(),
        }),
    )
    .await;
    let session = match connected {
        Ok(Ok(session)) => Arc::new(session),
        Ok(Err(error)) => {
            return RuntimeServer::unavailable(
                descriptor,
                fingerprint,
                redactor.redact(error.to_string()),
                redactor,
            );
        }
        Err(_) => {
            return RuntimeServer::unavailable(
                descriptor,
                fingerprint,
                format!(
                    "MCP health check timed out after {} seconds",
                    startup_timeout.as_secs()
                ),
                redactor,
            );
        }
    };
    match tokio::time::timeout(startup_timeout, session.list_tools()).await {
        Ok(Ok(tools)) => RuntimeServer::available(
            descriptor,
            fingerprint,
            session,
            filter_tool_definitions(tools, &config.config),
            request_timeout,
            config.tool_effect,
            redactor,
        ),
        Ok(Err(error)) => {
            session.close().await;
            let message = redactor.redact(error.to_string());
            RuntimeServer::unavailable(descriptor, fingerprint, message, redactor)
        }
        Err(_) => {
            session.close().await;
            RuntimeServer::unavailable(
                descriptor,
                fingerprint,
                format!(
                    "MCP tool discovery timed out after {} seconds",
                    startup_timeout.as_secs()
                ),
                redactor,
            )
        }
    }
}
