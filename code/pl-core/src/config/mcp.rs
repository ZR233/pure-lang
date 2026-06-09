use std::collections::BTreeMap;

use pl_protocol::{PureError, Result};
use serde::{Deserialize, Serialize};

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

pub fn validate_mcp_servers(servers: &BTreeMap<String, McpServerConfig>) -> Result<()> {
    for (server_id, server) in servers {
        server.validate(server_id)?;
    }
    Ok(())
}

pub fn active_mcp_server_names(servers: &BTreeMap<String, McpServerConfig>) -> Vec<String> {
    servers
        .iter()
        .filter(|(_, server)| server.enabled)
        .map(|(server_id, _)| server_id.clone())
        .collect()
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
