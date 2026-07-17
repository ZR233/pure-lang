use std::collections::HashMap;
use std::time::Duration;

use async_openai::Client;
use async_openai::config::Config;
use async_openai::types::stream::StreamResponse;
use futures::StreamExt;
use pl_protocol::{
    ContentPart, ImageSource, Message, MessageContent, MessageRole, ModelContextItem, PureError,
    Result,
};
use serde::Deserialize;
use serde_json::{Map, Value};

use super::{OpenAiProvider, PureOpenAiConfig, get_auth_token, openai_error_to_pure};
use crate::protocol::openai::sse;
use crate::protocol::openai::{OpenAiProtocol, OpenAiRequestBody};
use crate::provider_info::ProviderWireProtocol;
use crate::request::{
    CompletionRequest, ModelCompactionRequest, ModelCompactionResponse, OpenAiCompactionMode,
    TokenUsage,
};

const REMOTE_COMPACTION_V2_FEATURE: &str = "remote_compaction_v2";
const MAX_REMOTE_V2_RETRIES: u32 = 2;

pub(super) async fn compact_context(
    provider: &OpenAiProvider,
    request: ModelCompactionRequest,
) -> Result<ModelCompactionResponse> {
    if provider.info.protocol != ProviderWireProtocol::Responses {
        return Err(PureError::ConfigError(
            "remote context compaction requires the Responses protocol".to_string(),
        ));
    }
    match request.mode {
        OpenAiCompactionMode::RemoteV2 => compact_v2(provider, request).await,
        OpenAiCompactionMode::RemoteLegacy => compact_legacy(provider, request).await,
        OpenAiCompactionMode::Local => Err(PureError::ConfigError(
            "local context compaction must be orchestrated by pl-core".to_string(),
        )),
    }
}

async fn compact_legacy(
    provider: &OpenAiProvider,
    request: ModelCompactionRequest,
) -> Result<ModelCompactionResponse> {
    let (mut body, model_headers) = build_compaction_body(provider, &request)?;
    body.remove("stream");
    let token = get_auth_token(provider.info.bearer_token.clone()).await?;
    let config = PureOpenAiConfig::new(
        provider.resolve_base_url(),
        token,
        provider.info.http_headers.as_ref(),
        &model_headers,
    )?;
    let url = format!("{}/responses/compact", provider.resolve_base_url());
    let response = provider
        .http_client
        .post(url)
        .headers(config.headers())
        .json(&body)
        .send()
        .await
        .map_err(|error| PureError::HttpError(error.to_string()))?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| PureError::HttpError(error.to_string()))?;
    if !status.is_success() {
        let text = String::from_utf8_lossy(&bytes);
        return Err(PureError::LlmError(format!(
            "OpenAI compact endpoint returned {status}: {text}"
        )));
    }
    let response: CompactHistoryResponse = serde_json::from_slice(&bytes).map_err(|error| {
        PureError::HttpError(format!("failed to decode OpenAI compact response: {error}"))
    })?;
    Ok(ModelCompactionResponse {
        input: parse_output_items(response.output)?,
        usage: None,
    })
}

