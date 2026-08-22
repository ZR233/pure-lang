//! MCP 运行态 server 与 generation 结构,以及 descriptor/fingerprint 投影。

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use pl_protocol::McpServerDescriptor;
use rmcp::model::Tool;
use serde_json::Value;

use super::super::{McpGeneration, McpResetScope, McpRuntimeToolDescriptor, McpToolSafetyHints};
use super::redaction::McpErrorRedactor;
use super::tools::{PROBE_TIMEOUT, configured_tool_timeout, serialize_optional};
use crate::config::{EffectiveMcpServerConfig, McpServerSourceKind};
use crate::mcp::ConnectedMcp;
use crate::mcp::health::McpAvailabilityKind;
use crate::mcp::naming::assign_exposed_tool_names;
use crate::time::unix_seconds;
use crate::turn::ToolEffect;

pub(super) struct RuntimeGeneration {
    pub(super) id: McpGeneration,
    pub(super) servers: BTreeMap<String, RuntimeServer>,
    pub(super) leases: usize,
    pub(super) retired: bool,
}

impl RuntimeGeneration {
    pub(super) fn empty(id: McpGeneration) -> Self {
        Self {
            id,
            servers: BTreeMap::new(),
            leases: 0,
            retired: false,
        }
    }
}

pub(super) fn reset_failed(scope: &McpResetScope, generation: &RuntimeGeneration) -> bool {
    match scope {
        McpResetScope::Server { server_id } => generation
            .servers
            .get(server_id)
            .is_none_or(|server| server.availability == McpAvailabilityKind::Unavailable),
        McpResetScope::All => generation
            .servers
            .values()
            .any(|server| server.availability == McpAvailabilityKind::Unavailable),
    }
}

pub(super) struct RuntimeServer {
    pub(super) descriptor: McpServerDescriptor,
    pub(super) fingerprint: u64,
    pub(super) availability: McpAvailabilityKind,
    pub(super) message: Option<String>,
    pub(super) last_checked_at: Option<i64>,
    pub(super) session: Option<Arc<ConnectedMcp>>,
    pub(super) definitions: Vec<Tool>,
    pub(super) tools: Vec<McpRuntimeToolDescriptor>,
    pub(super) request_timeout: Duration,
    pub(super) tool_effect: Option<ToolEffect>,
    pub(super) redactor: McpErrorRedactor,
}

impl Clone for RuntimeServer {
    fn clone(&self) -> Self {
        Self {
            descriptor: self.descriptor.clone(),
            fingerprint: self.fingerprint,
            availability: self.availability,
            message: self.message.clone(),
            last_checked_at: self.last_checked_at,
            session: self.session.clone(),
            definitions: self.definitions.clone(),
            tools: self.tools.clone(),
            request_timeout: self.request_timeout,
            tool_effect: self.tool_effect,
            redactor: self.redactor.clone(),
        }
    }
}

impl RuntimeServer {
    pub(super) fn terminal(
        config: &EffectiveMcpServerConfig,
        fingerprint: u64,
        availability: McpAvailabilityKind,
        message: Option<String>,
    ) -> Self {
        Self {
            descriptor: server_descriptor(config),
            fingerprint,
            availability,
            message,
            last_checked_at: None,
            session: None,
            definitions: Vec::new(),
            tools: Vec::new(),
            request_timeout: configured_tool_timeout(config.config.tool_timeout_secs),
            tool_effect: config.tool_effect,
            redactor: McpErrorRedactor::new(config),
        }
    }

    pub(super) fn available(
        descriptor: McpServerDescriptor,
        fingerprint: u64,
        session: Arc<ConnectedMcp>,
        definitions: Vec<Tool>,
        request_timeout: Duration,
        tool_effect: Option<ToolEffect>,
        redactor: McpErrorRedactor,
    ) -> Self {
        Self {
            descriptor,
            fingerprint,
            availability: McpAvailabilityKind::Available,
            message: Some(format!("Available with {} tools", definitions.len())),
            last_checked_at: Some(unix_seconds()),
            session: Some(session),
            definitions,
            tools: Vec::new(),
            request_timeout,
            tool_effect,
            redactor,
        }
    }

    pub(super) fn unavailable(
        descriptor: McpServerDescriptor,
        fingerprint: u64,
        error: String,
        redactor: McpErrorRedactor,
    ) -> Self {
        Self {
            descriptor,
            fingerprint,
            availability: McpAvailabilityKind::Unavailable,
            message: Some(error),
            last_checked_at: Some(unix_seconds()),
            session: None,
            definitions: Vec::new(),
            tools: Vec::new(),
            request_timeout: PROBE_TIMEOUT,
            tool_effect: None,
            redactor,
        }
    }
}

pub(super) fn assign_tool_descriptors(servers: &mut BTreeMap<String, RuntimeServer>) {
    let names = assign_exposed_tool_names(servers.iter().flat_map(|(server_id, server)| {
        server
            .definitions
            .iter()
            .map(move |definition| (server_id.as_str(), definition.name.as_ref()))
    }));
    let mut names = names.into_iter();
    for server in servers.values_mut() {
        server.tools = server
            .definitions
            .iter()
            .map(|definition| {
                let annotations = serialize_optional(&definition.annotations);
                let safety_hints = McpToolSafetyHints::parse(annotations.as_ref());
                // server 配置显式声明的 effect 优先；否则只有 readOnlyHint=true
                // 推导为 Read。destructiveHint 不映射写 effect，保持保守 None。
                let effect = server.tool_effect.or({
                    (safety_hints.read_only_hint == Some(true)).then_some(ToolEffect::Read)
                });
                McpRuntimeToolDescriptor {
                    server_id: server.descriptor.id.clone(),
                    raw_name: definition.name.to_string(),
                    exposed_name: names.next().expect("every MCP tool receives a name"),
                    description: definition
                        .description
                        .as_deref()
                        .unwrap_or_default()
                        .to_string(),
                    input_schema: Value::Object(definition.input_schema.as_ref().clone()),
                    output_schema: definition
                        .output_schema
                        .as_ref()
                        .map(|schema| Value::Object(schema.as_ref().clone())),
                    annotations,
                    icons: serialize_optional(&definition.icons),
                    metadata: serialize_optional(&definition.meta),
                    safety_hints,
                    effect,
                }
            })
            .collect();
    }
}

pub(super) fn unique_sessions(servers: BTreeMap<String, RuntimeServer>) -> Vec<Arc<ConnectedMcp>> {
    let mut seen = BTreeSet::new();
    servers
        .into_values()
        .filter_map(|server| server.session)
        .filter(|session| seen.insert(Arc::as_ptr(session) as usize))
        .collect()
}

pub(super) fn server_descriptor(config: &EffectiveMcpServerConfig) -> McpServerDescriptor {
    McpServerDescriptor {
        id: config.id.clone(),
        source: config.source_kind.as_str().to_string(),
        transport: config.config.transport.as_str().to_string(),
        endpoint: config.config.endpoint_summary(),
        built_in: config.source_kind == McpServerSourceKind::BuiltIn,
    }
}

pub(super) fn server_fingerprint(server: &EffectiveMcpServerConfig) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    server.config.hash(&mut hasher);
    server.status_kind.as_str().hash(&mut hasher);
    server.source_kind.as_str().hash(&mut hasher);
    server.bearer_token.hash(&mut hasher);
    server.tool_effect.hash(&mut hasher);
    hasher.finish()
}
