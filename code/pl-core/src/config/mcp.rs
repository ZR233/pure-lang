use std::collections::BTreeMap;

use pl_model::{ProviderKind, ZHIPU_CODING_PLAN_BASE_URL};
use pl_protocol::{PureError, Result};
use serde::{Deserialize, Serialize};

pub const ZHIPU_SEARCH_MCP_ID: &str = "zhipu_search";
pub const ZHIPU_READER_MCP_ID: &str = "zhipu_reader";
pub const ZHIPU_ZREAD_MCP_ID: &str = "zhipu_zread";
pub const ZHIPU_VISION_MCP_ID: &str = "zhipu_vision";

const BUILTIN_ZHIPU_MCP_SERVERS: &[BuiltinMcpServerDefinition] = &[
    BuiltinMcpServerDefinition {
        id: ZHIPU_SEARCH_MCP_ID,
        transport: McpServerTransport::StreamableHttp,
        url: Some("https://open.bigmodel.cn/api/mcp/web_search_prime/mcp"),
        command: None,
        args: &[],
    },
    BuiltinMcpServerDefinition {
        id: ZHIPU_READER_MCP_ID,
        transport: McpServerTransport::StreamableHttp,
        url: Some("https://open.bigmodel.cn/api/mcp/web_reader/mcp"),
        command: None,
        args: &[],
    },
    BuiltinMcpServerDefinition {
        id: ZHIPU_ZREAD_MCP_ID,
        transport: McpServerTransport::StreamableHttp,
        url: Some("https://open.bigmodel.cn/api/mcp/zread/mcp"),
        command: None,
        args: &[],
    },
    BuiltinMcpServerDefinition {
        id: ZHIPU_VISION_MCP_ID,
        transport: McpServerTransport::Stdio,
        url: None,
        command: Some("npx"),
        args: &["-y", "@z_ai/mcp-server"],
    },
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServerConfig {
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "McpServerTransport::is_default")]
    pub transport: McpServerTransport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer_token_env_var: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuiltinMcpServerState {
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveMcpServerConfig {
    pub id: String,
    pub config: McpServerConfig,
    pub source_kind: McpServerSourceKind,
    pub source_label: String,
    pub source_detail: Option<String>,
    pub status_kind: McpServerStatusKind,
    pub status_message: Option<String>,
    pub mutation_policy: McpServerMutationPolicy,
    pub bearer_token: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpServerSourceKind {
    User,
    BuiltIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpServerStatusKind {
    Enabled,
    Disabled,
    MissingCredential,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpServerMutationPolicy {
    UserEditable,
    LockedIdentity,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum McpServerTransport {
    #[default]
    Stdio,
    StreamableHttp,
}

impl McpServerConfig {
    pub fn validate(&self, server_id: &str) -> Result<()> {
        validate_mcp_identifier(server_id, "MCP server id")?;
        if !self.enabled {
            return Ok(());
        }
        match self.transport {
            McpServerTransport::Stdio => {
                let Some(command) = self.command.as_deref().map(str::trim) else {
                    return Err(mcp_config_error(server_id, "stdio command is required"));
                };
                if command.is_empty() {
                    return Err(mcp_config_error(server_id, "stdio command is required"));
                }
                for key in self.env.keys() {
                    validate_env_key(server_id, key)?;
                }
            }
            McpServerTransport::StreamableHttp => {
                let Some(url) = self.url.as_deref().map(str::trim) else {
                    return Err(mcp_config_error(
                        server_id,
                        "streamable HTTP url is required",
                    ));
                };
                if url.is_empty() {
                    return Err(mcp_config_error(
                        server_id,
                        "streamable HTTP url is required",
                    ));
                }
                if let Some(token_env) = self.bearer_token_env_var.as_deref() {
                    validate_env_key(server_id, token_env)?;
                }
            }
        }
        Ok(())
    }

    pub fn endpoint_summary(&self) -> String {
        match self.transport {
            McpServerTransport::Stdio => self.command.clone().unwrap_or_default(),
            McpServerTransport::StreamableHttp => self.url.clone().unwrap_or_default(),
        }
    }
}

impl McpServerTransport {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::StreamableHttp => "streamableHttp",
        }
    }
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            transport: McpServerTransport::Stdio,
            command: None,
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            url: None,
            bearer_token_env_var: None,
            headers: BTreeMap::new(),
        }
    }
}

impl Default for BuiltinMcpServerState {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl McpServerSourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::BuiltIn => "builtIn",
        }
    }
}

