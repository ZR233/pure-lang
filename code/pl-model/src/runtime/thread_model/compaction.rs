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
                        ThreadCompactionStrategy::PreferNative,
                    )
                })
        } else {
            self.runtime
                .summarize(
                    TextSummaryRequest {
                        instructions: &options.instructions,
                        input: encoded.input,
                        prepared_content: encoded.prepared_content,
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
                        ThreadCompactionStrategy::TextSummary,
                    )
                })
        };
        let cleanup = session.close().await;
        let (item, text, accounting, implementation) = match result {
            Ok(result) => {
                if let Err(source) = cleanup {
                    return Err(receipt::failure_error(
                        binding,
                        crate::completion::CompletionFailure {
                            source,
                            accounting: Box::new(result.2),
                        },
                    ));
                }
                result
            }
            Err(mut error) => {
                if let Err(cleanup) = cleanup {
                    error.source = crate::completion::PureError::Io(std::io::Error::other(
                        CompactionCleanupError {
                            primary: error.source,
                            cleanup,
                        },
                    ));
                }
                return Err(receipt::failure_error(binding, error));
            }
        };
        let content = if let Some(item) = item {
            let content = serde_json::to_string(&item)
                .map_err(|error| failure(ModelFailureKind::InvalidResponse, error))?;
            vec![ContextContent::Opaque {
                payload: OpaquePayload::new(FORMAT, 1, content)
                    .map_err(|error| failure(ModelFailureKind::InvalidResponse, error))?,
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

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{model::Model, thread::ThreadHandle};
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn summary_commits_by_revision_and_replays_as_context_without_reinvocation() {
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"project alpha\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":3,\"total_tokens\":15}}\n\n",
            "data: [DONE]\n\n"
        );
        let (url, server) =
            crate::runtime::test_support::serve_sse_checked(sse.into(), |request| {
                request.body.get("tools").is_none()
                    && request.body.get("tool_choice").is_none()
                    && request.body["messages"].as_array().is_some_and(|messages| {
                        messages.iter().any(|message| {
                            message["content"]
                                .as_str()
                                .is_some_and(|text| text.contains("Keep project facts"))
                        })
                    })
            })
            .await;
        let runtime = crate::runtime::ModelRuntime::new(
            crate::provider::ProviderEndpoint::deepseek(Some(url)),
            crate::model::ModelInfo::compatible("compact-test"),
        )
        .unwrap();
        let model = ThreadModel::new(runtime, None);
        let thread =
            ThreadHandle::start("thread".into(), model.open_session().await.unwrap()).unwrap();
        let records = vec![
            ContextRecord {
                id: "instruction".into(),
                turn_id: None,
                source: ContextSource::Instruction,
                content: vec![ContextContent::Text {
                    text: "Keep facts".into(),
                }],
                tool_calls: vec![],
            },
            ContextRecord {
                id: "user".into(),
                turn_id: Some("turn".into()),
                source: ContextSource::User,
                content: vec![ContextContent::Text {
                    text: "project alpha; obsolete detail".into(),
                }],
                tool_calls: vec![],
            },
        ];
        thread
            .replace_context(ReplaceContext {
                expected_revision: 0,
                reason: ContextReplacementReason::Rebuild,
                records,
            })
            .await
            .unwrap();
        let snapshot = thread.snapshot();
        let compacted = model
            .compact(
                ModelRequest {
                    tool_call_mode: pl_core::model::ToolCallMode::Parallel,
                    solo_tool_ids: Vec::new().into(),
                    thread_id: "thread".into(),
                    turn_id: "turn".into(),
                    attempt_id: "summary".into(),
                    context: snapshot.context.clone(),
                    tools: [].into(),
                    committed_private_context: None,
                    resources: None,
                    cancellation: Default::default(),
                    progress: None,
                },
                ThreadCompactionOptions {
                    strategy: ThreadCompactionStrategy::PreferNative,
                    instructions: "Summarize".into(),
                    requirement: "Keep project facts".into(),
                    summary_prefix: "Summary".into(),
                    max_output_tokens: Some(32),
                },
            )
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(
            thread.snapshot().context,
            snapshot.context,
            "reduction alone must not mutate the main thread"
        );
        assert_eq!(
            compacted.implementation,
            ThreadCompactionStrategy::TextSummary
        );
        assert_eq!(compacted.accounting.usage.input_tokens, Some(12));
        assert_eq!(
            compacted.replacement.records[0],
            snapshot.context.records[0]
        );
        thread.replace_context(compacted.replacement).await.unwrap();
        let current = thread.snapshot();
        assert_eq!(
            current.context.records[1].content,
            vec![ContextContent::Text {
                text: "Summary\nproject alpha".into()
            }]
        );
        assert_eq!(
            pl_core::thread::journal::replay(&thread.journal().await.unwrap())
                .unwrap()
                .context,
            current.context
        );
        thread.close().await.unwrap();
    }

    #[test]
    fn checkpoint_replay_requires_model_runtime_authority_and_valid_content() {
        let payload = OpaquePayload::new(
            FORMAT,
            1,
            serde_json::to_string(&ModelContextItem::Compaction {
                encrypted_content: "encrypted".into(),
            })
            .unwrap(),
        )
        .unwrap();
        let mut record = ContextRecord {
            id: "checkpoint".into(),
            turn_id: None,
            source: ContextSource::Runtime {
                source_id: "model.compaction".into(),
            },
            content: vec![ContextContent::Opaque { payload }],
            tool_calls: vec![],
        };
        assert!(
            matches!(decode(&record).unwrap(), Some(ModelContextItem::Compaction { encrypted_content }) if encrypted_content == "encrypted")
        );
        record.source = ContextSource::User;
        assert_eq!(
            decode(&record).unwrap_err().kind,
            ModelFailureKind::IncompatibleContext
        );
    }
}
