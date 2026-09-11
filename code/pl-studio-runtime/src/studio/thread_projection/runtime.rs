//! Usage and product state are decoded by their producers, never inferred from tool text.
use super::ProjectionError;
use pl_core::thread::{AttemptOutcome, ThreadSnapshot, journal::ThreadCommit};
use pl_protocol::{InferenceAccounting, ThreadRuntimeSnapshot, ThreadRuntimeUsage};
use std::sync::Arc;

pub(super) fn project_runtime(
    thread_id: &str,
    state: &ThreadSnapshot,
    journal: &[Arc<ThreadCommit>],
) -> Result<ThreadRuntimeSnapshot, ProjectionError> {
    let updated_at = journal
        .iter()
        .rev()
        .find(|commit| commit.sequence <= state.commit_sequence)
        .map_or(0, |commit| commit.committed_at);
    let mut usage = ThreadRuntimeUsage {
        has_incomplete_usage: false,
        model: String::new(),
        context_window: None,
        latest_context_tokens: 0,
        prompt_tokens: 0,
        completion_tokens: 0,
        cached_prompt_tokens: 0,
        cache_write_tokens: 0,
        cache_miss_tokens: 0,
        reasoning_tokens: 0,
        inference_count: 0,
        total_tokens: 0,
        cache_hit_rate: None,
        estimated_costs: Vec::new(),
        estimated_cache_savings: Vec::new(),
        has_unpriced_usage: false,
        prompt_generation: None,
        prompt_cache_policy: None,
        prefix_changed_reason: None,
        updated_at,
    };
    let mut turn_completion_tokens: u64 = 0;
    let mut turn_decode_millis: u64 = 0;
    for attempt in state.attempts.iter() {
        // Model identity and capacity come from the newest attempt's admission-time binding, so
        // they are projected before any response exists. An attempt that saved no capacity clears
        // the previous model's capacity instead of reusing a value from an older model.
        match attempt.request_metadata.as_ref() {
            Some(metadata) => {
                let request = pl_model::runtime::model_request_receipt(metadata)?;
                usage.model = request.binding.requested_model;
                usage.context_window = request.binding.context_window;
            }
            None => usage.context_window = None,
        }
        let output = match &attempt.outcome {
            AttemptOutcome::Committed(output) | AttemptOutcome::Rejected { output, .. } => {
                Some(Ok(output))
            }
            AttemptOutcome::Failed(error) => Some(Err(error.as_ref())),
            AttemptOutcome::Cancelled { result } => Some(result.as_ref().map_err(Arc::as_ref)),
            AttemptOutcome::Running => None,
            AttemptOutcome::Interrupted => {
                usage.has_incomplete_usage = true;
                usage.has_unpriced_usage = true;
                None
            }
        };
        let Some(output) = output else {
            continue;
        };
        let accounting = match output {
            Ok(output) => match pl_model::runtime::model_response_receipt(output)? {
                Some(receipt) => {
                    if state
                        .turns
                        .last()
                        .is_some_and(|turn| turn.turn_id == attempt.turn_id)
                        && let Some(timing) = receipt.response.timing
                    {
                        turn_completion_tokens = turn_completion_tokens
                            .checked_add(
                                receipt.response.accounting.usage.output_tokens.unwrap_or(0),
                            )
                            .ok_or(ProjectionError::Count)?;
                        turn_decode_millis = turn_decode_millis
                            .checked_add(timing.decode_millis)
                            .ok_or(ProjectionError::Count)?;
                    }
                    receipt.response.accounting
                }
                None => unknown_accounting(&output.usage),
            },
            Err(error) => pl_model::runtime::model_failure_receipt(error)?.map_or_else(
                || unknown_accounting(&error.usage),
                |receipt| receipt.accounting,
            ),
        };
        add_usage(&mut usage, &accounting)?;
    }
    let latest_context_tokens = usage.latest_context_tokens;
    for record in state.extensions.values() {
        if let Some(receipt) = super::compactions::receipt(&record.payload)? {
            add_usage(&mut usage, &receipt.accounting)?;
        }
    }
    usage.latest_context_tokens = latest_context_tokens;
    if !usage.has_incomplete_usage && usage.prompt_tokens > 0 {
        usage.cache_hit_rate = Some(usage.cached_prompt_tokens as f64 / usage.prompt_tokens as f64);
    }
    let todo = state
        .extensions
        .get("pl.tool.todo")
        .map(|record| {
            let call_id = state
                .deliveries
                .iter()
                .rev()
                .find(|delivery| delivery.output.payload() == &record.payload)
                .map_or_else(String::new, |delivery| delivery.call_id.clone());
            pl_tool::todo::saved_snapshot(&record.payload, call_id)
        })
        .transpose()?;
    let workflow = state
        .extensions
        .get(crate::workflow_tool::WORKFLOW_EXTENSION)
        .map(|record| {
            crate::workflow_tool::decode_workflow_state(&record.payload)
                .map(|state| pl_protocol::WorkflowRuntimeSnapshot::from(&state))
        })
        .transpose()?;
    let active_skills = state
        .extensions
        .values()
        .filter(|record| record.payload.format() == "pl.tool.skill-view")
        .map(|record| pl_tool::skill::saved_skill_name(&record.payload))
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?
        .into_iter()
        .collect();
    Ok(ThreadRuntimeSnapshot {
        thread_id: thread_id.into(),
        usage,
        turn_completion_tokens,
        turn_decode_millis,
        todo,
        workflow,
        active_skills,
        active_mcp_servers: Vec::new(),
        active_lsp_servers: Vec::new(),
        progress: None,
        mcp_health: None,
        updated_at,
    })
}

