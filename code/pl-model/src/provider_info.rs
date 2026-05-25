use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;

const DEFAULT_STREAM_IDLE_TIMEOUT_MS: u64 = 300_000;
const DEFAULT_STREAM_MAX_RETRIES: u32 = 5;
const DEFAULT_REQUEST_MAX_RETRIES: u32 = 4;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub name: String,
    pub base_url: Option<String>,
    pub env_key: Option<String>,
    pub env_key_instructions: Option<String>,
    #[serde(default)]
    pub default_model: String,
    pub bearer_token: Option<String>,
    pub auth_command: Option<AuthCommand>,
    #[serde(default)]
    pub wire_api: WireApi,
    pub http_headers: Option<HashMap<String, String>>,
    pub env_http_headers: Option<HashMap<String, String>>,
    pub request_max_retries: Option<u32>,
    pub stream_max_retries: Option<u32>,
    pub stream_idle_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthCommand {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireApi {
    #[default]
    Responses,
    Chat,
}

impl fmt::Display for WireApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Responses => f.write_str("responses"),
            Self::Chat => f.write_str("chat"),
        }
    }
}

impl ProviderInfo {
    pub fn default_provider() -> Self {
        Self::deepseek(None)
    }

    pub fn openai(base_url: Option<String>) -> Self {
        Self {
            name: "OpenAI".into(),
            base_url: base_url.or_else(|| Some("https://api.openai.com/v1".into())),
            default_model: "gpt-5.5".into(),
            wire_api: WireApi::Responses,
            ..Default::default()
        }
    }

    pub fn deepseek(base_url: Option<String>) -> Self {
        Self {
            name: "DeepSeek".into(),
            base_url: base_url.or_else(|| Some("https://api.deepseek.com".into())),
            default_model: "deepseek-v4-flash".into(),
            wire_api: WireApi::Chat,
            ..Default::default()
        }
    }

    pub fn request_max_retries(&self) -> u32 {
        self.request_max_retries
            .unwrap_or(DEFAULT_REQUEST_MAX_RETRIES)
    }

    pub fn stream_max_retries(&self) -> u32 {
        self.stream_max_retries
            .unwrap_or(DEFAULT_STREAM_MAX_RETRIES)
    }

    pub fn stream_idle_timeout(&self) -> Duration {
        Duration::from_millis(
            self.stream_idle_timeout_ms
                .unwrap_or(DEFAULT_STREAM_IDLE_TIMEOUT_MS),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_provider_is_deepseek() {
        let info = ProviderInfo::default_provider();

        assert_eq!(info.name, "DeepSeek");
        assert_eq!(info.env_key, None);
        assert_eq!(info.default_model, "deepseek-v4-flash");
        assert_eq!(info.wire_api, WireApi::Chat);
    }

    #[test]
    fn openai_uses_default_model_and_responses_wire_api() {
        let info = ProviderInfo::openai(None);

        assert_eq!(info.name, "OpenAI");
        assert_eq!(info.env_key, None);
        assert_eq!(info.default_model, "gpt-5.5");
        assert_eq!(info.wire_api, WireApi::Responses);
    }
}
