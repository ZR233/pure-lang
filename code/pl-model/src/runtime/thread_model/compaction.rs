//! Model-owned context reduction; the host commits replacements using the frozen revision.
use super::{AdapterError, ThreadModel, codec, failure, receipt};
use crate::{
    completion::{InferenceAccounting, ModelContextItem},
    runtime::{ModelInvocationContext, ModelSession, TextSummaryRequest},
};
use pl_core::{
    context::{ContextContent, ContextRecord, ContextSource, OpaquePayload},
    model::{ModelError, ModelFailureKind, ModelRequest},
    thread::{ContextReplacementReason, ReplaceContext},
};

const FORMAT: &str = "pl.model.compaction";

/// Host-selected reduction behavior. Native support is queried from the selected adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadCompactionStrategy {
    TextSummary,
    PreferNative,
}

/// Product wording and output budget; model adapters own all protocol conversion.
#[derive(Debug, Clone)]
pub struct ThreadCompactionOptions {
    pub strategy: ThreadCompactionStrategy,
    pub instructions: String,
    pub requirement: String,
    pub summary_prefix: String,
    pub max_output_tokens: Option<u64>,
}

/// Successful reduction before the host's compare-and-swap commit.
#[derive(Debug)]
pub struct ThreadCompaction {
    pub binding: super::receipt::ModelCallBinding,
    pub replacement: ReplaceContext,
    pub accounting: InferenceAccounting,
    pub model_observation: Option<crate::completion::InferenceModelObservation>,
    pub implementation: ThreadCompactionStrategy,
}

impl ThreadModel {
    /// Estimates a frozen model-owned encoding without opening or advancing a physical session.
    /// # Errors
    /// Rejects invalid history, unsupported declarations and unavailable retained media.
    pub async fn estimate_input(
        &self,
        request: &ModelRequest,
    ) -> Result<Option<pl_core::model::TokenEstimate>, ModelError> {
        let mut encoded = codec::request(request)?;
        self.runtime
            .validate_context(&encoded.input)
            .map_err(|error| failure(ModelFailureKind::IncompatibleContext, error))?;
        encoded.reasoning = self.reasoning.clone();
        encoded.parallel_tool_calls = request.tool_call_mode
            == pl_core::model::ToolCallMode::Parallel
            && self.runtime.model().capabilities.tools.parallel_tool_calls;
        encoded.tools = codec::declarations(&request.tools)?
            .into_iter()
            .map(|(_, spec)| spec)
            .collect();
        encoded.tools.extend(
            self.hosted_tools
                .iter()
                .map(crate::runtime::HostedTool::declaration),
        );
        super::media::prepare(request, &mut encoded, self.runtime.model()).await?;
        let encoded = self
            .runtime
            .prepare_request(encoded)
            .map_err(|error| failure(ModelFailureKind::UnsupportedContent, error))?;
        Ok(
            crate::completion::estimate_text_input_tokens(&encoded).map(|tokens| {
                pl_core::model::TokenEstimate {
                    tokens,
                    accuracy: pl_core::model::EstimateAccuracy::Approximate,
                }
            }),
        )
    }