impl McpServerStatusKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::MissingCredential => "missingCredential",
        }
    }
}

impl McpServerMutationPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UserEditable => "userEditable",
            Self::LockedIdentity => "lockedIdentity",
        }
    }
}

pub fn validate_mcp_servers(servers: &BTreeMap<String, McpServerConfig>) -> Result<()> {
    for (server_id, server) in servers {
        if is_builtin_mcp_server_id(server_id) {
            return Err(PureError::ConfigError(format!(
                "mcp server id '{server_id}' is reserved for a built-in server"
            )));
        }
        server.validate(server_id)?;
    }
    Ok(())
}

pub fn validate_builtin_mcp_server_states(
    states: &BTreeMap<String, BuiltinMcpServerState>,
) -> Result<()> {
    for server_id in states.keys() {
        if !is_builtin_mcp_server_id(server_id) {
            return Err(PureError::ConfigError(format!(
                "unknown built-in mcp server id: {server_id}"
            )));
        }
    }
    Ok(())
}

pub fn active_mcp_server_names(config: &super::PureConfig) -> Vec<String> {
    effective_mcp_servers(config)
        .into_iter()
        .filter(|(_, server)| server.status_kind == McpServerStatusKind::Enabled)
        .map(|(server_id, _)| server_id)
        .collect()
}

pub fn builtin_mcp_server_ids() -> &'static [&'static str] {
    &[
        ZHIPU_SEARCH_MCP_ID,
        ZHIPU_READER_MCP_ID,
        ZHIPU_ZREAD_MCP_ID,
        ZHIPU_VISION_MCP_ID,
    ]
}

pub fn is_builtin_mcp_server_id(server_id: &str) -> bool {
    builtin_mcp_server_ids().contains(&server_id)
}

pub fn effective_mcp_servers(
    config: &super::PureConfig,
) -> BTreeMap<String, EffectiveMcpServerConfig> {
    let mut servers = BTreeMap::new();
    for (server_id, server) in &config.mcp_servers {
        let status_kind = if server.enabled {
            McpServerStatusKind::Enabled
        } else {
            McpServerStatusKind::Disabled
        };
        servers.insert(
            server_id.clone(),
            EffectiveMcpServerConfig {
                id: server_id.clone(),
                config: server.clone(),
                source_kind: McpServerSourceKind::User,
                source_label: "User".to_string(),
                source_detail: None,
                status_kind,
                status_message: None,
                mutation_policy: McpServerMutationPolicy::UserEditable,
                bearer_token: None,
            },
        );
    }

    let zhipu_token = zhipu_coding_plan_token(config);
    for definition in BUILTIN_ZHIPU_MCP_SERVERS {
        let status_kind = if zhipu_token.is_some() {
            McpServerStatusKind::Enabled
        } else {
            McpServerStatusKind::MissingCredential
        };
        servers.insert(
            definition.id.to_string(),
            EffectiveMcpServerConfig {
                id: definition.id.to_string(),
                config: definition.config(zhipu_token.as_deref()),
                source_kind: McpServerSourceKind::BuiltIn,
                source_label: "Built-in".to_string(),
                source_detail: Some("Zhipu Coding Plan".to_string()),
                status_kind,
                status_message: match status_kind {
                    McpServerStatusKind::Enabled => Some(
                        "Using the configured Zhipu Coding Plan or Zhipu provider token"
                            .to_string(),
                    ),
                    McpServerStatusKind::MissingCredential => Some(
                        "Configure a Zhipu Coding Plan or Zhipu provider token to enable this server"
                            .to_string(),
                    ),
                    McpServerStatusKind::Disabled => None,
                },
                mutation_policy: McpServerMutationPolicy::LockedIdentity,
                bearer_token: zhipu_token.clone(),
            },
        );
    }

    servers
}

