use crate::mcp::config::EffectiveMcpServerConfig;

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
