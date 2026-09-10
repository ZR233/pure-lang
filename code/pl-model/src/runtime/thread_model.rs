//! Provider-backed implementation of the protocol-independent Thread model ports.
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use pl_core::{
    context::OpaquePayload,
    model::{
        DynModelSession, Model, ModelError, ModelFailureKind, ModelRequest,
        ModelSession as CoreModelSession, ModelUsage, PreparedModelCall,
    },
};

use super::{ModelInvocationContext, ModelRuntime, ModelSession};
use crate::completion::{ReasoningConfig, ToolSpec};

mod cache;
mod compaction;
pub use compaction::{ThreadCompaction, ThreadCompactionOptions, ThreadCompactionStrategy};
mod codec;
mod media;
pub(crate) mod progress;
mod receipt;
pub use media::attachment_content;
pub use receipt::{
    ModelCallBinding, ModelFailureReceipt, ModelRequestReceipt, ModelResponseReceipt,
    model_failure_receipt, model_request_receipt, model_response_receipt,
};

/// Model factory sharing clients while creating a private physical session for each Thread.
#[derive(Debug, Clone)]
pub struct ThreadModel {
    runtime: ModelRuntime,
    reasoning: Option<ReasoningConfig>,
    purpose: String,
    hosted_tools: Arc<[super::HostedTool]>,
}

impl ThreadModel {
    /// Freezes model parameters before the Thread session is opened.
    pub fn new(runtime: ModelRuntime, reasoning: Option<ReasoningConfig>) -> Self {
        Self {
            runtime,
            reasoning,
            purpose: "turn".into(),
            hosted_tools: Vec::new().into(),
        }
    }
    /// Configures provider-executed capabilities; these never create local tool executors.
    pub fn with_hosted_tools(mut self, tools: Vec<super::HostedTool>) -> Self {
        self.hosted_tools = tools.into();
        self
    }

    /// Sets a diagnostic purpose for this independent session factory; it grants no execution permissions.
    pub fn with_purpose(mut self, purpose: impl Into<String>) -> Self {
        self.purpose = purpose.into();
        self
    }
}

impl Model for ThreadModel {
    fn open_session(
        &self,
    ) -> impl std::future::Future<Output = Result<DynModelSession, ModelError>> + Send {
        let session = ThreadModelSession {
            runtime: self.runtime.clone(),
            reasoning: self.reasoning.clone(),
            purpose: self.purpose.clone(),
            hosted_tools: self.hosted_tools.clone(),
            physical: ModelSession::default(),
            observed: Arc::new(Mutex::new(None)),
        };
        async move { Ok(DynModelSession::new(session)) }
    }
}

struct ThreadModelSession {
    runtime: ModelRuntime,
    reasoning: Option<ReasoningConfig>,
    purpose: String,
    hosted_tools: Arc<[super::HostedTool]>,
    physical: ModelSession,
    observed: Arc<Mutex<Option<OpaquePayload>>>,
}

impl CoreModelSession for ThreadModelSession {
    async fn prepare(&mut self, request: ModelRequest) -> Result<PreparedModelCall, ModelError> {
        let mut encoded = codec::request(&request)?;
        encoded.reasoning = self.reasoning.clone();
        encoded.max_tokens = self.runtime.model().max_output_tokens;
        encoded.parallel_tool_calls = request.tool_call_mode
            == pl_core::model::ToolCallMode::Parallel
            && self.runtime.model().capabilities.tools.parallel_tool_calls;
        self.runtime
            .validate_context(&encoded.input)
            .map_err(|error| failure(ModelFailureKind::IncompatibleContext, error))?;
        let observed = self
            .observed
            .lock()
            .map_err(|_| failure(ModelFailureKind::Unavailable, AdapterError::Poisoned))?
            .clone();
        if observed != request.committed_private_context {
            self.physical
                .close()
                .await
                .map_err(|error| failure(ModelFailureKind::Unavailable, error))?;
            self.physical = ModelSession::default();
            *self
                .observed
                .lock()
                .map_err(|_| failure(ModelFailureKind::Unavailable, AdapterError::Poisoned))? =
                None;
        }
        let declarations = codec::declarations(&request.tools)?;
        let names = declarations
            .iter()
            .map(|(id, spec)| {
                (
                    spec.name().to_owned(),
                    codec::ToolBinding::new(id.clone(), spec),
                )
            })
            .collect::<BTreeMap<_, _>>();
        encoded.tools = declarations.into_iter().map(|(_, spec)| spec).collect();
        for tool in self.hosted_tools.iter() {
            let declaration = tool.declaration();
            if encoded
                .tools
                .iter()
                .any(|existing| existing.name() == declaration.name())
            {
                return Err(failure(
                    ModelFailureKind::UnsupportedContent,
                    AdapterError::Content(
                        "hosted and local tool declarations have conflicting identities",
                    ),
                ));
            }
            encoded.tools.push(declaration);
        }
        media::prepare(&request, &mut encoded, self.runtime.model()).await?;
        encoded = self
            .runtime
            .prepare_request(encoded)
            .map_err(|error| failure(ModelFailureKind::UnsupportedContent, error))?;
        let estimate = crate::completion::estimate_text_input_tokens(&encoded);
        let tool_projection = media::tool_projection(self.runtime.model())?;
        let runtime = self.runtime.clone();
        let binding = receipt::ModelCallBinding::capture(&runtime, &self.purpose);
        let request_metadata = receipt::request_metadata(&binding, &encoded)?;
        let observed = self.observed.clone();
        let marker = OpaquePayload::new("pl.model.continuation", 1, request.attempt_id.clone())
            .map_err(|error| failure(ModelFailureKind::InvalidResponse, error))?;
        let cache_key = cache::key(&runtime, &encoded)?;
        let invocation = ModelInvocationContext::new(self.physical.clone())
            .with_cancellation(Some(request.cancellation.clone()))
            .with_progress(request.progress.clone())
            .with_prompt_cache_key(cache_key);
        let call = PreparedModelCall::new(async move {
            *observed
                .lock()
                .map_err(|_| failure(ModelFailureKind::Unavailable, AdapterError::Poisoned))? =
                Some(marker.clone());
            let response = runtime
                .complete(encoded, invocation)
                .await
                .map_err(|error| receipt::failure_error(binding.clone(), error))?;
            let output = codec::response(
                codec::ResponseContext {
                    request: &request,
                    names: &names,
                    marker: marker.clone(),
                    binding,
                },
                response,
            )?;
            Ok(output)
        });
        let call = call
            .with_tool_projection(tool_projection)
            .with_request_metadata(request_metadata);
        Ok(match estimate {
            Some(tokens) => call.with_input_estimate(pl_core::model::TokenEstimate {
                tokens,
                accuracy: pl_core::model::EstimateAccuracy::Approximate,
            }),
            None => call,
        })
    }

