//! Studio wording for model-owned context compaction.
use pl_model::{
    completion::OpenAiCompactionMode,
    runtime::{ThreadCompactionOptions, ThreadCompactionStrategy},
};

/// Selects the product summary policy without changing provider capabilities.
pub(crate) fn policy(mode: OpenAiCompactionMode) -> ThreadCompactionOptions {
    ThreadCompactionOptions {
        strategy: match mode {
            OpenAiCompactionMode::Local => ThreadCompactionStrategy::TextSummary,
            OpenAiCompactionMode::RemoteV2 => ThreadCompactionStrategy::PreferNative,
        },
        instructions: include_str!("prompts/compact.md").into(),
        requirement: "请根据以上完整上下文生成压缩摘要。".into(),
        summary_prefix: "以下是此前对话的压缩摘要。".into(),
        max_output_tokens: None,
    }
}

use pl_core::thread::{
    context_preparation::{
        ContextPreparation, ContextPreparationHook, ContextPreparationRequest, ContextPreparer,
    },
    extensions::ExtensionMutation,
};
use pl_model::{
    config::ResolvedModelRoute,
    runtime::{ModelRuntime, ThreadModel},
};

/// Binds the policy to the same frozen route as the main model session.
pub(crate) fn preparer(
    route: &ResolvedModelRoute,
    mode: OpenAiCompactionMode,
) -> Result<Option<ContextPreparer>, pl_model::PureError> {
    let Some(limit) = route.model.resolved_auto_compact_limit() else {
        return Ok(None);
    };
    Ok(Some(ContextPreparer::new(StudioCompaction {
        model: ThreadModel::new(ModelRuntime::from_route(route)?, route.reasoning_config()),
        limit,
        reasoning_effort: route
            .effort
            .as_ref()
            .map(|effort| effort.as_str().to_owned()),
        context_window: route.model.resolved_context_window(),
        options: policy(mode),
    })))
}

#[derive(Debug)]
struct StudioCompaction {
    model: ThreadModel,
    limit: u64,
    reasoning_effort: Option<String>,
    context_window: Option<u64>,
    options: ThreadCompactionOptions,
}
impl ContextPreparationHook for StudioCompaction {
    async fn before_step(&self, request: ContextPreparationRequest) -> ContextPreparation {
        let estimate = match self.model.estimate_input(&request.model).await {
            Ok(estimate) => estimate
                .map(|estimate| estimate.tokens)
                .or_else(|| request.previous_usage.and_then(|usage| usage.input_tokens)),
            Err(error) => {
                return ContextPreparation::Failed {
                    error,
                    mutations: vec![],
                };
            }
        };
        if request
            .model
            .context
            .records
            .iter()
            .all(|record| record.source == pl_core::context::ContextSource::Instruction)
            || !estimate.is_some_and(|tokens| tokens >= self.limit)
        {
            return ContextPreparation::Unchanged;
        }
        let turn_id = request.model.turn_id.clone();
        let id = format!(
            "studio.compaction:{}:{}",
            request.model.attempt_id, request.extension_sequence
        );
        let mut model_request = request.model;
        model_request.attempt_id = id.clone();
        match self
            .model
            .compact(model_request, self.options.clone())
            .await
        {
            Ok(result) => {
                let receipt = CompactionReceipt {
                    inference_id: id.clone(),
                    binding: result.binding,
                    reasoning_effort: self.reasoning_effort.clone(),
                    context_window: self.context_window,
                    turn_id: turn_id.clone(),
                    accounting: result.accounting,
                    implementation: Some(
                        match result.implementation {
                            ThreadCompactionStrategy::TextSummary => "textSummary",
                            ThreadCompactionStrategy::PreferNative => "native",
                        }
                        .into(),
                    ),
                    error: None,
                };
                match receipt_mutation(id, receipt) {
                    Ok(mutation) => ContextPreparation::Replaced {
                        replacement: result.replacement,
                        mutations: vec![mutation],
                    },
                    Err(error) => ContextPreparation::Failed {
                        error,
                        mutations: vec![],
                    },
                }
            }
            Err(error) => {
                let receipt = pl_model::runtime::model_failure_receipt(&error)
                    .ok()
                    .flatten()
                    .map(|receipt| CompactionReceipt {
                        inference_id: id.clone(),
                        binding: receipt.binding,
                        reasoning_effort: self.reasoning_effort.clone(),
                        context_window: self.context_window,
                        turn_id: turn_id.clone(),
                        accounting: receipt.accounting,
                        implementation: None,
                        error: Some(receipt.message),
                    });
                let mutations = receipt
                    .and_then(|receipt| receipt_mutation(id, receipt).ok())
                    .into_iter()
                    .collect();
                ContextPreparation::Failed { error, mutations }
            }
        }
    }
}

