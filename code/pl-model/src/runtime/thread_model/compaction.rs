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
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeCheckpoint {
    binding: super::receipt::ModelCallBinding,
    #[serde(default)]
    compatibility_family: Option<String>,
    item: ModelContextItem,
}

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
    /// `previous` must be a usage origin whose retained prefix and tool plan core has validated.
    /// Native history adds observed output and new text; unmeasurable additions stay unknown.
    /// # Errors
    /// Rejects invalid history, unsupported declarations and unavailable retained media.
    pub async fn estimate_input(
        &self,
        request: &ModelRequest,
        previous: Option<&pl_core::thread::context_preparation::PreviousContextUsage>,
    ) -> Result<Option<pl_core::model::TokenEstimate>, ModelError> {
        self.validate_context_origin(&request.context)?;
        let mut encoded = codec::request(request, &self.runtime)?;
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
        let tokens = crate::completion::estimate_text_input_tokens(&encoded).or_else(|| {
            // Core verifies the immutable input prefix and tool plan before supplying this
            // observation. Native assistant output is counted by its reported output usage.
            let previous = previous.filter(|previous| {
                previous.origin.binding.route_identity.as_deref()
                    == Some(self.route_identity().as_str())
            })?;
            let tail = request
                .context
                .records
                .get(previous.origin.input_record_count..)?;
            let characters = tail
                .iter()
                .filter(|record| record.source != ContextSource::Assistant)
                .flat_map(|record| &record.content)
                .try_fold(0_u64, |total, content| match content {
                    ContextContent::Text { text } => {
                        total.checked_add(u64::try_from(text.chars().count()).ok()?)
                    }
                    ContextContent::Resource { .. } | ContextContent::Opaque { .. } => None,
                })?;
            previous
                .usage
                .input_tokens?
                .checked_add(previous.usage.output_tokens?)?
                .checked_add(characters.div_ceil(4))
        });
        Ok(tokens.map(|tokens| pl_core::model::TokenEstimate {
            tokens,
            accuracy: pl_core::model::EstimateAccuracy::Approximate,
        }))
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
        self.validate_context_origin(&request.context)?;
        let mut encoded = codec::request(&request, &self.runtime)?;
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
            .with_cancellation(Some(request.cancellation.clone()))
            .with_prompt_cache_key(super::cache::key(&self.runtime, &request.thread_id))
            .with_trace_metadata(crate::completion::CompletionTraceContext {
                session_id: request.thread_id.clone(),
                turn_id: request.turn_id.clone(),
                inference_id: request.attempt_id.clone(),
            });
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
                        prefix: encoded,
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
                            presentation_items: Vec::new(),
                            partial_progress: None,
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
            let content = serde_json::to_string(&NativeCheckpoint {
                binding: binding.clone(),
                compatibility_family: self
                    .runtime
                    .model()
                    .capabilities
                    .native_context_family
                    .clone(),
                item,
            })
            .map_err(|error| {
                receipt::postprocess_failure_error(
                    binding.clone(),
                    accounting.clone(),
                    model_observation.clone(),
                    None,
                    failure(ModelFailureKind::InvalidResponse, error),
                )
            })?;
            vec![ContextContent::Opaque {
                payload: OpaquePayload::new(FORMAT, 2, content).map_err(|error| {
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
        // Compression canonicalizes instruction revisions to the current complete snapshot.
        let snapshot = request
            .context
            .records
            .iter()
            .rev()
            .find(|record| matches!(record.source, ContextSource::InstructionSnapshot { .. }));
        let mut records = if let Some(record) = snapshot {
            vec![record.clone()]
        } else {
            request
                .context
                .records
                .iter()
                .filter(|record| record.source == ContextSource::Instruction)
                .cloned()
                .collect::<Vec<_>>()
        };
        records.push(ContextRecord {
            id: format!("compaction:{}", request.attempt_id),
            turn_id: None,
            source: ContextSource::Runtime {
                source_id: "model.compaction".into(),
            },
            content,
            tool_calls: Vec::new(),
        });
        // The last complete tool batch is immediate execution evidence. Reducing it to prose
        // alone makes an aggressively compacted agent repeatedly verify the same operation.
        let latest_batch = request.context.records.iter().rposition(|record| {
            record.source == ContextSource::Assistant
                && record.turn_id.as_deref() == Some(request.turn_id.as_str())
                && !record.tool_calls.is_empty()
        });
        // Delegated inputs and the active task survive independently of summarizer quality.
        records.extend(
            request
                .context
                .records
                .iter()
                .enumerate()
                .filter(|(index, record)| {
                    (record.source == ContextSource::User
                        && record.turn_id.as_deref() == Some(request.turn_id.as_str()))
                        || matches!(&record.source, ContextSource::AgentMessage { purpose, .. } if *purpose != pl_core::context::AgentMessageKind::Report)
                        || (latest_batch.is_some_and(|start| *index >= start)
                            && !matches!(record.source, ContextSource::Instruction
                                | ContextSource::InstructionSnapshot { .. }
                                | ContextSource::RuntimeFact { .. }))
                })
                .map(|(_, record)| record)
                .cloned(),
        );
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

pub(super) fn decode(
    record: &ContextRecord,
    runtime: &super::ModelRuntime,
) -> Result<Option<ModelContextItem>, ModelError> {
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
    if payload.version() != 2
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
    let checkpoint: NativeCheckpoint = serde_json::from_str(payload.content())
        .map_err(|error| failure(ModelFailureKind::IncompatibleContext, error))?;
    let target = super::receipt::ModelCallBinding::capture(runtime, "replay");
    let origin = &checkpoint.binding;
    let same_isolation = origin.provider_instance_id == target.provider_instance_id
        && origin.isolation == target.isolation
        && origin.adapter == target.adapter
        && origin.protocol == target.protocol;
    let same_model = origin.requested_model == target.requested_model;
    let explicit_family = checkpoint
        .compatibility_family
        .as_ref()
        .filter(|family| !family.is_empty())
        .is_some_and(|family| {
            runtime.model().capabilities.native_context_family.as_ref() == Some(family)
        });
    if !same_isolation || (!same_model && !explicit_family) {
        return Err(failure(
            ModelFailureKind::IncompatibleContext,
            AdapterError::Content(
                "native checkpoint origin is incompatible with this binding; original session preserved",
            ),
        ));
    }
    let item = checkpoint.item;
    if !matches!(&item, ModelContextItem::Compaction { encrypted_content } if !encrypted_content.trim().is_empty())
    {
        return Err(failure(
            ModelFailureKind::IncompatibleContext,
            AdapterError::Content("compaction record contains no native checkpoint"),
        ));
    }
    Ok(Some(item))
}

/// Explicit migration boundary: only an identity-matched native receipt can bind old material.
/// Unproven payloads remain untouched and future request preparation rejects them.
/// # Errors
/// Returns malformed recognized producer data without deleting the original record.
pub fn migrate_legacy_compaction(
    record: &mut ContextRecord,
    inference_id: &str,
    binding: super::receipt::ModelCallBinding,
) -> Result<bool, ModelError> {
    if record.id != format!("compaction:{inference_id}")
        || record.source
            != (ContextSource::Runtime {
                source_id: "model.compaction".into(),
            })
    {
        return Ok(false);
    }
    let [ContextContent::Opaque { payload }] = record.content.as_slice() else {
        return Ok(false);
    };
    if payload.format() != FORMAT || payload.version() != 1 {
        return Ok(false);
    }
    let item: ModelContextItem = serde_json::from_str(payload.content())
        .map_err(|error| failure(ModelFailureKind::IncompatibleContext, error))?;
    if !matches!(&item, ModelContextItem::Compaction { encrypted_content } if !encrypted_content.trim().is_empty())
    {
        return Ok(false);
    }
    let bytes = serde_json::to_string(&NativeCheckpoint {
        binding,
        compatibility_family: None,
        item,
    })
    .map_err(|error| failure(ModelFailureKind::IncompatibleContext, error))?;
    record.content = vec![ContextContent::Opaque {
        payload: OpaquePayload::new(FORMAT, 2, bytes)
            .map_err(|error| failure(ModelFailureKind::IncompatibleContext, error))?,
    }];
    Ok(true)
}

impl ThreadModel {
    /// Validates retained checkpoint authority before a host replaces the active model session.
    /// # Errors
    /// Refuses unbound, multiple, empty or incompatible native checkpoints.
    pub fn validate_context_origin(
        &self,
        context: &pl_core::context::ContextSnapshot,
    ) -> Result<(), ModelError> {
        let mut checkpoints = Vec::new();
        for record in context.records.iter() {
            if let Some(item) = decode(record, &self.runtime)? {
                checkpoints.push(item);
            }
        }
        if checkpoints.len() > 1 {
            return Err(failure(
                ModelFailureKind::IncompatibleContext,
                AdapterError::Content("multiple native checkpoints in current context"),
            ));
        }
        self.runtime
            .validate_context(&checkpoints)
            .map_err(|error| failure(ModelFailureKind::IncompatibleContext, error))
    }
}
