use crate::config::EffectiveMcpServerConfig;

/// MCP Host 错误进入 health、trace 或模型输出前的统一脱敏器。
#[derive(Debug, Clone, Default)]
pub(super) struct McpErrorRedactor {
    replacements: Vec<(String, String)>,
}

impl McpErrorRedactor {
    pub(super) fn new(server: &EffectiveMcpServerConfig) -> Self {
        let mut replacements = Vec::new();
        if let Some(url) = server
            .config
            .url
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            replacements.push((url.to_string(), server.config.endpoint_summary()));
        }
        if let Some(token) = server
            .bearer_token
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            replacements.push((token.to_string(), "[redacted]".to_string()));
        }
        for value in server
            .config
            .headers
            .values()
            .chain(server.config.env.values())
            .filter(|value| !value.is_empty())
        {
            replacements.push((value.clone(), "[redacted]".to_string()));
        }
        replacements.sort_by_key(|item| std::cmp::Reverse(item.0.len()));
        replacements.dedup_by(|left, right| left.0 == right.0);
        Self { replacements }
    }

    pub(super) fn redact(&self, value: impl Into<String>) -> String {
        let mut redacted = value.into();
        for (secret, replacement) in &self.replacements {
            redacted = redacted.replace(secret, replacement);
        }
        redacted
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::config::{
        McpServerConfig, McpServerMutationPolicy, McpServerSourceKind, McpServerStatusKind,
        McpServerTransport,
    };

    #[test]
    fn secrets_and_raw_endpoint_are_removed_from_host_errors() {
        let server = EffectiveMcpServerConfig {
            id: "future".to_string(),
            config: McpServerConfig {
                transport: McpServerTransport::StreamableHttp,
                url: Some("https://example.com/mcp?token=query-secret".to_string()),
                env: BTreeMap::from([("SECRET".to_string(), "env-secret".to_string())]),
                headers: BTreeMap::from([("X-Key".to_string(), "header-secret".to_string())]),
                ..Default::default()
            },
            source_kind: McpServerSourceKind::User,
            source_label: "User".to_string(),
            source_detail: None,
            status_kind: McpServerStatusKind::Enabled,
            status_message: None,
            mutation_policy: McpServerMutationPolicy::UserEditable,
            bearer_token: Some("bearer-secret".to_string()),
            tool_effect: None,
        };

        let redacted = McpErrorRedactor::new(&server).redact(format!(
            "{} env-secret header-secret bearer-secret",
            server.config.url.as_deref().unwrap()
        ));

        assert_eq!(
            redacted,
            "https://example.com/mcp [redacted] [redacted] [redacted]"
        );
    }
}
