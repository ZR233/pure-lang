//! Resolved MCP connection options. Provider and product policy belong to the host.
use crate::approval::ToolEffect;
use pl_protocol::{PureError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
}
