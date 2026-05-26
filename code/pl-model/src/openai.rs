use std::collections::HashMap;
use std::time::Duration;

use futures::StreamExt;
use pl_protocol::{AgentEvent, AgentEventSender, PureError, Result};
use tracing::{debug, warn};

use crate::capabilities::ProviderCapabilities;
use crate::model_info::ModelInfo;
use crate::provider::ModelProvider;
use crate::provider_info::{ProviderInfo, WireApi};
use crate::request::{CompletionRequest, CompletionResponse, FinishReason, TokenUsage, ToolCall};
use crate::sse::{self, StreamEvent, ToolCallDeltaPayload};
use crate::wire_api::WireDispatch;

#[derive(Debug)]
pub struct OpenAiCompatibleProvider {
    info: ProviderInfo,
    http_client: reqwest::Client,
    wire_dispatch: WireDispatch,
    bundled_models: Vec<ModelInfo>,
}

impl OpenAiCompatibleProvider {
    pub fn new(info: ProviderInfo) -> Result<Self> {
        Self::with_models(info, Vec::new())
    }

    pub fn with_models(info: ProviderInfo, configured_models: Vec<ModelInfo>) -> Result<Self> {
        let wire_dispatch = match info.wire_api {
            WireApi::Responses => WireDispatch::Responses,
            WireApi::Chat => WireDispatch::Chat,
        };
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| PureError::HttpError(e.to_string()))?;

        let bundled_models =
            merge_models(crate::default_models::default_models(), configured_models);

        Ok(Self {
            info,
            http_client,
            wire_dispatch,
            bundled_models,
        })
    }

    fn resolve_base_url(&self) -> String {
        self.info
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".into())
    }

    fn resolve_endpoint(&self) -> String {
        let base = self.resolve_base_url();
        match self.info.wire_api {
            WireApi::Responses => format!("{base}/responses"),
            WireApi::Chat => format!("{base}/chat/completions"),
        }
    }
}

fn merge_models(
    mut bundled_models: Vec<ModelInfo>,
    configured_models: Vec<ModelInfo>,
) -> Vec<ModelInfo> {
    for model in configured_models {
        match bundled_models
            .iter_mut()
            .find(|existing| existing.slug == model.slug)
        {
            Some(existing) => *existing = model,
            None => bundled_models.push(model),
        }
    }
    bundled_models
}

impl ModelProvider for OpenAiCompatibleProvider {
    fn info(&self) -> &ProviderInfo {
        &self.info
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::all()
    }

    fn auth_token(&self) -> impl std::future::Future<Output = Result<Option<String>>> + Send {
        let bearer = self.info.bearer_token.clone();
        get_auth_token(bearer)
    }

    fn stream_complete(
        &self,
        request: CompletionRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<CompletionResponse>> + Send {
        let http_client = self.http_client.clone();
        let endpoint = self.resolve_endpoint();
        let wire_dispatch = self.wire_dispatch;
        let info = self.info.clone();
        let model_info = self.model_info(&request.model);
        async move {
            let bearer = info.bearer_token.clone();
            let token = get_auth_token(bearer).await?;

            let supports_custom_tools = info.supports_custom_tools_for_model(&model_info)
                && info.supports_freeform_tools_for_model(&model_info);
            let request = request.provider_compatible(supports_custom_tools);
            let body = wire_dispatch.build_request_body(&request);

            let mut req_builder = http_client
                .post(&endpoint)
                .header("Content-Type", "application/json");

            if let Some(ref token) = token {
                req_builder = req_builder.bearer_auth(token);
            }

            if let Some(headers) = &info.http_headers {
                for (key, value) in headers {
                    req_builder = req_builder.header(key.as_str(), value.as_str());
                }
            }

            let response = req_builder
                .json(&body)
                .send()
                .await
                .map_err(|e| PureError::HttpError(e.to_string()))?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(PureError::LlmError(format!("API error {status}: {body}")));
            }

            process_sse_stream(response, &event_tx, &wire_dispatch).await
        }
    }

    fn model_info(&self, model: &str) -> ModelInfo {
        self.bundled_models
            .iter()
            .find(|m| m.slug == model)
            .cloned()
            .unwrap_or_else(|| ModelInfo::fallback(model))
    }

    fn default_model(&self) -> &str {
        self.info.default_model.as_str()
    }
}

