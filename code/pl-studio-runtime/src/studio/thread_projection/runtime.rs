//! Usage and product state are decoded by their producers, never inferred from tool text.
use super::{ProjectionError, panel::PanelState};
use pl_core::thread::ThreadSnapshot;
use pl_protocol::ThreadRuntimeSnapshot;
use std::sync::Arc;

/// Explicit reconstruction: folds the shared [`PanelState`] builders over one stable snapshot.
///
/// # Errors
/// Rejects a saved model or compaction receipt that cannot be decoded.
pub(super) fn project_runtime(
    thread_id: &str,
    state: &ThreadSnapshot,
    journal: &[Arc<pl_core::thread::journal::ThreadCommit>],
) -> Result<ThreadRuntimeSnapshot, ProjectionError> {
    let updated_at = journal
        .iter()
        .rev()
        .find(|commit| commit.sequence <= state.commit_sequence)
        .map_or(0, |commit| commit.committed_at);
    PanelState::from_snapshot(state, updated_at)?.materialize(thread_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::model::{ModelError, ModelFailureKind, ModelStepOutput};
    use pl_core::thread::{AttemptOutcome, RequestAttempt, ThreadSnapshot};
    use pretty_assertions::assert_eq;

    const FLASH_MODEL: &str = "deepseek-flash";
    const PRO_MODEL: &str = "deepseek-v4-pro";
    const DEEPSEEK_CAPACITY: u64 = 1_000_000;

    fn binding(model: &str, context_window: Option<u64>) -> serde_json::Value {
        let mut binding = serde_json::json!({
            "providerInstanceId": "deepseek",
            "requestedModel": model,
            "adapter": serde_json::to_value(pl_model::provider::ProviderAdapterKind::DeepSeek)
                .expect("adapter kind serializes"),
            "protocol": serde_json::to_value(pl_model::provider::ProviderWireProtocol::Responses)
                .expect("wire protocol serializes"),
            "isolation": "deepseek::responses",
            "purpose": "turn",
        });
        if let Some(window) = context_window {
            binding["contextWindow"] = serde_json::json!(window);
        }
        binding
    }

    fn prepared_request(
        model: &str,
        context_window: Option<u64>,
    ) -> pl_core::context::OpaquePayload {
        let content = serde_json::json!({
            "binding": binding(model, context_window),
            "tools": [],
            "toolChoice": "auto",
            "parallelToolCalls": false,
            "reasoning": null,
            "temperature": null,
            "maxTokens": null,
        });
        pl_core::context::OpaquePayload::new("pl.model.prepared-request", 1, content.to_string())
            .expect("static format and version are valid")
    }

    fn output() -> ModelStepOutput {
        ModelStepOutput {
            attempt_id: "attempt".into(),
            base_context_revision: 0,
            content: Vec::new(),
            tool_calls: Vec::new(),
            private_context: None,
            usage: Default::default(),
        }
    }

    fn failed() -> AttemptOutcome {
        AttemptOutcome::Failed(Arc::new(ModelError {
            details: None,
            kind: ModelFailureKind::Unavailable,
            usage: Default::default(),
            source: None,
        }))
    }

    fn attempt(
        attempt_id: &str,
        metadata: Option<pl_core::context::OpaquePayload>,
        outcome: AttemptOutcome,
    ) -> RequestAttempt {
        RequestAttempt {
            request_metadata: metadata,
            tool_projection: None,
            turn_id: "turn".into(),
            attempt_id: attempt_id.into(),
            retry_of: None,
            input: Default::default(),
            tools: Vec::new().into(),
            outcome,
            input_estimate: None,
        }
    }

    fn snapshot(attempts: Vec<RequestAttempt>) -> ThreadSnapshot {
        ThreadSnapshot {
            attempts: attempts.into(),
            commit_sequence: 1,
            ..Default::default()
        }
    }

    fn projected_usage(state: &ThreadSnapshot) -> pl_protocol::ThreadRuntimeUsage {
        project_runtime("thread", state, &[])
            .expect("projection succeeds")
            .usage
    }

    #[test]
    fn admitted_attempt_projects_model_and_capacity_before_any_response() {
        let state = snapshot(vec![attempt(
            "first",
            Some(prepared_request(FLASH_MODEL, Some(DEEPSEEK_CAPACITY))),
            AttemptOutcome::Running,
        )]);
        let usage = projected_usage(&state);
        assert_eq!(usage.model, FLASH_MODEL);
        assert_eq!(usage.context_window, Some(DEEPSEEK_CAPACITY));
    }

    #[test]
    fn failed_attempt_keeps_its_frozen_capacity() {
        let state = snapshot(vec![attempt(
            "first",
            Some(prepared_request(FLASH_MODEL, Some(DEEPSEEK_CAPACITY))),
            failed(),
        )]);
        let usage = projected_usage(&state);
        assert_eq!(usage.model, FLASH_MODEL);
        assert_eq!(usage.context_window, Some(DEEPSEEK_CAPACITY));
    }

    #[test]
    fn newer_attempt_with_unknown_capacity_clears_the_previous_model_capacity() {
        let state = snapshot(vec![
            attempt(
                "first",
                Some(prepared_request(FLASH_MODEL, Some(DEEPSEEK_CAPACITY))),
                AttemptOutcome::Committed(output()),
            ),
            attempt(
                "second",
                Some(prepared_request(PRO_MODEL, None)),
                AttemptOutcome::Running,
            ),
        ]);
        let usage = projected_usage(&state);
        assert_eq!(usage.model, PRO_MODEL);
        assert_eq!(usage.context_window, None);
    }

    #[test]
    fn attempt_without_a_saved_binding_clears_the_previous_capacity() {
        let state = snapshot(vec![
            attempt(
                "first",
                Some(prepared_request(FLASH_MODEL, Some(DEEPSEEK_CAPACITY))),
                AttemptOutcome::Committed(output()),
            ),
            attempt("second", None, AttemptOutcome::Running),
        ]);
        let usage = projected_usage(&state);
        assert_eq!(usage.model, FLASH_MODEL);
        assert_eq!(usage.context_window, None);
    }

    #[test]
    fn legacy_binding_without_a_capacity_field_stays_unknown() {
        let state = snapshot(vec![attempt(
            "first",
            Some(prepared_request(FLASH_MODEL, None)),
            AttemptOutcome::Committed(output()),
        )]);
        let usage = projected_usage(&state);
        assert_eq!(usage.model, FLASH_MODEL);
        assert_eq!(usage.context_window, None);
    }

    #[test]
    fn auxiliary_compaction_binding_does_not_override_thread_capacity() {
        let receipt = serde_json::json!({
            "inferenceId": "studio.compaction",
            "binding": binding("deepseek-summary", Some(200_000)),
            "reasoningEffort": null,
            "contextWindow": 200_000,
            "turnId": "turn",
            "accounting": serde_json::to_value(pl_protocol::InferenceAccounting::default())
                .expect("accounting serializes"),
            "implementation": "native",
            "error": null,
        });
        let mut state = snapshot(vec![attempt(
            "first",
            Some(prepared_request(FLASH_MODEL, Some(DEEPSEEK_CAPACITY))),
            AttemptOutcome::Committed(output()),
        )]);
        state.extensions.insert(
            "compaction".into(),
            pl_core::thread::extensions::ExtensionRecord {
                revision: 1,
                payload: pl_core::context::OpaquePayload::new(
                    "pl.studio.compaction",
                    1,
                    receipt.to_string(),
                )
                .expect("static format and version are valid"),
            },
        );
        let usage = projected_usage(&state);
        assert_eq!(usage.model, FLASH_MODEL);
        assert_eq!(usage.context_window, Some(DEEPSEEK_CAPACITY));
    }
}