/// Auxiliary model accounting is retained separately from normal turn-response receipts.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompactionReceipt {
    pub inference_id: String,
    pub binding: pl_model::runtime::ModelCallBinding,
    pub reasoning_effort: Option<String>,
    pub context_window: Option<u64>,
    pub turn_id: String,
    pub accounting: pl_model::completion::InferenceAccounting,
    pub implementation: Option<String>,
    pub error: Option<String>,
}
fn receipt_mutation(
    id: String,
    receipt: CompactionReceipt,
) -> Result<ExtensionMutation, pl_core::model::ModelError> {
    let payload = serde_json::to_string(&receipt).map_err(receipt_error)?;
    let payload = pl_core::context::OpaquePayload::new("pl.studio.compaction", 1, payload)
        .map_err(receipt_error)?;
    Ok(ExtensionMutation::Put {
        id,
        expected_revision: None,
        payload,
    })
}
fn receipt_error(
    source: impl std::error::Error + Send + Sync + 'static,
) -> pl_core::model::ModelError {
    pl_core::model::ModelError {
        kind: pl_core::model::ModelFailureKind::InvalidResponse,
        details: None,
        usage: Default::default(),
        source: Some(Box::new(source)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{
        context::{ContextContent, ContextRecord, ContextSource},
        model::Model,
        thread::{ContextReplacementReason, ReplaceContext, StepInput, ThreadHandle},
    };
    use pretty_assertions::assert_eq;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn threshold_policy_summarizes_before_main_dispatch_and_keeps_original_instruction_and_usage()
     {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for (index, text) in ["summary alpha", "final alpha"].into_iter().enumerate() {
                let (mut socket, _) = listener.accept().await.unwrap();
                let body = request_body(&mut socket).await;
                if index == 0 {
                    assert!(body.get("tools").is_none());
                    assert!(body.get("tool_choice").is_none());
                } else {
                    assert!(body.to_string().contains("summary alpha"));
                    assert!(body.to_string().contains("stable instruction"));
                    assert!(body.to_string().contains("followup"));
                }
                let response = format!(
                    "data: {}\n\ndata: [DONE]\n\n",
                    serde_json::json!({"choices":[{"delta":{"content":text},"finish_reason":"stop"}],"usage":{"prompt_tokens":12,"completion_tokens":3,"total_tokens":15}})
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{response}",
                    response.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
                socket.shutdown().await.unwrap();
            }
        });
        let mut info = pl_model::model::ModelInfo::compatible("compaction-fixture");
        info.auto_compact_token_limit = Some(1);
        let route = ResolvedModelRoute {
            pricing_mode: pl_protocol::PricingMode::Disabled,
            role: pl_protocol::AgentRoleId::new("test").unwrap(),
            provider_id: pl_model::config::ProviderId::new("fixture").unwrap(),
            endpoint: pl_model::provider::ProviderEndpoint::deepseek(Some(format!(
                "http://{address}"
            ))),
            model: info,
            effort: None,
        };
        let model = ThreadModel::new(ModelRuntime::from_route(&route).unwrap(), None);
        let thread =
            ThreadHandle::start("compaction".into(), model.open_session().await.unwrap()).unwrap();
        let records = [
            (
                "instruction",
                ContextSource::Instruction,
                "stable instruction",
            ),
            ("old", ContextSource::User, "project alpha: old context"),
        ]
        .into_iter()
        .map(|(id, source, text)| ContextRecord {
            id: id.into(),
            turn_id: None,
            source,
            content: vec![ContextContent::Text { text: text.into() }],
            tool_calls: vec![],
        })
        .collect();
        thread
            .replace_context(ReplaceContext {
                expected_revision: 0,
                reason: ContextReplacementReason::Rebuild,
                records,
            })
            .await
            .unwrap();
        thread
            .set_context_preparation(preparer(&route, OpenAiCompactionMode::Local).unwrap())
            .await
            .unwrap();
        thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: vec![ContextContent::Text {
                    text: "followup".into(),
                }],
                cancellation: Default::default(),
            })
            .await
            .unwrap();
        let snapshot = thread.snapshot();
        assert_eq!(
            snapshot.attempts.len(),
            1,
            "summary accounting is not a normal turn inference"
        );
        assert_eq!(snapshot.context_replacements.len(), 2);
        let receipt: CompactionReceipt = serde_json::from_str(
            snapshot
                .extensions
                .values()
                .next()
                .unwrap()
                .payload
                .content(),
        )
        .unwrap();
        assert_eq!(receipt.accounting.usage.input_tokens, Some(12));
        assert_eq!(receipt.implementation.as_deref(), Some("textSummary"));
        thread.close().await.unwrap();
        server.await.unwrap();
    }
    async fn request_body(socket: &mut tokio::net::TcpStream) -> serde_json::Value {
        let mut bytes = Vec::new();
        let mut chunk = [0; 4096];
        loop {
            let read = socket.read(&mut chunk).await.unwrap();
            assert_ne!(read, 0);
            bytes.extend_from_slice(&chunk[..read]);
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                let length = String::from_utf8_lossy(&bytes[..end])
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                if bytes.len() >= end + 4 + length {
                    return serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap();
                }
            }
        }
    }
}
