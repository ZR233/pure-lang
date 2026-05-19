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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub fn openai(base_url: Option<String>) -> Self {
        Self {
            name: "OpenAI".into(),
            base_url: base_url.or_else(|| Some("https://api.openai.com/v1".into())),
            env_key: Some("OPENAI_API_KEY".into()),
            wire_api: WireApi::Responses,
            ..Default::default()
        }
    }

    pub fn anthropic(base_url: Option<String>) -> Self {
        Self {
            name: "Anthropic".into(),
            base_url: base_url.or_else(|| Some("https://api.anthropic.com".into())),
            env_key: Some("ANTHROPIC_API_KEY".into()),
            wire_api: WireApi::Chat,
            http_headers: Some(HashMap::from([(
                "anthropic-version".into(),
                "2023-06-01".into(),
            )])),
            ..Default::default()
        }
    }

    pub fn ollama() -> Self {
        Self {
            name: "Ollama".into(),
            base_url: Some("http://localhost:11434/v1".into()),
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
