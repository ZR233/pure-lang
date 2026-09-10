//! Committed context reductions are distinct from auxiliary inference accounting.
use super::{ProjectionError, order};
use pl_core::thread::{
    ContextReplacementReason, ThreadSnapshot, extensions::ExtensionChange, journal::ThreadCommit,
};
use pl_protocol::{ThreadContextCompactionItem, ThreadItem, ThreadItemState};
use std::sync::Arc;

pub(super) fn receipt(
    payload: &pl_core::context::OpaquePayload,
) -> Result<Option<crate::compaction::CompactionReceipt>, ProjectionError> {
    if payload.format() != "pl.studio.compaction" {
        return Ok(None);
    }
    if payload.version() != 1 {
        return Err(ProjectionError::UnsupportedOutput(
            "unsupported compaction receipt version".into(),
        ));
    }
    Ok(Some(serde_json::from_str(payload.content())?))
}

pub(super) fn project_compactions(
    thread_id: &str,
    snapshot: &ThreadSnapshot,
    journal: &[Arc<ThreadCommit>],
) -> Result<Vec<ThreadItem>, ProjectionError> {
    let mut items = Vec::new();
    for commit in journal
        .iter()
        .filter(|commit| commit.sequence <= snapshot.commit_sequence)
    {
        if !commit
            .replacements
            .iter()
            .any(|replacement| replacement.reason == ContextReplacementReason::Compaction)
        {
            continue;
        }
        for extension in commit.extensions.iter() {
            let ExtensionChange::Put { id, record } = extension else {
                continue;
            };
            let (turn_id, state) = match receipt(&record.payload) {
                Ok(Some(receipt)) if receipt.implementation.is_some() => (
                    receipt.turn_id,
                    ThreadItemState::ContextCompaction(ThreadContextCompactionItem::new(
                        None,
                        None,
                        commit.committed_at,
                    )),
                ),
                Ok(Some(_)) | Ok(None) => continue,
                Err(error) => (
                    String::new(),
                    ThreadItemState::Raw(pl_protocol::ThreadRawItem {
                        payloads: vec![super::raw_payload(&record.payload)],
                        notice: error.to_string(),
                        recorded_at: commit.committed_at,
                    }),
                ),
            };
            items.push(ThreadItem::new(
                order::compaction_id(id),
                thread_id.into(),
                turn_id,
                0,
                record.revision,
                commit.committed_at,
                commit.committed_at,
                state,
            ));
        }
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{
        context::{ContextContent, ContextRecord, ContextSource, OpaquePayload},
        model::{
            DynModelSession, ModelError, ModelRequest, ModelSession, ModelStepOutput,
            PreparedModelCall,
        },
        thread::{
            ReplaceContext, StepInput, ThreadHandle,
            context_preparation::{
                ContextPreparation, ContextPreparationHook, ContextPreparationRequest,
                ContextPreparer,
            },
            extensions::ExtensionMutation,
        },
    };
    use pretty_assertions::assert_eq;

    struct Model;
    impl ModelSession for Model {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            Ok(PreparedModelCall::new(async move {
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: vec![],
                    tool_calls: vec![],
                    private_context: None,
                    usage: Default::default(),
                })
            }))
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }
    #[derive(Debug)]
    struct Hook;
    impl ContextPreparationHook for Hook {
        async fn before_step(&self, request: ContextPreparationRequest) -> ContextPreparation {
            let receipt = crate::compaction::CompactionReceipt {
                inference_id: "compaction-attempt".into(),
                binding: pl_model::runtime::ModelCallBinding {
                    provider_instance_id: "frozen-provider".into(),
                    requested_model: "frozen-model".into(),
                    adapter: pl_model::provider::ProviderAdapterKind::DeepSeek,
                    protocol: pl_model::provider::ProviderWireProtocol::ChatCompletions,
                    isolation: "frozen-isolation".into(),
                    purpose: "compaction".into(),
                },
                reasoning_effort: None,
                context_window: None,
                turn_id: request.model.turn_id,
                implementation: Some("native".into()),
                error: None,
                accounting: pl_protocol::InferenceAccounting {
                    usage: pl_protocol::UsageReport {
                        input_tokens: Some(7),
                        output_tokens: Some(2),
                        total_tokens: Some(9),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            };
            ContextPreparation::Replaced {
                replacement: ReplaceContext {
                    expected_revision: request.model.context.revision,
                    reason: ContextReplacementReason::Compaction,
                    records: vec![ContextRecord {
                        id: "summary".into(),
                        turn_id: None,
                        source: ContextSource::Runtime {
                            source_id: "summary".into(),
                        },
                        content: vec![ContextContent::Text {
                            text: "summary".into(),
                        }],
                        tool_calls: vec![],
                    }],
                },
                mutations: vec![ExtensionMutation::Put {
                    id: "compaction".into(),
                    expected_revision: None,
                    payload: OpaquePayload::new(
                        "pl.studio.compaction",
                        1,
                        serde_json::to_string(&receipt).unwrap(),
                    )
                    .unwrap(),
                }],
            }
        }
    }
    #[tokio::test]
    async fn reduction_timeline_preserves_unknown_token_sizes_and_counts_auxiliary_usage_once() {
        let thread = ThreadHandle::start("projection".into(), DynModelSession::new(Model)).unwrap();
        thread
            .set_context_preparation(Some(ContextPreparer::new(Hook)))
            .await
            .unwrap();
        thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: vec![ContextContent::Text {
                    text: "new input".into(),
                }],
                cancellation: Default::default(),
            })
            .await
            .unwrap();
        let snapshot = thread.snapshot();
        let journal = thread.journal().await.unwrap();
        let items = project_compactions("projection", &snapshot, &journal).unwrap();
        assert_eq!(items.len(), 1);
        let ThreadItemState::ContextCompaction(item) = items[0].state() else {
            panic!("expected compaction timeline item");
        };
        assert_eq!(item.before_tokens(), None);
        assert_eq!(item.after_tokens(), None);
        let usage = super::super::runtime::project_runtime("projection", &snapshot, &journal)
            .unwrap()
            .usage;
        assert_eq!(usage.prompt_tokens, 7);
        assert_eq!(usage.completion_tokens, 2);
        assert_eq!(usage.total_tokens, 9);
        assert_eq!(usage.inference_count, 2);
        let positions = order::positions(&journal, snapshot.commit_sequence).unwrap();
        assert!(
            positions[&items[0].id].ordinal
                < positions[&order::response_id("attempt", "inference")].ordinal
        );
        thread.close().await.unwrap();
    }
}
