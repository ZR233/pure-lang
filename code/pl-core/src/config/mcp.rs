use std::collections::BTreeMap;

use pl_protocol::{PureError, Result};
use serde::{Deserialize, Serialize};

use crate::turn::ToolEffect;

pub const ZHIPU_SEARCH_MCP_ID: &str = "zhipu_search";
pub const ZHIPU_READER_MCP_ID: &str = "zhipu_reader";
pub const ZHIPU_ZREAD_MCP_ID: &str = "zhipu_zread";
pub const ZHIPU_VISION_MCP_ID: &str = "zhipu_vision";

const BUILTIN_MCP_SERVERS: &[BuiltinMcpServerDefinition] = &[
    BuiltinMcpServerDefinition {
        id: ZHIPU_SEARCH_MCP_ID,
        transport: McpServerTransport::StreamableHttp,
        url: Some("https://open.bigmodel.cn/api/mcp/web_search_prime/mcp"),
        command: None,
        args: &[],
        credential: ZHIPU_CREDENTIAL,
        source_detail: "Zhipu Coding Plan",
        tool_effect: Some(ToolEffect::Read),
        env: &[],
        credential_env_var: None,
        startup_timeout_secs: None,
        tool_timeout_secs: None,
    },
    BuiltinMcpServerDefinition {
        id: ZHIPU_READER_MCP_ID,
        transport: McpServerTransport::StreamableHttp,
        url: Some("https://open.bigmodel.cn/api/mcp/web_reader/mcp"),
        command: None,
        args: &[],
        credential: ZHIPU_CREDENTIAL,
        source_detail: "Zhipu Coding Plan",
        tool_effect: Some(ToolEffect::Read),
        env: &[],
        credential_env_var: None,
        startup_timeout_secs: None,
        tool_timeout_secs: None,
    },
    BuiltinMcpServerDefinition {
        id: ZHIPU_ZREAD_MCP_ID,
        transport: McpServerTransport::StreamableHttp,
        url: Some("https://open.bigmodel.cn/api/mcp/zread/mcp"),
        command: None,
        args: &[],
        credential: ZHIPU_CREDENTIAL,
        source_detail: "Zhipu Coding Plan",
        tool_effect: Some(ToolEffect::Read),
        env: &[],
        credential_env_var: None,
        startup_timeout_secs: None,
        tool_timeout_secs: None,
    },
    BuiltinMcpServerDefinition {
        id: ZHIPU_VISION_MCP_ID,
        transport: McpServerTransport::Stdio,
        url: None,
        command: Some("npx"),
        args: &["-y", "@z_ai/mcp-server"],
        credential: ZHIPU_CREDENTIAL,
        source_detail: "Zhipu Coding Plan",
        tool_effect: Some(ToolEffect::Read),
        env: &[
            ("Z_AI_MODE", "ZHIPU"),
            // npm server 默认 32768，会显著放大简单图片冒烟的延迟和上下文。
            ("Z_AI_VISION_MODEL_MAX_TOKENS", "4096"),
        ],
        credential_env_var: Some("Z_AI_API_KEY"),
        // 首次运行可能需要由 npx 下载内置 server；后续 generation 会复用 npm cache。
        startup_timeout_secs: Some(60),
        // 上游 vision server 自身的默认请求超时为 300 秒；仅对该 server 放宽，
        // 不改变其他 MCP 的快速失败语义。
        tool_timeout_secs: Some(360),
    },
];