    /// Reduces frozen history using an independent physical model session.
    ///
    /// # Errors
    /// Returns invalid context or model failures with observed usage. The caller's history and
    /// continuation remain untouched, including when reduction or session cleanup fails.
    pub async fn compact(
        &self,
        request: ModelRequest,
        options: ThreadCompactionOptions,
    ) -> Result<ThreadCompaction, ModelError> {
        let mut encoded = codec::request(&request)?;
        self.runtime
            .validate_context(&encoded.input)
            .map_err(|error| failure(ModelFailureKind::IncompatibleContext, error))?;
        encoded.reasoning = self.reasoning.clone();
        encoded.parallel_tool_calls = request.tool_call_mode
            == pl_core::model::ToolCallMode::Parallel
            && self.runtime.model().capabilities.tools.parallel_tool_calls;
        encoded.tools = codec::declarations(&request.tools)?
            .into_iter()
            .map(|(_, spec)| spec)
            .collect();
        super::media::prepare(&request, &mut encoded, self.runtime.model()).await?;
        let binding = receipt::ModelCallBinding::capture(&self.runtime, "compaction");
        let session = ModelSession::default();
        let invocation = ModelInvocationContext::new(session.clone())
            .with_cancellation(Some(request.cancellation.clone()));
        let native = (options.strategy == ThreadCompactionStrategy::PreferNative
            && encoded.attachments.is_empty()
            && encoded.prepared_content.is_empty())
        .then(|| self.runtime.compaction())
        .flatten();
        let result = if let Some(native) = native {
            native
                .checkpoint(encoded, invocation)
                .await
                .map(|checkpoint| {
                    (
                        Some(checkpoint.item),
                        None,
                        checkpoint.accounting,
                        checkpoint.model_observation,
                        ThreadCompactionStrategy::PreferNative,
                    )
                })
        } else {
            self.runtime
                .summarize(
                    TextSummaryRequest {
                        instructions: &options.instructions,
                        input: encoded.input,
                        attachments: encoded.attachments,
                        reasoning: encoded.reasoning,
                        tools: &encoded.tools,
                        requirement: &options.requirement,
                        max_output_tokens: options.max_output_tokens,
                        empty_summary_error: "context compaction returned an empty summary",
                    },
                    invocation,
                )
                .await
                .map(|summary| {
                    (
                        None,
                        Some(summary.text),
                        summary.accounting,
                        summary.model_observation,
                        ThreadCompactionStrategy::TextSummary,
                    )
                })
        };
        let cleanup = session.close().await;
        let (item, text, accounting, model_observation, implementation) = match result {
            Ok(result) => {
                if let Err(source) = cleanup {
                    return Err(receipt::failure_error(
                        binding,
                        crate::completion::CompletionFailure {
                            source: Box::new(source),
                            accounting: Box::new(result.2),
                            model_observation: result.3.map(Box::new),
                            cancelled: false,
                        },
                        None,
                    ));
                }
                result
            }
            Err(mut error) => {
                if let Err(cleanup) = cleanup {
                    error.source = Box::new(crate::completion::PureError::Io(
                        std::io::Error::other(CompactionCleanupError {
                            primary: *error.source,
                            cleanup,
                        }),
                    ));
                }
                return Err(receipt::failure_error(binding, error, None));
            }
        };
        let content = if let Some(item) = item {
            let content = serde_json::to_string(&item).map_err(|error| {
                receipt::postprocess_failure_error(
                    binding.clone(),
                    accounting.clone(),
                    model_observation.clone(),
                    None,
                    failure(ModelFailureKind::InvalidResponse, error),
                )
            })?;
            vec![ContextContent::Opaque {
                payload: OpaquePayload::new(FORMAT, 1, content).map_err(|error| {
                    receipt::postprocess_failure_error(
                        binding.clone(),
                        accounting.clone(),
                        model_observation.clone(),
                        None,
                        failure(ModelFailureKind::InvalidResponse, error),
                    )
                })?,
            }]
        } else {
            vec![ContextContent::Text {
                text: format!("{}\n{}", options.summary_prefix, text.unwrap_or_default()).into(),
            }]
        };
        let mut records = request
            .context
            .records
            .iter()
            .filter(|record| record.source == ContextSource::Instruction)
            .cloned()
            .collect::<Vec<_>>();
        records.push(ContextRecord {
            id: format!("compaction:{}", request.attempt_id),
            turn_id: None,
            source: ContextSource::Runtime {
                source_id: "model.compaction".into(),
            },
            content,
            tool_calls: Vec::new(),
        });
        Ok(ThreadCompaction {
            binding,
            replacement: ReplaceContext {
                expected_revision: request.context.revision,
                reason: ContextReplacementReason::Compaction,
                records,
            },
            accounting,
            model_observation,
            implementation,
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("compaction failed ({primary}); independent session cleanup also failed ({cleanup})")]
struct CompactionCleanupError {
    #[source]
    primary: crate::completion::PureError,
    cleanup: crate::completion::PureError,
}

pub(super) fn decode(record: &ContextRecord) -> Result<Option<ModelContextItem>, ModelError> {
    let contains = record.content.iter().any(|content| matches!(content, ContextContent::Opaque { payload } if payload.format() == FORMAT));
    if !contains {
        return Ok(None);
    }
    let [ContextContent::Opaque { payload }] = record.content.as_slice() else {
        return Err(failure(
            ModelFailureKind::IncompatibleContext,
            AdapterError::Content("compaction record must contain one checkpoint"),
        ));
    };
    if payload.version() != 1
        || record.source
            != (ContextSource::Runtime {
                source_id: "model.compaction".into(),
            })
        || !record.tool_calls.is_empty()
    {
        return Err(failure(
            ModelFailureKind::IncompatibleContext,
            AdapterError::Content("invalid compaction record authority or version"),
        ));
    }
    let item: ModelContextItem = serde_json::from_str(payload.content())
        .map_err(|error| failure(ModelFailureKind::IncompatibleContext, error))?;
    if !matches!(&item, ModelContextItem::Compaction { encrypted_content } if !encrypted_content.is_empty())
    {
        return Err(failure(
            ModelFailureKind::IncompatibleContext,
            AdapterError::Content("compaction record contains no native checkpoint"),
        ));
    }
    Ok(Some(item))
}