fn unknown_accounting(usage: &pl_core::model::ModelUsage) -> InferenceAccounting {
    InferenceAccounting {
        usage: pl_protocol::UsageReport {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            cache_write_tokens: usage.cache_write_tokens,
            reasoning_tokens: usage.reasoning_tokens,
            total_tokens: usage
                .input_tokens
                .zip(usage.output_tokens)
                .and_then(|(a, b)| a.checked_add(b)),
        },
        ..Default::default()
    }
}

fn add_usage(
    target: &mut ThreadRuntimeUsage,
    accounting: &InferenceAccounting,
) -> Result<(), ProjectionError> {
    let totals = accounting.usage.totals();
    target.has_incomplete_usage |= accounting.has_incomplete_usage();
    target.has_unpriced_usage |= accounting.has_unpriced_usage();
    target.inference_count = target
        .inference_count
        .checked_add(1)
        .ok_or(ProjectionError::Count)?;
    for (current, value) in [
        (&mut target.prompt_tokens, totals.prompt_tokens),
        (&mut target.completion_tokens, totals.completion_tokens),
        (
            &mut target.cached_prompt_tokens,
            totals.cached_prompt_tokens,
        ),
        (&mut target.cache_write_tokens, totals.cache_write_tokens),
        (&mut target.reasoning_tokens, totals.reasoning_tokens),
        (&mut target.total_tokens, totals.total_tokens),
    ] {
        *current = current.checked_add(value).ok_or(ProjectionError::Count)?;
    }
    target.cache_miss_tokens = target
        .prompt_tokens
        .saturating_sub(target.cached_prompt_tokens);
    if let Some(tokens) = accounting.usage.known_total_tokens() {
        target.latest_context_tokens = tokens;
    }
    merge_costs(&mut target.estimated_costs, &accounting.estimated_costs());
    merge_costs(
        &mut target.estimated_cache_savings,
        &accounting.estimated_cache_savings(),
    );
    Ok(())
}

fn merge_costs(
    target: &mut Vec<pl_protocol::RuntimeCostAmount>,
    incoming: &[pl_protocol::RuntimeCostAmount],
) {
    for cost in incoming {
        if let Some(existing) = target
            .iter_mut()
            .find(|existing| existing.currency == cost.currency)
        {
            existing.amount += cost.amount;
        } else {
            target.push(cost.clone());
        }
    }
    target.sort_by(|left, right| left.currency.cmp(&right.currency));
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

    fn projected_usage(state: &ThreadSnapshot) -> ThreadRuntimeUsage {
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
