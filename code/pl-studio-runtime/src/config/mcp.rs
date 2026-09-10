//! Studio built-in MCP directory, credential selection and persisted enablement.
use pl_protocol::{PureError, Result};
use pl_tool::approval::ToolEffect;
use pl_tool::mcp::config::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
pub struct BuiltinMcpServerState {
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub enabled: bool,
}

impl Default for BuiltinMcpServerState {
    fn default() -> Self {
        Self { enabled: true }
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
    models: &pl_model::config::AgentModelConfig,
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
    models: &pl_model::config::AgentModelConfig,
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
    models: &pl_model::config::AgentModelConfig,
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

pub fn zhipu_coding_plan_token(models: &pl_model::config::AgentModelConfig) -> Option<String> {
    ZHIPU_CREDENTIAL.resolve(models)
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
    fn resolve(self, models: &pl_model::config::AgentModelConfig) -> Option<String> {
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
            .find_map(pl_model::config::ProviderConfig::resolved_bearer_token)
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

    use pl_model::config::{
        AgentModelConfig, ProviderConfig, ProviderId, builtin_provider_catalog,
    };
    use pl_model::model::ModelInfo;
    use pl_model::provider::ProviderEndpoint;

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
        let compatible_model = ModelInfo::compatible("compatible-model");
        let mut compatible_info =
            ProviderEndpoint::compatible("Compatible", "https://open.bigmodel.cn/custom/v1");
        compatible_info.bearer_token = Some("compatible-token".to_string());
        let compatible =
            ProviderConfig::from_explicit_models(compatible_info, vec![compatible_model]);
        let unrelated_model = ModelInfo::compatible("unrelated-model");
        let mut unrelated_info =
            ProviderEndpoint::compatible("Unrelated", "https://example.com/v1");
        unrelated_info.bearer_token = Some("unrelated-token".to_string());
        let unrelated = ProviderConfig::from_explicit_models(unrelated_info, vec![unrelated_model]);
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
        let model = ModelInfo::compatible("compatible-model");
        let mut info = ProviderEndpoint::compatible("Compatible", "https://open.bigmodel.cn/v1");
        info.bearer_token = Some("secret".to_string());
        let models = AgentModelConfig {
            providers: BTreeMap::from([(
                ProviderId::new("compatible").unwrap(),
                ProviderConfig::from_explicit_models(info, vec![model]),
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
