//! [`RuntimeWorker`] 的工具调用与资源查询分发,以及失败 session 标记。

use std::sync::Arc;
use std::time::Duration;

use pl_protocol::{PureError, Result};
use rmcp::model::CallToolResult;
use serde_json::{Map, Value};
use tokio::sync::mpsc;

use super::super::{McpGeneration, ResourceOperation, RuntimeCommand};
use super::RuntimeWorker;
use super::redaction::McpErrorRedactor;
use super::tools::serialize_resource_result;
use crate::mcp::ConnectedMcp;
use crate::mcp::health::McpAvailabilityKind;
use crate::time::unix_seconds;

type ResourceSession = (String, Arc<ConnectedMcp>, Duration, McpErrorRedactor);

impl RuntimeWorker {
    pub(super) fn spawn_tool_call(
        &self,
        generation: McpGeneration,
        server_id: String,
        raw_name: String,
        arguments: Value,
        reply: tokio::sync::oneshot::Sender<Result<CallToolResult>>,
    ) {
        let session = self.session(generation, &server_id);
        let commands = self.commands.clone();
        tokio::spawn(async move {
            let result = match session {
                Ok((session, request_timeout, redactor)) => {
                    tokio::time::timeout(request_timeout, session.call_tool(raw_name, arguments))
                        .await
                        .map_err(|_| {
                            format!(
                                "request timed out after {} seconds",
                                request_timeout.as_secs()
                            )
                        })
                        .and_then(|result| {
                            result.map_err(|error| redactor.redact(error.to_string()))
                        })
                        .map_err(|message| {
                            let _ = commands.send(RuntimeCommand::MarkUnavailable {
                                generation,
                                server_id: server_id.clone(),
                                error: message.clone(),
                            });
                            PureError::ToolExecutionFailed {
                                tool: server_id,
                                error: message,
                            }
                        })
                }
                Err(error) => Err(error),
            };
            let _ = reply.send(result);
        });
    }

    pub(super) fn spawn_resource_query(
        &self,
        generation: McpGeneration,
        server_id: Option<String>,
        operation: ResourceOperation,
        reply: tokio::sync::oneshot::Sender<Result<Value>>,
    ) {
        let sessions = self.sessions(generation, server_id.as_deref());
        let commands = self.commands.clone();
        tokio::spawn(async move {
            let result = resource_query(sessions, generation, operation, commands).await;
            let _ = reply.send(result);
        });
    }

    fn session(
        &self,
        generation: McpGeneration,
        server_id: &str,
    ) -> Result<(Arc<ConnectedMcp>, Duration, McpErrorRedactor)> {
        self.generations
            .get(&generation)
            .and_then(|generation| generation.servers.get(server_id))
            .and_then(|server| {
                (server.availability == McpAvailabilityKind::Available)
                    .then(|| server.session.clone())
                    .flatten()
                    .map(|session| (session, server.request_timeout, server.redactor.clone()))
            })
            .ok_or_else(|| PureError::ToolExecutionFailed {
                tool: "mcp".to_string(),
                error: format!(
                    "MCP server '{server_id}' is unavailable in generation {}",
                    generation.0
                ),
            })
    }

    fn sessions(
        &self,
        generation: McpGeneration,
        server_id: Option<&str>,
    ) -> Result<Vec<ResourceSession>> {
        let state =
            self.generations
                .get(&generation)
                .ok_or_else(|| PureError::ToolExecutionFailed {
                    tool: "mcp".to_string(),
                    error: format!("MCP generation {} is unavailable", generation.0),
                })?;
        match server_id {
            Some(server_id) => {
                let (session, request_timeout, redactor) = self.session(generation, server_id)?;
                Ok(vec![(
                    server_id.to_string(),
                    session,
                    request_timeout,
                    redactor,
                )])
            }
            None => Ok(state
                .servers
                .values()
                .filter_map(|server| {
                    (server.availability == McpAvailabilityKind::Available)
                        .then(|| server.session.clone())
                        .flatten()
                        .map(|session| {
                            (
                                server.descriptor.id.clone(),
                                session,
                                server.request_timeout,
                                server.redactor.clone(),
                            )
                        })
                })
                .collect()),
        }
    }

    pub(super) fn mark_unavailable(
        &mut self,
        generation: McpGeneration,
        server_id: &str,
        error: String,
    ) {
        let failed = self
            .generations
            .get(&generation)
            .and_then(|generation| generation.servers.get(server_id))
            .and_then(|server| server.session.clone());
        for generation in self.generations.values_mut() {
            for server in generation.servers.values_mut() {
                let same_session = failed.as_ref().is_some_and(|failed| {
                    server
                        .session
                        .as_ref()
                        .is_some_and(|session| Arc::ptr_eq(failed, session))
                });
                if same_session {
                    server.availability = McpAvailabilityKind::Unavailable;
                    server.message = Some(error.clone());
                    server.last_checked_at = Some(unix_seconds());
                }
            }
        }
        self.emit_update();
    }
}

async fn resource_query(
    sessions: Result<Vec<ResourceSession>>,
    generation: McpGeneration,
    operation: ResourceOperation,
    commands: mpsc::UnboundedSender<RuntimeCommand>,
) -> Result<Value> {
    let sessions = sessions?;
    let explicit = sessions.len() == 1;
    let mut values = Map::new();
    for (server_id, session, request_timeout, redactor) in sessions {
        let result = tokio::time::timeout(request_timeout, async {
            match &operation {
                ResourceOperation::ListResources { cursor } => session
                    .list_resources(cursor.clone())
                    .await
                    .and_then(serialize_resource_result),
                ResourceOperation::ListResourceTemplates { cursor } => session
                    .list_resource_templates(cursor.clone())
                    .await
                    .and_then(serialize_resource_result),
                ResourceOperation::ReadResource { uri } => session
                    .read_resource(uri.clone())
                    .await
                    .and_then(serialize_resource_result),
            }
        })
        .await;
        let value = result
            .map_err(|_| {
                format!(
                    "request timed out after {} seconds",
                    request_timeout.as_secs()
                )
            })
            .and_then(|result| result.map_err(|error| redactor.redact(error.to_string())))
            .map_err(|message| {
                let _ = commands.send(RuntimeCommand::MarkUnavailable {
                    generation,
                    server_id: server_id.clone(),
                    error: message.clone(),
                });
                PureError::ToolExecutionFailed {
                    tool: server_id.clone(),
                    error: message,
                }
            })?;
        if explicit {
            return Ok(value);
        }
        values.insert(server_id, value);
    }
    Ok(Value::Object(values))
}