    async fn close(&mut self) -> Result<(), ModelError> {
        self.physical
            .close()
            .await
            .map_err(|error| failure(ModelFailureKind::Unavailable, error))
    }
}

/// Encodes a stable tool declaration for the Thread adapter without exposing its schema to core.
///
/// # Errors
/// Returns declaration encoding failure.
pub fn thread_tool_declaration(spec: &ToolSpec) -> Result<OpaquePayload, ModelError> {
    let mut value = serde_json::to_value(spec)
        .map_err(|error| failure(ModelFailureKind::UnsupportedContent, error))?;
    crate::completion::canonicalize_json(&mut value);
    OpaquePayload::new("pl.model.tool-spec", 1, value.to_string())
        .map_err(|error| failure(ModelFailureKind::UnsupportedContent, error))
}

#[derive(Debug, thiserror::Error)]
enum AdapterError {
    #[error("model session observation state is poisoned")]
    Poisoned,
    #[error("unsupported or invalid model history: {0}")]
    Content(&'static str),
}

fn failure(
    kind: ModelFailureKind,
    source: impl std::error::Error + Send + Sync + 'static,
) -> ModelError {
    ModelError {
        details: None,
        kind,
        usage: ModelUsage::default(),
        source: Some(Box::new(source)),
    }
}

fn usage(report: &pl_protocol::UsageReport) -> ModelUsage {
    ModelUsage {
        input_tokens: report.input_tokens,
        output_tokens: report.output_tokens,
        cache_read_tokens: report.cache_read_tokens,
        cache_write_tokens: report.cache_write_tokens,
        reasoning_tokens: report.reasoning_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{
        context::ContextContent,
        thread::{StepInput, ThreadHandle},
    };
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn provider_response_commits_through_core_and_replays_without_invocation() {
        let response = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"reply\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":3,\"total_tokens\":15}}\n\n",
            "data: [DONE]\n\n",
        );
        let (url, server) = super::super::test_support::serve_sse_once(response.into()).await;
        let runtime = ModelRuntime::new(
            crate::provider::ProviderEndpoint::deepseek(Some(url)),
            crate::model::ModelInfo::compatible("thread-test"),
        )
        .unwrap();
        let model = ThreadModel::new(runtime, None);
        let thread =
            ThreadHandle::start("thread".into(), model.open_session().await.unwrap()).unwrap();
        let output = thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: vec![ContextContent::Text {
                    text: Arc::from("hello"),
                }],
                cancellation: tokio_util::sync::CancellationToken::new(),
            })
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(
            output.content[0],
            ContextContent::Text {
                text: Arc::from("reply")
            }
        );
        assert_eq!(output.usage.input_tokens, Some(12));
        let snapshot = thread.snapshot();
        let replayed = pl_core::thread::journal::replay(&thread.journal().await.unwrap()).unwrap();
        assert_eq!(replayed.context, snapshot.context);
        assert_eq!(replayed.private_context, snapshot.private_context);
        thread.close().await.unwrap();
    }
    #[tokio::test]
    async fn mixed_catalog_keeps_parallel_request_option_subject_to_model_capability() {
        for supports_parallel in [false, true] {
            let mut info = crate::model::ModelInfo::compatible("parallel-test");
            info.capabilities.tools.parallel_tool_calls = supports_parallel;
            let runtime =
                ModelRuntime::new(crate::provider::ProviderEndpoint::deepseek(None), info).unwrap();
            let mut session = ThreadModel::new(runtime, None)
                .open_session()
                .await
                .unwrap();
            let declarations = ["read_file", "discover_tools"]
                .into_iter()
                .map(|name| pl_core::model::ModelToolDeclaration {
                    tool_id: name.into(),
                    declaration: thread_tool_declaration(&ToolSpec::function(
                        name,
                        "tool",
                        serde_json::json!({"type":"object"}),
                    ))
                    .unwrap(),
                })
                .collect::<Vec<_>>();
            let request = ModelRequest {
                thread_id: "thread".into(),
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                context: Default::default(),
                tools: declarations.into(),
                tool_call_mode: pl_core::model::ToolCallMode::Parallel,
                solo_tool_ids: vec!["discover_tools".into()].into(),
                committed_private_context: None,
                resources: None,
                progress: None,
                cancellation: Default::default(),
            };
            let prepared = session.prepare(request).await.unwrap();
            let metadata = prepared.request_metadata().unwrap();
            let receipt = model_request_receipt(metadata).unwrap();
            assert_eq!(receipt.parallel_tool_calls, supports_parallel);
            session.close().await.unwrap();
        }
    }

    #[tokio::test]
    async fn hosted_capability_conflicts_are_rejected_before_a_model_request_is_admitted() {
        let runtime = ModelRuntime::new(
            crate::provider::ProviderEndpoint::deepseek(None),
            crate::model::ModelInfo::compatible("hosted-test"),
        )
        .unwrap();
        let factory = ThreadModel::new(runtime, None).with_hosted_tools(vec![
            super::super::HostedTool::WebSearch(pl_protocol::HostedWebSearchOptions::DeepSeek),
        ]);
        let mut session = factory.open_session().await.unwrap();
        let declaration = thread_tool_declaration(&ToolSpec::function(
            "web_search",
            "local search",
            serde_json::json!({"type":"object"}),
        ))
        .unwrap();
        let request = ModelRequest {
            tool_call_mode: pl_core::model::ToolCallMode::Parallel,
            solo_tool_ids: Vec::new().into(),
            thread_id: "thread".into(),
            turn_id: "turn".into(),
            attempt_id: "attempt".into(),
            context: Default::default(),
            tools: vec![pl_core::model::ModelToolDeclaration {
                tool_id: "local".into(),
                declaration,
            }]
            .into(),
            committed_private_context: None,
            resources: None,
            progress: None,
            cancellation: tokio_util::sync::CancellationToken::new(),
        };
        let error = session.prepare(request).await.unwrap_err();
        assert_eq!(error.kind, ModelFailureKind::UnsupportedContent);
        assert!(
            error
                .source
                .unwrap()
                .to_string()
                .contains("conflicting identities")
        );
        session.close().await.unwrap();
    }
    #[tokio::test]
    async fn stream_preview_is_visible_before_completion_and_cleared_after_commit() {
        use tokio::io::AsyncWriteExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (release, released) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            super::super::test_support::capture_http_request(&mut socket).await;
            let prefix = "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n";
            let suffix = "data: {\"choices\":[{\"delta\":{\"content\":\" answer\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
            let header = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                prefix.len() + suffix.len()
            );
            socket.write_all(header.as_bytes()).await.unwrap();
            socket.write_all(prefix.as_bytes()).await.unwrap();
            socket.flush().await.unwrap();
            released.await.unwrap();
            socket.write_all(suffix.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        });
        let model = ThreadModel::new(
            ModelRuntime::new(
                crate::provider::ProviderEndpoint::deepseek(Some(format!("http://{address}"))),
                crate::model::ModelInfo::compatible("stream-test"),
            )
            .unwrap(),
            None,
        );
        let thread =
            ThreadHandle::start("stream".into(), model.open_session().await.unwrap()).unwrap();
        let mut subscription = thread.subscribe();
        let request_thread = thread.clone();
        let execution = tokio::spawn(async move {
            request_thread
                .step(StepInput {
                    turn_id: "turn".into(),
                    attempt_id: "attempt".into(),
                    content: vec![ContextContent::Text {
                        text: "hello".into(),
                    }],
                    cancellation: Default::default(),
                })
                .await
        });
        let observed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let snapshot = subscription.next().await.expect("thread remains open");
                if let Some(preview) = snapshot.model_progress
                    && !preview.progress.content.is_empty()
                {
                    break preview;
                }
            }
        })
        .await
        .expect("live preview must arrive while the provider is paused");
        assert_eq!(observed.attempt_id, "attempt");
        assert_eq!(
            observed.progress.content,
            vec![ContextContent::Text {
                text: "partial".into()
            }]
        );
        assert!(!execution.is_finished());
        release.send(()).unwrap();
        let output = execution.await.unwrap().unwrap();
        assert_eq!(
            output.content[0],
            ContextContent::Text {
                text: "partial answer".into()
            }
        );
        assert!(thread.snapshot().model_progress.is_none());
        server.await.unwrap();
        thread.close().await.unwrap();
    }
}