const ZHIPU_CREDENTIAL: BuiltinMcpCredentialSource = BuiltinMcpCredentialSource::Provider {
    preset_ids: &["zhipu-coding-plan", "zhipu"],
    endpoint_hosts: &["open.bigmodel.cn"],
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
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
    /// 建立 transport 并完成工具探测的超时秒数。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup_timeout_secs: Option<u64>,
    /// 单次工具或资源请求的超时秒数。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_timeout_secs: Option<u64>,
    /// 可选工具白名单；未配置时允许 server 暴露的全部工具。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_tools: Option<Vec<String>>,
    /// 工具黑名单，优先级高于白名单。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
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
    pub tool_effect: Option<ToolEffect>,
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

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
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
        if self.startup_timeout_secs == Some(0) {
            return Err(mcp_config_error(
                server_id,
                "startup timeout must be greater than zero",
            ));
        }
        if self.tool_timeout_secs == Some(0) {
            return Err(mcp_config_error(
                server_id,
                "tool timeout must be greater than zero",
            ));
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
                let parsed = reqwest::Url::parse(url).map_err(|error| {
                    mcp_config_error(server_id, &format!("invalid url: {error}"))
                })?;
                if !matches!(parsed.scheme(), "http" | "https") {
                    return Err(mcp_config_error(
                        server_id,
                        "streamable HTTP url must use http or https",
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
            McpServerTransport::StreamableHttp => self
                .url
                .as_deref()
                .map(redacted_http_endpoint)
                .unwrap_or_default(),
        }
    }
}

fn redacted_http_endpoint(value: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(value) else {
        return "invalid MCP endpoint".to_string();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
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
            startup_timeout_secs: None,
            tool_timeout_secs: None,
            enabled_tools: None,
            disabled_tools: Vec::new(),
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

pub fn active_mcp_server_names(
    user_servers: &BTreeMap<String, McpServerConfig>,
    builtin_states: &BTreeMap<String, BuiltinMcpServerState>,
    models: &crate::AgentModelConfig,
) -> Vec<String> {
    effective_mcp_servers(user_servers, builtin_states, models)
        .into_iter()
        .filter(|(_, server)| server.status_kind == McpServerStatusKind::Enabled)
        .map(|(server_id, _)| server_id)
        .collect()
}

pub fn builtin_mcp_server_ids() -> Vec<&'static str> {
    BUILTIN_MCP_SERVERS
        .iter()
        .map(|definition| definition.id)
        .collect()
}

pub fn is_builtin_mcp_server_id(server_id: &str) -> bool {
    BUILTIN_MCP_SERVERS
        .iter()
        .any(|definition| definition.id == server_id)
}

pub fn effective_mcp_servers(
    user_servers: &BTreeMap<String, McpServerConfig>,
    builtin_states: &BTreeMap<String, BuiltinMcpServerState>,
    models: &crate::AgentModelConfig,
) -> BTreeMap<String, EffectiveMcpServerConfig> {
    let mut servers = BTreeMap::new();
    for (server_id, server) in user_servers {
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
                tool_effect: None,
            },
        );
    }

    for definition in BUILTIN_MCP_SERVERS {
        let token = definition.credential.resolve(models);
        let builtin_enabled = builtin_states
            .get(definition.id)
            .is_none_or(|state| state.enabled);
        let status_kind = if !builtin_enabled {
            McpServerStatusKind::Disabled
        } else if token.is_some() {
            McpServerStatusKind::Enabled
        } else {
            McpServerStatusKind::MissingCredential
        };
        servers.insert(
            definition.id.to_string(),
            EffectiveMcpServerConfig {
                id: definition.id.to_string(),
                config: definition.config(token.as_deref()),
                source_kind: McpServerSourceKind::BuiltIn,
                source_label: "Built-in".to_string(),
                source_detail: Some(definition.source_detail.to_string()),
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
                bearer_token: token,
                tool_effect: definition.tool_effect,
            },
        );
    }

    servers
}

pub fn normalize_builtin_mcp_server_states(
    states: &mut BTreeMap<String, BuiltinMcpServerState>,
    models: &crate::AgentModelConfig,
) {
    states.retain(|server_id, _| is_builtin_mcp_server_id(server_id));
    for definition in BUILTIN_MCP_SERVERS {
        if definition.credential.resolve(models).is_some() {
            states
                .entry(definition.id.to_string())
                .or_insert(BuiltinMcpServerState { enabled: true });
        }
    }
}

pub fn zhipu_coding_plan_token(models: &crate::AgentModelConfig) -> Option<String> {
    ZHIPU_CREDENTIAL.resolve(models)
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
    credential: BuiltinMcpCredentialSource,
    source_detail: &'static str,
    tool_effect: Option<ToolEffect>,
    env: &'static [(&'static str, &'static str)],
    credential_env_var: Option<&'static str>,
    startup_timeout_secs: Option<u64>,
    tool_timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
enum BuiltinMcpCredentialSource {
    Provider {
        preset_ids: &'static [&'static str],
        endpoint_hosts: &'static [&'static str],
    },
}

impl BuiltinMcpCredentialSource {
    fn resolve(self, models: &crate::AgentModelConfig) -> Option<String> {
        let Self::Provider {
            preset_ids,
            endpoint_hosts,
        } = self;
        models
            .providers
            .values()
            .filter(|provider| {
                provider
                    .preset_id()
                    .is_some_and(|preset| preset_ids.contains(&preset.as_str()))
            })
            .find_map(crate::ProviderConfig::resolved_bearer_token)
            .or_else(|| {
                models.providers.values().find_map(|provider| {
                    let matches_endpoint = reqwest::Url::parse(&provider.base_url)
                        .ok()
                        .and_then(|url| url.host_str().map(str::to_string))
                        .is_some_and(|host| endpoint_hosts.contains(&host.as_str()));
                    matches_endpoint
                        .then(|| provider.resolved_bearer_token())
                        .flatten()
                })
            })
    }
}

