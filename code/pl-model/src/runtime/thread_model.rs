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
pub use media::{ToolAttachment, attachment_content, decode_attachment};
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
        let mut declarations = codec::declarations(&request.tools)?;
        for (id, declaration) in &mut declarations {
            if request.solo_tool_ids.contains(id) {
                match declaration {
                    ToolSpec::Function { description, .. }
                    | ToolSpec::Custom { description, .. } => {
                        description.push_str(" This tool must be the only tool call in its model response; never batch it with queries or other tools.");
                    }
                    ToolSpec::ProgrammaticToolCalling | ToolSpec::WebSearch { .. } => {}
                }
            }
        }
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
        let terminal_progress = request.progress.clone();
        let observed = self.observed.clone();
        let marker = OpaquePayload::new("pl.model.continuation", 1, request.attempt_id.clone())
            .map_err(|error| failure(ModelFailureKind::InvalidResponse, error))?;
        let cache_key = cache::key(&runtime, &encoded)?;
        let invocation = ModelInvocationContext::new(self.physical.clone())
            .with_trace_metadata(crate::completion::CompletionTraceContext {
                session_id: request.thread_id.clone(),
                turn_id: request.turn_id.clone(),
                inference_id: request.attempt_id.clone(),
            })
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
                .map_err(|error| {
                    receipt::failure_error(
                        binding.clone(),
                        error,
                        terminal_progress.as_ref().map(|sender| sender.latest()),
                    )
                })?;
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