pub fn builtin_mcp_server_states_are_default(
    states: &BTreeMap<String, BuiltinMcpServerState>,
) -> bool {
    states.is_empty() || states.values().all(|state| state.enabled)
}

pub fn normalize_builtin_mcp_server_states(config: &mut super::PureConfig) {
    config
        .builtin_mcp_servers
        .retain(|server_id, _| is_builtin_mcp_server_id(server_id));
    if zhipu_coding_plan_token(config).is_some() {
        for server_id in builtin_mcp_server_ids() {
            config.builtin_mcp_servers.insert(
                (*server_id).to_string(),
                BuiltinMcpServerState { enabled: true },
            );
        }
    }
}

pub fn zhipu_coding_plan_token(config: &super::PureConfig) -> Option<String> {
    config
        .providers
        .iter()
        .find_map(|(provider_key, provider)| {
            is_zhipu_coding_plan_provider(provider_key, provider)
                .then(|| provider_token(provider))
                .flatten()
        })
        .or_else(|| {
            config.providers.iter().find_map(|(_, provider)| {
                (provider.provider_kind == ProviderKind::Zhipu)
                    .then(|| provider_token(provider))
                    .flatten()
            })
        })
}

fn is_zhipu_coding_plan_provider(provider_key: &str, provider: &super::ProviderConfig) -> bool {
    provider.provider_kind == ProviderKind::Zhipu
        && (provider_key == "zhipu-coding-plan"
            || normalized_base_url(&provider.base_url) == ZHIPU_CODING_PLAN_BASE_URL)
}

fn provider_token(provider: &super::ProviderConfig) -> Option<String> {
    provider
        .bearer_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
}

fn normalized_base_url(value: &str) -> &str {
    value.trim().trim_end_matches('/')
}

pub fn validate_mcp_identifier(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(PureError::ConfigError(format!("{label} is required")));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        return Err(PureError::ConfigError(format!(
            "{label} '{value}' may only contain ASCII letters, digits, '_' and '-'"
        )));
    }
    Ok(())
}

fn validate_env_key(server_id: &str, key: &str) -> Result<()> {
    if key.trim().is_empty() {
        return Err(mcp_config_error(
            server_id,
            "environment variable key is required",
        ));
    }
    Ok(())
}

fn mcp_config_error(server_id: &str, message: &str) -> PureError {
    PureError::ConfigError(format!("mcp server '{server_id}': {message}"))
}

#[derive(Debug, Clone, Copy)]
struct BuiltinMcpServerDefinition {
    id: &'static str,
    transport: McpServerTransport,
    url: Option<&'static str>,
    command: Option<&'static str>,
    args: &'static [&'static str],
}

impl BuiltinMcpServerDefinition {
    fn config(&self, zhipu_token: Option<&str>) -> McpServerConfig {
        let mut env = BTreeMap::new();
        if self.id == ZHIPU_VISION_MCP_ID {
            env.insert("Z_AI_MODE".to_string(), "ZHIPU".to_string());
            if let Some(token) = zhipu_token {
                env.insert("Z_AI_API_KEY".to_string(), token.to_string());
            }
        }
        McpServerConfig {
            enabled: zhipu_token.is_some(),
            transport: self.transport,
            command: self.command.map(ToOwned::to_owned),
            args: self.args.iter().map(|arg| (*arg).to_string()).collect(),
            env,
            cwd: None,
            url: self.url.map(ToOwned::to_owned),
            bearer_token_env_var: None,
            headers: BTreeMap::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}
