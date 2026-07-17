use std::collections::HashMap;
use std::time::Duration;

use async_openai::Client;
use async_openai::config::Config;
use async_openai::error::OpenAIError;
use async_openai::types::stream::StreamResponse;
use pl_protocol::{PureError, Result};
use pl_trace::AgentEventSender;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use secrecy::SecretString;

use crate::capabilities::{ModelCapabilities, ProviderCapabilities};
use crate::model_info::ModelInfo;
use crate::protocol::openai::sse;
use crate::protocol::openai::{OpenAiProtocol, OpenAiRequestBody};
use crate::provider_info::{ProviderConnectionMode, ProviderInfo, ProviderWireProtocol};
use crate::request::{CompletionRequest, CompletionResponse};
use crate::request::{ModelCompactionRequest, ModelCompactionResponse};
use crate::stream::{
    CompletionEventStream, collect_completion_event_stream, decode_openai_event_stream,
    decode_provider_stream,
};

#[derive(Debug)]
pub struct OpenAiProvider {
    info: ProviderInfo,
    http_client: reqwest::Client,
    protocol: OpenAiProtocol,
    capabilities: ProviderCapabilities,
    models: Vec<ModelInfo>,
}

mod compaction;
mod responses_websocket;

impl OpenAiProvider {
    pub(crate) fn new(info: ProviderInfo, models: Vec<ModelInfo>) -> Result<Self> {
        if info.connection_mode == ProviderConnectionMode::WebSocket
            && info.protocol != ProviderWireProtocol::Responses
        {
            return Err(PureError::ConfigError(
                "chat_completions protocol does not support web_socket connection mode".to_string(),
            ));
        }
        let (protocol, capabilities) = provider_profile(info.protocol);
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| PureError::HttpError(e.to_string()))?;

        Ok(Self {
            info,
            http_client,
            protocol,
            capabilities,
            models,
        })
    }

    fn resolve_base_url(&self) -> String {
        self.info.base_url.clone().trim_end_matches('/').to_string()
    }
    pub(crate) fn info(&self) -> &ProviderInfo {
        &self.info
    }

    pub(crate) fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities
    }

    pub(crate) fn auth_token(
        &self,
    ) -> impl std::future::Future<Output = Result<Option<String>>> + Send {
        let bearer = self.info.bearer_token.clone();
        get_auth_token(bearer)
    }

    pub(crate) fn stream_complete(
        &self,
        request: CompletionRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<CompletionResponse>> + Send {
        let trace = request.trace.clone();
        async move {
            let event_stream = self.stream_events(request).await?;
            collect_completion_event_stream(event_stream, &event_tx, trace).await
        }
    }

    pub(crate) fn stream_events(
        &self,
        request: CompletionRequest,
    ) -> impl std::future::Future<Output = Result<CompletionEventStream>> + Send {
        let http_client = self.http_client.clone();
        let api_base = self.resolve_base_url();
        let protocol = self.protocol;
        let capabilities = self.capabilities;
        let info = self.info.clone();
        let model_info = self.model_info(&request.model);
        let connection_key = self.connection_fingerprint(&request.model);
        async move {
            let bearer = info.bearer_token.clone();
            let token = get_auth_token(bearer).await?;

            let effective_capabilities = model_info
                .capabilities
                .clone()
                .with_provider_capabilities(capabilities, info.uses_native_custom_tools());
            let supports_custom_tools = info.uses_native_custom_tools()
                && effective_capabilities.supports_custom_tools()
                && effective_capabilities.supports_freeform_tools();
            let mut request = request.provider_compatible(supports_custom_tools);
            request.validate_against(&effective_capabilities)?;
            if let Some(api_model) = &model_info.request_profile.api_model {
                request.model = api_model.clone();
            }
            let body = protocol.build_request(&request, &model_info)?;
            if info.connection_mode == ProviderConnectionMode::WebSocket {
                let OpenAiRequestBody::Responses(body) = body else {
                    return Err(PureError::ConfigError(
                        "web_socket connection mode requires the Responses API".to_string(),
                    ));
                };
                let raw_stream = responses_websocket::stream_responses(
                    api_base,
                    token,
                    info.http_headers.as_ref(),
                    &model_info.request_profile.headers,
                    connection_key,
                    request.transport_session.clone(),
                    body,
                )
                .await?;
                return Ok(decode_openai_event_stream(raw_stream, protocol));
            }
            let config = PureOpenAiConfig::new(
                api_base,
                token,
                info.http_headers.as_ref(),
                &model_info.request_profile.headers,
            )?;
            let client = Client::build(http_client, config);
            let stream: StreamResponse<sse::SseStreamEvent> = match body {
                OpenAiRequestBody::Responses(body) => client
                    .responses()
                    .create_stream_byot(body)
                    .await
                    .map_err(openai_error_to_pure)?,
                OpenAiRequestBody::Chat(body) => client
                    .chat()
                    .create_stream_byot(body)
                    .await
                    .map_err(openai_error_to_pure)?,
            };

            Ok(decode_provider_stream(stream, protocol))
        }
    }

    pub(crate) fn compact_context(
        &self,
        request: ModelCompactionRequest,
    ) -> impl std::future::Future<Output = Result<ModelCompactionResponse>> + Send {
        compaction::compact_context(self, request)
    }

    pub(crate) fn model_info(&self, model: &str) -> ModelInfo {
        self.models
            .iter()
            .find(|m| m.slug == model)
            .cloned()
            .unwrap_or_else(|| ModelInfo::fallback(model))
    }

    pub(crate) fn list_models(&self) -> Vec<ModelInfo> {
        self.models.clone()
    }

    pub(crate) fn effective_model_capabilities(&self, model: &str) -> ModelCapabilities {
        self.model_info(model)
            .capabilities
            .with_provider_capabilities(self.capabilities, self.info.uses_native_custom_tools())
    }

    pub(crate) fn default_model(&self) -> &str {
        self.info.default_model.as_str()
    }

    pub(crate) fn connection_fingerprint(&self, model: &str) -> u64 {
        use std::hash::{DefaultHasher, Hash, Hasher};

        let model_info = self.model_info(model);
        let mut hasher = DefaultHasher::new();
        self.info.connection_fingerprint().hash(&mut hasher);
        model.hash(&mut hasher);
        let mut headers = model_info
            .request_profile
            .headers
            .iter()
            .collect::<Vec<_>>();
        headers.sort_by(|left, right| left.0.cmp(right.0));
        for (name, value) in headers {
            name.hash(&mut hasher);
            value.hash(&mut hasher);
        }
        let fingerprint = hasher.finish();
        if fingerprint == 0 { 1 } else { fingerprint }
    }
}

