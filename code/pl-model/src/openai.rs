use std::collections::HashMap;
use std::time::Duration;

use futures::StreamExt;
use pl_core::{AgentEvent, AgentEventSender, PureError, Result};
use tracing::{debug, warn};

use crate::capabilities::ProviderCapabilities;
use crate::model_info::ModelInfo;
use crate::provider::ModelProvider;
use crate::provider_info::{ProviderInfo, WireApi};
use crate::request::{CompletionRequest, CompletionResponse, FinishReason, TokenUsage, ToolCall};
use crate::sse::{self, StreamEvent};
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
        let wire_dispatch = match info.wire_api {
            WireApi::Responses => WireDispatch::Responses,
            WireApi::Chat => WireDispatch::Chat,
        };
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| PureError::HttpError(e.to_string()))?;

        let bundled_models: Vec<ModelInfo> =
            serde_json::from_str(include_str!("../models/default.json")).unwrap_or_default();

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

impl ModelProvider for OpenAiCompatibleProvider {
    fn info(&self) -> &ProviderInfo {
        &self.info
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::all()
    }

    fn auth_token(&self) -> impl std::future::Future<Output = Result<Option<String>>> + Send {
        let bearer = self.info.bearer_token.clone();
        let auth_cmd = self.info.auth_command.clone();
        let env_key = self.info.env_key.clone();
        async move {
            if let Some(token) = bearer {
                return Ok(Some(token));
            }
            if let Some(cmd) = auth_cmd {
                let output = tokio::time::timeout(
                    Duration::from_millis(cmd.timeout_ms),
                    tokio::process::Command::new(&cmd.command)
                        .args(&cmd.args)
                        .output(),
                )
                .await
                .map_err(|_| PureError::LlmError("auth command timed out".into()))?
                .map_err(|e| PureError::LlmError(format!("auth command failed: {e}")))?;
                return Ok(Some(
                    String::from_utf8_lossy(&output.stdout).trim().to_string(),
                ));
            }
            if let Some(env_key) = env_key {
                return Ok(std::env::var(env_key).ok());
            }
            Ok(None)
        }
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
        async move {
            let bearer = info.bearer_token.clone();
            let auth_cmd = info.auth_command.clone();
            let env_key = info.env_key.clone();
            let token = get_auth_token(bearer, auth_cmd, env_key).await?;

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
        "gpt-4o"
    }
}

async fn get_auth_token(
    bearer: Option<String>,
    auth_cmd: Option<crate::provider_info::AuthCommand>,
    env_key: Option<String>,
) -> Result<Option<String>> {
    if let Some(token) = bearer {
        return Ok(Some(token));
    }
    if let Some(cmd) = auth_cmd {
        let output = tokio::time::timeout(
            Duration::from_millis(cmd.timeout_ms),
            tokio::process::Command::new(&cmd.command)
                .args(&cmd.args)
                .output(),
        )
        .await
        .map_err(|_| PureError::LlmError("auth command timed out".into()))?
        .map_err(|e| PureError::LlmError(format!("auth command failed: {e}")))?;
        return Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ));
    }
    if let Some(env_key) = env_key {
        return Ok(std::env::var(env_key).ok());
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
    let mut content_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut tool_call_accumulators: HashMap<String, ToolCallAccumulator> = HashMap::new();
    let mut final_usage: Option<TokenUsage> = None;

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

        match stream_event {
            StreamEvent::OutputTextDelta(text) => {
                content_parts.push(text.clone());
                let _ = event_tx.send(AgentEvent::TextDelta { content: text });
            }

            StreamEvent::ThinkingDelta { delta } => {
                let _ = event_tx.send(AgentEvent::ThinkingDelta { content: delta });
            }

            StreamEvent::ToolCallDelta {
                item_id,
                call_id,
                delta,
            } => {
                let key = call_id.as_ref().unwrap_or(&item_id).clone();
                let acc = tool_call_accumulators
                    .entry(key.clone())
                    .or_insert_with(|| ToolCallAccumulator {
                        id: item_id.clone(),
                        call_id: call_id.clone(),
                        name: String::new(),
                        arguments: String::new(),
                    });
                acc.arguments.push_str(&delta);
                let _ = event_tx.send(AgentEvent::ToolCallDelta {
                    id: acc.id.clone(),
                    name: String::new(),
                    arguments_delta: delta,
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

                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments: serde_json::from_str(&arguments)
                            .unwrap_or(serde_json::Value::String(arguments)),
                        call_id: Some(call_id),
                    });
                }
            }

            StreamEvent::Completed { usage, response_id } => {
                final_usage = usage;
                let _ = response_id;
            }

            StreamEvent::Failed { message, .. } => {
                return Err(PureError::LlmError(message));
            }

            StreamEvent::Created => {}
        }
    }

    // 合并累积的工具调用（如果有 delta 但没有 output_item.done）
    for (_, acc) in tool_call_accumulators {
        if !tool_calls.iter().any(|tc| tc.id == acc.id) {
            tool_calls.push(ToolCall {
                id: acc.id.clone(),
                name: acc.name,
                arguments: serde_json::from_str(&acc.arguments)
                    .unwrap_or(serde_json::Value::String(acc.arguments)),
                call_id: acc.call_id,
            });
        }
    }

    let content = if content_parts.is_empty() {
        None
    } else {
        Some(content_parts.join(""))
    };

    let finish_reason = if !tool_calls.is_empty() {
        FinishReason::ToolCalls
    } else {
        FinishReason::Stop
    };

    Ok(CompletionResponse {
        content,
        tool_calls,
        usage: final_usage.unwrap_or(TokenUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        }),
        finish_reason,
        model: String::new(),
    })
}

struct ToolCallAccumulator {
    id: String,
    call_id: Option<String>,
    name: String,
    arguments: String,
}