async fn get_auth_token(bearer: Option<String>) -> Result<Option<String>> {
    if let Some(token) = bearer {
        return Ok(Some(token));
    }
    Ok(None)
}

async fn process_sse_stream(
    response: reqwest::Response,
    event_tx: &AgentEventSender,
    wire_dispatch: &WireDispatch,
) -> Result<CompletionResponse> {
    use eventsource_stream::Eventsource;

    let stream = response.bytes_stream().eventsource();
    let mut accumulator = StreamCompletionAccumulator::default();

    let mut stream = std::pin::pin!(stream);

    while let Some(event) = stream.next().await {
        let event = match event {
            Ok(e) => e,
            Err(e) => {
                warn!("SSE parse error: {e}");
                break;
            }
        };

        let sse_event: sse::SseStreamEvent = match serde_json::from_str(&event.data) {
            Ok(e) => e,
            Err(e) => {
                debug!("Skipping unparseable SSE event: {e}");
                continue;
            }
        };

        let stream_event = match wire_dispatch.parse_stream_event(&sse_event)? {
            Some(e) => e,
            None => continue,
        };

        accumulator.apply(stream_event, event_tx)?;
    }

    Ok(accumulator.finish())
}

#[derive(Default)]
struct StreamCompletionAccumulator {
    content_parts: Vec<String>,
    reasoning_parts: Vec<String>,
    tool_calls: Vec<ToolCall>,
    tool_call_accumulators: HashMap<String, ToolCallAccumulator>,
    final_usage: Option<TokenUsage>,
}