/// 按 wire protocol 选择 endpoint；供应商能力由模型目录与数据化 wire policy 收敛。
fn provider_profile(protocol: ProviderWireProtocol) -> (OpenAiProtocol, ProviderCapabilities) {
    match protocol {
        ProviderWireProtocol::Responses => {
            (OpenAiProtocol::responses(), ProviderCapabilities::all())
        }
        ProviderWireProtocol::ChatCompletions => {
            (OpenAiProtocol::chat(), ProviderCapabilities::all())
        }
    }
}

async fn get_auth_token(bearer: Option<String>) -> Result<Option<String>> {
    if let Some(token) = bearer {
        return Ok(Some(token));
    }
    Ok(None)
}

#[derive(Debug, Clone)]
struct PureOpenAiConfig {
    api_base: String,
    api_key: SecretString,
    bearer_token: Option<String>,
    custom_headers: HeaderMap,
}

impl PureOpenAiConfig {
    fn new(
        api_base: String,
        bearer_token: Option<String>,
        http_headers: Option<&HashMap<String, String>>,
        model_headers: &HashMap<String, String>,
    ) -> Result<Self> {
        let mut custom_headers = HeaderMap::new();
        if let Some(headers) = http_headers {
            for (key, value) in headers {
                insert_header(&mut custom_headers, key, value)?;
            }
        }
        for (key, value) in model_headers {
            insert_header(&mut custom_headers, key, value)?;
        }

        Ok(Self {
            api_base,
            api_key: bearer_token.clone().unwrap_or_default().into(),
            bearer_token,
            custom_headers,
        })
    }
}

fn insert_header(headers: &mut HeaderMap, key: &str, value: &str) -> Result<()> {
    let name = HeaderName::from_bytes(key.as_bytes())
        .map_err(|error| PureError::HttpError(error.to_string()))?;
    let value =
        HeaderValue::from_str(value).map_err(|error| PureError::HttpError(error.to_string()))?;
    headers.insert(name, value);
    Ok(())
}

impl Config for PureOpenAiConfig {
    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(token) = &self.bearer_token
            && let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}"))
        {
            headers.insert(AUTHORIZATION, value);
        }

        for (key, value) in &self.custom_headers {
            headers.insert(key, value.clone());
        }

        headers
    }

    fn url(&self, path: &str) -> String {
        let base = &self.api_base;
        format!("{base}{path}")
    }

    fn query(&self) -> Vec<(&str, &str)> {
        Vec::new()
    }

    fn api_base(&self) -> &str {
        &self.api_base
    }

    fn api_key(&self) -> &SecretString {
        &self.api_key
    }
}

fn openai_error_to_pure(error: OpenAIError) -> PureError {
    match error {
        OpenAIError::ApiError(api_error) => {
            PureError::LlmError(redact_secret_like_values(&format!("API error {api_error}")))
        }
        OpenAIError::Reqwest(error) => {
            PureError::HttpError(redact_secret_like_values(&error.to_string()))
        }
        OpenAIError::JSONDeserialize(error, content) => {
            PureError::HttpError(redact_secret_like_values(&format!("{error}: {content}")))
        }
        OpenAIError::StreamError(error) => {
            PureError::HttpError(redact_secret_like_values(&error.to_string()))
        }
        OpenAIError::InvalidArgument(message) => PureError::ConfigError(message),
        OpenAIError::FileSaveError(message) | OpenAIError::FileReadError(message) => {
            PureError::Io(std::io::Error::other(message))
        }
    }
}

fn redact_secret_like_values(input: &str) -> String {
    input
        .split_whitespace()
        .map(redact_secret_like_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_secret_like_token(token: &str) -> String {
    let trimmed = token.trim_matches(|ch: char| {
        matches!(
            ch,
            '.' | ',' | ';' | ':' | ')' | '(' | '"' | '\'' | '[' | ']' | '{' | '}'
        )
    });
    if !looks_like_secret_token(trimmed) {
        return token.to_string();
    }
    token.replacen(trimmed, "[REDACTED_API_KEY]", 1)
}

fn looks_like_secret_token(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    (lower.starts_with("sk-") || lower.starts_with("sk_"))
        && token.len() >= 12
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '*' | '.'))
}

#[cfg(test)]
mod tests;