async fn compact_v2(
    provider: &OpenAiProvider,
    request: ModelCompactionRequest,
) -> Result<ModelCompactionResponse> {
    let (mut body, mut model_headers) = build_compaction_body(provider, &request)?;
    let input = body
        .get_mut("input")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| PureError::ConfigError("OpenAI Responses input must be an array".into()))?;
    input.push(serde_json::json!({ "type": "compaction_trigger" }));
    body.insert("stream".to_string(), Value::Bool(true));
    append_beta_feature(
        &mut model_headers,
        provider.info.http_headers.as_ref(),
        REMOTE_COMPACTION_V2_FEATURE,
    );

    let mut retry = 0;
    loop {
        let result = compact_v2_attempt(provider, body.clone(), &model_headers).await;
        match result {
            Ok(response) => return Ok(response),
            Err(error) if retry < MAX_REMOTE_V2_RETRIES && is_retryable(&error) => {
                retry += 1;
                tokio::time::sleep(Duration::from_millis(100 * u64::from(retry))).await;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn compact_v2_attempt(
    provider: &OpenAiProvider,
    body: Map<String, Value>,
    model_headers: &HashMap<String, String>,
) -> Result<ModelCompactionResponse> {
    let token = get_auth_token(provider.info.bearer_token.clone()).await?;
    let config = PureOpenAiConfig::new(
        provider.resolve_base_url(),
        token,
        provider.info.http_headers.as_ref(),
        model_headers,
    )?;
    let client = Client::build(provider.http_client.clone(), config);
    let mut stream: StreamResponse<sse::SseStreamEvent> = client
        .responses()
        .create_stream_byot(body)
        .await
        .map_err(openai_error_to_pure)?;
    let mut compaction = None;
    let mut compaction_count = 0;
    let mut usage = None;
    let mut completed = false;
    while let Some(event) = stream.next().await {
        let event = event.map_err(openai_error_to_pure)?;
        match event.kind.as_str() {
            "response.output_item.done" => {
                if let Some(item) = event.item
                    && item.get("type").and_then(Value::as_str) == Some("compaction")
                {
                    compaction_count += 1;
                    if compaction.is_none() {
                        compaction = parse_output_item(item)?;
                    }
                }
            }
            "response.completed" => {
                completed = true;
                usage = event
                    .response
                    .as_ref()
                    .and_then(|response| response.get("usage"))
                    .map(token_usage)
                    .transpose()?;
                break;
            }
            _ => {}
        }
    }
    if !completed {
        return Err(PureError::HttpError(
            "OpenAI remote compaction v2 stream closed before response.completed".to_string(),
        ));
    }
    if compaction_count != 1 {
        return Err(PureError::ConfigError(format!(
            "OpenAI remote compaction v2 expected exactly one compaction item, got {compaction_count}"
        )));
    }
    let Some(compaction) = compaction else {
        return Err(PureError::ConfigError(
            "OpenAI remote compaction v2 returned an invalid compaction item".to_string(),
        ));
    };
    Ok(ModelCompactionResponse {
        input: vec![compaction],
        usage,
    })
}

fn build_compaction_body(
    provider: &OpenAiProvider,
    request: &ModelCompactionRequest,
) -> Result<(Map<String, Value>, HashMap<String, String>)> {
    let model_info = provider.model_info(&request.model);
    let effective_capabilities = model_info.capabilities.clone().with_provider_capabilities(
        provider.capabilities,
        provider.info.uses_native_custom_tools(),
    );
    let supports_custom_tools = provider.info.uses_native_custom_tools()
        && effective_capabilities.supports_custom_tools()
        && effective_capabilities.supports_freeform_tools();
    let mut completion = CompletionRequest::builder(request.model.clone())
        .instructions(request.instructions.clone())
        .input(request.input.clone())
        .tools(request.tools.clone())
        .parallel_tool_calls(request.parallel_tool_calls)
        .prompt_cache_key(request.prompt_cache_key.clone())
        .reasoning(request.reasoning.clone())
        .store(Some(false))
        .build()
        .provider_compatible(supports_custom_tools);
    completion.validate_against(&effective_capabilities)?;
    if let Some(api_model) = &model_info.request_profile.api_model {
        completion.model = api_model.clone();
    }
    let OpenAiRequestBody::Responses(mut body) =
        OpenAiProtocol::responses().build_request(&completion, &model_info)?
    else {
        return Err(PureError::ConfigError(
            "OpenAI remote compaction requires the Responses protocol".to_string(),
        ));
    };
    for key in ["tool_choice", "store", "previous_response_id"] {
        body.remove(key);
    }
    Ok((body, model_info.request_profile.headers.clone()))
}

fn append_beta_feature(
    model_headers: &mut HashMap<String, String>,
    provider_headers: Option<&HashMap<String, String>>,
    feature: &str,
) {
    const HEADER: &str = "x-codex-beta-features";
    let mut features = provider_headers
        .into_iter()
        .flat_map(HashMap::iter)
        .chain(model_headers.iter())
        .filter(|(key, _)| key.eq_ignore_ascii_case(HEADER))
        .flat_map(|(_, value)| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if !features.iter().any(|value| value == feature) {
        features.push(feature.to_string());
    }
    features.dedup();
    model_headers.retain(|key, _| !key.eq_ignore_ascii_case(HEADER));
    model_headers.insert(HEADER.to_string(), features.join(","));
}

#[derive(Debug, Deserialize)]
struct CompactHistoryResponse {
    output: Vec<Value>,
}

fn parse_output_items(items: Vec<Value>) -> Result<Vec<ModelContextItem>> {
    let mut parsed = Vec::new();
    for item in items {
        if let Some(item) = parse_output_item(item)? {
            parsed.push(item);
        }
    }
    Ok(parsed)
}

fn parse_output_item(item: Value) -> Result<Option<ModelContextItem>> {
    match item.get("type").and_then(Value::as_str) {
        Some("compaction" | "compaction_summary" | "context_compaction") => {
            let encrypted_content = item
                .get("encrypted_content")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    PureError::HttpError(
                        "OpenAI compaction item is missing encrypted_content".to_string(),
                    )
                })?;
            Ok(Some(ModelContextItem::Compaction {
                encrypted_content: encrypted_content.to_string(),
            }))
        }
        Some("message") => parse_message_item(&item).map(|message| message.map(Into::into)),
        _ => Ok(None),
    }
}

fn parse_message_item(item: &Value) -> Result<Option<Message>> {
    let role = match item.get("role").and_then(Value::as_str) {
        Some("developer" | "system") => MessageRole::System,
        Some("user") => MessageRole::User,
        Some("assistant") => MessageRole::Assistant,
        _ => return Ok(None),
    };
    let content = item
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_content_part)
        .collect::<Vec<_>>();
    let content = if content.len() == 1 {
        match content.into_iter().next() {
            Some(ContentPart::Text { text }) => MessageContent::Text(text),
            Some(part) => MessageContent::MultiPart(vec![part]),
            None => MessageContent::Text(String::new()),
        }
    } else {
        MessageContent::MultiPart(content)
    };
    Ok(Some(Message {
        role,
        content,
        reasoning_content: None,
        metadata: HashMap::new(),
    }))
}

fn parse_content_part(value: &Value) -> Option<ContentPart> {
    match value.get("type").and_then(Value::as_str) {
        Some("input_text" | "output_text") => {
            value
                .get("text")
                .and_then(Value::as_str)
                .map(|text| ContentPart::Text {
                    text: text.to_string(),
                })
        }
        Some("input_image") => value
            .get("image_url")
            .and_then(Value::as_str)
            .and_then(parse_data_url),
        _ => None,
    }
}

fn parse_data_url(url: &str) -> Option<ContentPart> {
    let value = url.strip_prefix("data:")?;
    let (media_type, data) = value.split_once(";base64,")?;
    Some(ContentPart::Image {
        source: ImageSource::InlineBase64 {
            data: data.to_string(),
        },
        media_type: media_type.to_string(),
        filename: None,
    })
}

fn token_usage(value: &Value) -> Result<TokenUsage> {
    let input = value
        .get("input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let output = value
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let cached = value
        .get("input_tokens_details")
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or_default();
    Ok(TokenUsage {
        prompt_tokens: input,
        completion_tokens: output,
        total_tokens: value
            .get("total_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(input + output),
        cached_prompt_tokens: cached,
        reasoning_tokens: 0,
    })
}

fn is_retryable(error: &PureError) -> bool {
    match error {
        PureError::HttpError(_) => true,
        PureError::LlmError(message) => {
            let message = message.to_ascii_lowercase();
            ["408", "409", "429", "500", "502", "503", "504", "timeout"]
                .iter()
                .any(|needle| message.contains(needle))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_compaction_and_message_output_items() {
        let output = vec![
            serde_json::json!({
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "hello" }]
            }),
            serde_json::json!({
                "type": "compaction",
                "encrypted_content": "encrypted"
            }),
        ];

        let parsed = parse_output_items(output).unwrap();

        assert_eq!(parsed.len(), 2);
        assert!(matches!(parsed[0], ModelContextItem::Message { .. }));
        assert_eq!(
            parsed[1],
            ModelContextItem::Compaction {
                encrypted_content: "encrypted".to_string()
            }
        );
    }
}