impl StreamCompletionAccumulator {
    fn apply(&mut self, stream_event: StreamEvent, event_tx: &AgentEventSender) -> Result<()> {
        match stream_event {
            StreamEvent::OutputTextDelta(text) => {
                self.content_parts.push(text.clone());
                let _ = event_tx.send(AgentEvent::TextDelta { content: text });
            }

            StreamEvent::ThinkingDelta { delta } => {
                self.reasoning_parts.push(delta.clone());
                let _ = event_tx.send(AgentEvent::ThinkingDelta { content: delta });
            }

            StreamEvent::ToolCallDelta {
                item_id,
                call_id,
                name,
                payload_delta,
            } => {
                let key = call_id.as_ref().unwrap_or(&item_id).clone();
                let acc =
                    self.tool_call_accumulators
                        .entry(key)
                        .or_insert_with(|| ToolCallAccumulator {
                            id: item_id.clone(),
                            call_id: call_id.clone(),
                            name: String::new(),
                            payload: ToolCallPayloadAccumulator::FunctionArguments(String::new()),
                        });
                if let Some(name) = name
                    && !name.is_empty()
                {
                    acc.name = name;
                }
                let delta_text = payload_delta.text().to_string();
                acc.push_delta(payload_delta);
                let _ = event_tx.send(AgentEvent::ToolCallDelta {
                    id: acc.id.clone(),
                    name: acc.name.clone(),
                    arguments_delta: delta_text,
                });
            }

            StreamEvent::OutputItemDone(value) => {
                if let Some(func_call) = value.get("type").and_then(|t| t.as_str())
                    && func_call == "function_call"
                {
                    let name = value
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let call_id = value
                        .get("call_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let id = value
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let arguments = value
                        .get("arguments")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let _ = event_tx.send(AgentEvent::ToolCallComplete {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: arguments.clone(),
                    });

                    self.tool_calls.push(ToolCall::function(
                        id,
                        name,
                        serde_json::from_str(&arguments)
                            .unwrap_or(serde_json::Value::String(arguments)),
                        Some(call_id),
                    ));
                } else if let Some(custom_call) = value.get("type").and_then(|t| t.as_str())
                    && custom_call == "custom_tool_call"
                {
                    let name = value
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let call_id = value
                        .get("call_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let id = value
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or(call_id.as_str())
                        .to_string();
                    let input = value
                        .get("input")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let _ = event_tx.send(AgentEvent::ToolCallComplete {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: input.clone(),
                    });

                    self.tool_calls
                        .push(ToolCall::custom(id, name, input, Some(call_id)));
                }
            }

            StreamEvent::Completed { usage, response_id } => {
                self.final_usage = usage;
                let _ = response_id;
            }

            StreamEvent::Failed { message, .. } => {
                return Err(PureError::LlmError(message));
            }

            StreamEvent::Created => {}
        }

        Ok(())
    }

    fn finish(mut self) -> CompletionResponse {
        // 合并累积的工具调用（如果有 delta 但没有 output_item.done）
        for (_, acc) in self.tool_call_accumulators {
            if !self.tool_calls.iter().any(|tc| tc.id == acc.id) {
                self.tool_calls.push(acc.into_tool_call());
            }
        }

        let content = if self.content_parts.is_empty() {
            None
        } else {
            Some(self.content_parts.join(""))
        };

        let reasoning_content = if self.reasoning_parts.is_empty() {
            None
        } else {
            Some(self.reasoning_parts.join(""))
        };

        let finish_reason = if !self.tool_calls.is_empty() {
            FinishReason::ToolCalls
        } else {
            FinishReason::Stop
        };

        CompletionResponse {
            content,
            reasoning_content,
            tool_calls: self.tool_calls,
            usage: self.final_usage.unwrap_or_default(),
            finish_reason,
            model: String::new(),
        }
    }
}

struct ToolCallAccumulator {
    id: String,
    call_id: Option<String>,
    name: String,
    payload: ToolCallPayloadAccumulator,
}

impl ToolCallAccumulator {
    fn push_delta(&mut self, payload_delta: ToolCallDeltaPayload) {
        match (&mut self.payload, payload_delta) {
            (
                ToolCallPayloadAccumulator::FunctionArguments(arguments),
                ToolCallDeltaPayload::FunctionArguments(delta),
            ) => arguments.push_str(&delta),
            (
                ToolCallPayloadAccumulator::CustomInput(input),
                ToolCallDeltaPayload::CustomInput(delta),
            ) => input.push_str(&delta),
            (_, ToolCallDeltaPayload::FunctionArguments(delta)) => {
                self.payload = ToolCallPayloadAccumulator::FunctionArguments(delta);
            }
            (_, ToolCallDeltaPayload::CustomInput(delta)) => {
                self.payload = ToolCallPayloadAccumulator::CustomInput(delta);
            }
        }
    }

    fn into_tool_call(self) -> ToolCall {
        match self.payload {
            ToolCallPayloadAccumulator::FunctionArguments(arguments) => ToolCall::function(
                self.id,
                self.name,
                serde_json::from_str(&arguments).unwrap_or(serde_json::Value::String(arguments)),
                self.call_id,
            ),
            ToolCallPayloadAccumulator::CustomInput(input) => {
                ToolCall::custom(self.id, self.name, input, self.call_id)
            }
        }
    }
}

enum ToolCallPayloadAccumulator {
    FunctionArguments(String),
    CustomInput(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_accumulator_returns_content_and_reasoning_content() {
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::default();

        accumulator
            .apply(
                StreamEvent::ThinkingDelta {
                    delta: "先比较整数位。".to_string(),
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                StreamEvent::OutputTextDelta("9.11 更大。".to_string()),
                &event_tx,
            )
            .unwrap();

        let response = accumulator.finish();

        assert_eq!(response.content.as_deref(), Some("9.11 更大。"));
        assert_eq!(
            response.reasoning_content.as_deref(),
            Some("先比较整数位。")
        );
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::ThinkingDelta { .. }
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TextDelta { .. }
        ));
    }

    #[test]
    fn deepseek_provider_uses_provider_default_model() {
        let provider = OpenAiCompatibleProvider::new(ProviderInfo::deepseek(None)).unwrap();

        assert_eq!(provider.default_model(), "deepseek-v4-flash");
    }

    #[test]
    fn openai_provider_uses_provider_default_model() {
        let provider = OpenAiCompatibleProvider::new(ProviderInfo::openai(None)).unwrap();

        assert_eq!(provider.default_model(), "gpt-5.5");
    }

    #[test]
    fn configured_models_override_bundled_models() {
        let mut model = ModelInfo::fallback("deepseek-v4-flash");
        model.display_name = "Custom DeepSeek".to_string();
        let provider =
            OpenAiCompatibleProvider::with_models(ProviderInfo::deepseek(None), vec![model])
                .unwrap();

        assert_eq!(
            provider.model_info("deepseek-v4-flash").display_name,
            "Custom DeepSeek"
        );
    }
}
