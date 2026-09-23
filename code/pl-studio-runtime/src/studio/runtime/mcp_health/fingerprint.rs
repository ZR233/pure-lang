//! MCP effective / public fingerprint 计算。

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use crate::config::EffectiveMcpServerConfig;

pub(super) fn effective_mcp_fingerprint(
    servers: &BTreeMap<String, EffectiveMcpServerConfig>,
) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (server_id, server) in servers {
        server_id.hash(&mut hasher);
        server.config.hash(&mut hasher);
        server.status_kind.as_str().hash(&mut hasher);
        server.source_kind.as_str().hash(&mut hasher);
        server.bearer_token.hash(&mut hasher);
        server.tool_effect.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

/// 公开 fingerprint 仅描述可安全公开的配置结构；credential 值只参与内部
/// effective fingerprint，确保轮换 credential 会 reconcile，却不会产生可关联的公开值。
pub(super) fn public_mcp_fingerprint(
    servers: &BTreeMap<String, EffectiveMcpServerConfig>,
) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (server_id, server) in servers {
        server_id.hash(&mut hasher);
        server.config.enabled.hash(&mut hasher);
        server.config.transport.hash(&mut hasher);
        server.config.command.hash(&mut hasher);
        server.config.args.len().hash(&mut hasher);
        server
            .config
            .env
            .keys()
            .for_each(|key| key.hash(&mut hasher));
        server.config.cwd.hash(&mut hasher);
        server.config.endpoint_summary().hash(&mut hasher);
        server.config.bearer_token_env_var.hash(&mut hasher);
        server
            .config
            .headers
            .keys()
            .for_each(|key| key.hash(&mut hasher));
        server.config.startup_timeout_secs.hash(&mut hasher);
        server.config.tool_timeout_secs.hash(&mut hasher);
        server.config.enabled_tools.hash(&mut hasher);
        server.config.disabled_tools.hash(&mut hasher);
        server.status_kind.as_str().hash(&mut hasher);
        server.source_kind.as_str().hash(&mut hasher);
        server.tool_effect.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}