impl BuiltinMcpServerDefinition {
    fn config(&self, zhipu_token: Option<&str>) -> McpServerConfig {
        let mut env = self
            .env
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect::<BTreeMap<_, _>>();
        if let (Some(key), Some(token)) = (self.credential_env_var, zhipu_token) {
            env.insert(key.to_string(), token.to_string());
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
            startup_timeout_secs: self.startup_timeout_secs,
            tool_timeout_secs: self.tool_timeout_secs,
            enabled_tools: None,
            disabled_tools: Vec::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{AgentModelConfig, ProviderConfig, ProviderId, builtin_provider_catalog};
    use pl_model::{ModelInfo, ProviderEndpoint};

    #[test]
    fn public_http_endpoint_removes_userinfo_query_and_fragment() {
        let server = McpServerConfig {
            transport: McpServerTransport::StreamableHttp,
            url: Some("https://user:secret@example.com/mcp?api_key=secret#private".to_string()),
            ..Default::default()
        };

        assert_eq!(server.endpoint_summary(), "https://example.com/mcp");
    }

    #[test]
    fn malformed_http_endpoint_is_not_reflected_to_public_projection() {
        let server = McpServerConfig {
            transport: McpServerTransport::StreamableHttp,
            url: Some("not a url?token=secret".to_string()),
            ..Default::default()
        };

        assert_eq!(server.endpoint_summary(), "invalid MCP endpoint");
    }

    #[test]
    fn builtin_credential_selector_uses_preset_or_compatible_endpoint_not_provider_key() {
        let mut preset = builtin_provider_catalog()
            .presets
            .into_iter()
            .find(|preset| preset.id.as_str() == "zhipu-coding-plan")
            .unwrap()
            .provider;
        preset.bearer_token = Some("preset-token".to_string());
        preset.bearer_token_env = None;
        let compatible_model = ModelInfo::fallback("compatible-model");
        let mut compatible_info = ProviderEndpoint::openai_compatible_chat(
            "Compatible",
            "https://open.bigmodel.cn/custom/v1",
        );
        compatible_info.bearer_token = Some("compatible-token".to_string());
        let compatible = ProviderConfig::from_endpoint(compatible_info, vec![compatible_model]);
        let unrelated_model = ModelInfo::fallback("unrelated-model");
        let mut unrelated_info =
            ProviderEndpoint::openai_compatible_chat("Unrelated", "https://example.com/v1");
        unrelated_info.bearer_token = Some("unrelated-token".to_string());
        let unrelated = ProviderConfig::from_endpoint(unrelated_info, vec![unrelated_model]);
        let models = AgentModelConfig {
            providers: BTreeMap::from([
                (ProviderId::new("renamed-preset").unwrap(), preset),
                (ProviderId::new("compatible").unwrap(), compatible),
                (
                    ProviderId::new("zhipu-coding-plan").unwrap(),
                    unrelated.clone(),
                ),
            ]),
            routes: BTreeMap::new(),
        };

        assert_eq!(
            zhipu_coding_plan_token(&models).as_deref(),
            Some("preset-token")
        );
        let unrelated_only = AgentModelConfig {
            providers: BTreeMap::from([(ProviderId::new("zhipu-coding-plan").unwrap(), unrelated)]),
            routes: BTreeMap::new(),
        };
        assert_eq!(zhipu_coding_plan_token(&unrelated_only), None);
    }

    #[test]
    fn builtin_zhipu_directory_declares_read_effect_and_injects_vision_secret() {
        let model = ModelInfo::fallback("compatible-model");
        let mut info =
            ProviderEndpoint::openai_compatible_chat("Compatible", "https://open.bigmodel.cn/v1");
        info.bearer_token = Some("secret".to_string());
        let models = AgentModelConfig {
            providers: BTreeMap::from([(
                ProviderId::new("compatible").unwrap(),
                ProviderConfig::from_endpoint(info, vec![model]),
            )]),
            routes: BTreeMap::new(),
        };

        let servers = effective_mcp_servers(&BTreeMap::new(), &BTreeMap::new(), &models);

        assert!(
            servers
                .values()
                .all(|server| server.tool_effect == Some(ToolEffect::Read))
        );
        assert_eq!(
            servers[ZHIPU_VISION_MCP_ID]
                .config
                .env
                .get("Z_AI_API_KEY")
                .map(String::as_str),
            Some("secret")
        );
        assert_eq!(
            servers[ZHIPU_VISION_MCP_ID].config.startup_timeout_secs,
            Some(60)
        );
        assert_eq!(
            servers[ZHIPU_VISION_MCP_ID].config.tool_timeout_secs,
            Some(360)
        );
        assert_eq!(
            servers[ZHIPU_VISION_MCP_ID].config.command.as_deref(),
            Some("npx")
        );
        assert_eq!(
            servers[ZHIPU_VISION_MCP_ID]
                .config
                .env
                .get("Z_AI_VISION_MODEL_MAX_TOKENS")
                .map(String::as_str),
            Some("4096")
        );
    }
}
