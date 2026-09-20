//! Usage and product state are decoded by their producers, never inferred from tool text.
use super::ProjectionError;
use pl_core::thread::{AttemptOutcome, ThreadSnapshot, journal::ThreadCommit};
use pl_protocol::{
    CacheUsageSummary, InferenceAccounting, ThreadRuntimeSnapshot, ThreadRuntimeUsage, UsageReport,
};
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
        reasoning_tokens: 0,
        inference_count: 0,
        total_tokens: 0,
        cache_usage: CacheUsageSummary::default(),
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
                usage.cache_usage.has_incomplete_usage = true;
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
    if usage.cache_usage.input_tokens > 0 {
        usage.cache_usage.hit_rate = Some(
            usage.cache_usage.cache_read_tokens as f64 / usage.cache_usage.input_tokens as f64,
        );
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
    let child = state
        .extensions
        .contains_key(crate::studio::model_route::AGENT_PROFILE_EXTENSION);
    let model_route = crate::studio::model_route::route_record(state, child)
        .map_err(|error| ProjectionError::UnsupportedOutput(error.to_string()))?
        .map(|(record, route)| pl_protocol::ThreadModelRouteSnapshot {
            provider_id: route.provider.into_string(),
            model: route.model,
            effort: route.effort.map(|effort| effort.as_str().to_owned()),
            revision: record.revision,
            available: state.model_available,
            unavailable_reason: (!state.model_available)
                .then(|| "the selected model binding is unavailable".to_string()),
        });
    Ok(ThreadRuntimeSnapshot {
        thread_id: thread_id.into(),
        model_route,
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
    if let Some((input, read)) = valid_cache_sample(&accounting.usage) {
        target.cache_usage.input_tokens = target
            .cache_usage
            .input_tokens
            .checked_add(input)
            .ok_or(ProjectionError::Count)?;
        target.cache_usage.cache_read_tokens = target
            .cache_usage
            .cache_read_tokens
            .checked_add(read)
            .ok_or(ProjectionError::Count)?;
    } else {
        target.cache_usage.has_incomplete_usage = true;
    }
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

/// 提取一次 inference 的输入侧缓存样本。
///
/// 只有 input 与 cache read 同时报告、且 cache read（连同可选的 cache
/// write）不超过 input 时才有效。输出侧字段缺失或矛盾不影响缓存样本有效
/// 性，因为命中率只由输入侧累计得出。
fn valid_cache_sample(usage: &UsageReport) -> Option<(u64, u64)> {
    let input = usage.input_tokens?;
    let read = usage.cache_read_tokens?;
    if read > input {
        return None;
    }
    if let Some(write) = usage.cache_write_tokens {
        let billed = read.checked_add(write)?;
        if billed > input {
            return None;
        }
    }
    Some((input, read))
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
    use pl_core::model::{ModelError, ModelFailureKind, ModelStepOutput, ModelUsage};
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
        output_with_usage(ModelUsage::default())
    }

    fn output_with_usage(usage: ModelUsage) -> ModelStepOutput {
        ModelStepOutput {
            attempt_id: "attempt".into(),
            base_context_revision: 0,
            content: Vec::new(),
            tool_calls: Vec::new(),
            private_context: None,
            usage,
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

    /// 历史缺失的样本只标记不完整，后续有效样本仍能更新缓存命中率。
    ///
    /// 旧实现用总 `has_incomplete_usage` 门控命中率，任何历史缺失都会永久
    /// 阻断；本用例在那种实现下会失败。
    #[test]
    fn cache_usage_recovers_after_a_missing_history_sample() {
        let state = snapshot(vec![
            attempt(
                "missing",
                None,
                AttemptOutcome::Committed(output_with_usage(ModelUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(20),
                    ..Default::default()
                })),
            ),
            attempt(
                "reported",
                None,
                AttemptOutcome::Committed(output_with_usage(ModelUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(20),
                    cache_read_tokens: Some(90),
                    ..Default::default()
                })),
            ),
        ]);
        let usage = projected_usage(&state);
        assert!(
            usage.has_incomplete_usage,
            "the missing sample still flags the raw total"
        );
        assert!(usage.cache_usage.has_incomplete_usage);
        assert_eq!(usage.cache_usage.input_tokens, 100);
        assert_eq!(usage.cache_usage.cache_read_tokens, 90);
        assert_eq!(usage.cache_usage.hit_rate, Some(0.9));
    }

    /// 命中率只依赖输入侧字段：输出缺失不影响有效缓存样本。
    #[test]
    fn cache_usage_counts_input_samples_even_when_output_is_missing() {
        let state = snapshot(vec![attempt(
            "partial",
            None,
            AttemptOutcome::Committed(output_with_usage(ModelUsage {
                input_tokens: Some(100),
                cache_read_tokens: Some(40),
                ..Default::default()
            })),
        )]);
        let usage = projected_usage(&state);
        assert!(
            usage.has_incomplete_usage,
            "missing output still flags the total"
        );
        assert!(!usage.cache_usage.has_incomplete_usage);
        assert_eq!(usage.cache_usage.input_tokens, 100);
        assert_eq!(usage.cache_usage.cache_read_tokens, 40);
        assert_eq!(usage.cache_usage.hit_rate, Some(0.4));
    }

    /// 零输入零读取是有效样本，但累计分母为零时命中率未知而不是零。
    #[test]
    fn cache_usage_reports_an_unknown_rate_when_the_denominator_is_zero() {
        let state = snapshot(vec![attempt(
            "empty",
            None,
            AttemptOutcome::Committed(output_with_usage(ModelUsage {
                input_tokens: Some(0),
                cache_read_tokens: Some(0),
                ..Default::default()
            })),
        )]);
        let usage = projected_usage(&state);
        assert!(!usage.cache_usage.has_incomplete_usage);
        assert_eq!(usage.cache_usage.input_tokens, 0);
        assert_eq!(usage.cache_usage.hit_rate, None);
    }

    /// 正分母下的真实零命中报告为 `Some(0.0)`，不伪装成未知。
    #[test]
    fn cache_usage_reports_a_real_zero_hit_as_zero() {
        let state = snapshot(vec![attempt(
            "reported",
            None,
            AttemptOutcome::Committed(output_with_usage(ModelUsage {
                input_tokens: Some(100),
                output_tokens: Some(5),
                cache_read_tokens: Some(0),
                ..Default::default()
            })),
        )]);
        let usage = projected_usage(&state);
        assert_eq!(usage.cache_usage.hit_rate, Some(0.0));
    }

    /// 分母分子来自全部有效样本，而不是最后一次样本。
    #[test]
    fn cache_usage_matches_accumulated_numerator_and_denominator() {
        let state = snapshot(vec![
            attempt(
                "first",
                None,
                AttemptOutcome::Committed(output_with_usage(ModelUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(5),
                    cache_read_tokens: Some(40),
                    ..Default::default()
                })),
            ),
            attempt(
                "second",
                None,
                AttemptOutcome::Committed(output_with_usage(ModelUsage {
                    input_tokens: Some(300),
                    output_tokens: Some(5),
                    cache_read_tokens: Some(260),
                    ..Default::default()
                })),
            ),
        ]);
        let usage = projected_usage(&state);
        assert_eq!(usage.cache_usage.input_tokens, 400);
        assert_eq!(usage.cache_usage.cache_read_tokens, 300);
        assert_eq!(usage.cache_usage.hit_rate, Some(0.75));
    }

    /// 矛盾的输入侧样本被排除在累计之外，并标记不完整。
    #[test]
    fn cache_usage_excludes_contradictory_samples() {
        for (read, write) in [(120, None), (60, Some(60))] {
            let state = snapshot(vec![attempt(
                "contradiction",
                None,
                AttemptOutcome::Committed(output_with_usage(ModelUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(5),
                    cache_read_tokens: Some(read),
                    cache_write_tokens: write,
                    ..Default::default()
                })),
            )]);
            let usage = projected_usage(&state);
            assert!(
                usage.cache_usage.has_incomplete_usage,
                "read {read} write {write:?}"
            );
            assert_eq!(usage.cache_usage.input_tokens, 0);
            assert_eq!(usage.cache_usage.hit_rate, None);
        }
    }

    /// 累计溢出以类型化错误失败，不截断或回绕比例。
    #[test]
    fn cache_usage_overflow_fails_instead_of_truncating() {
        let state = snapshot(vec![
            attempt(
                "max",
                None,
                AttemptOutcome::Committed(output_with_usage(ModelUsage {
                    input_tokens: Some(u64::MAX),
                    cache_read_tokens: Some(0),
                    ..Default::default()
                })),
            ),
            attempt(
                "one",
                None,
                AttemptOutcome::Committed(output_with_usage(ModelUsage {
                    input_tokens: Some(1),
                    cache_read_tokens: Some(0),
                    ..Default::default()
                })),
            ),
        ]);
        assert!(matches!(
            project_runtime("thread", &state, &[]),
            Err(super::ProjectionError::Count)
        ));
    }

    /// Running 尝试不计数也不污染缓存样本完整性。
    #[test]
    fn running_attempt_neither_counts_nor_marks_cache_incomplete() {
        let state = snapshot(vec![
            attempt(
                "done",
                None,
                AttemptOutcome::Committed(output_with_usage(ModelUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(5),
                    cache_read_tokens: Some(50),
                    ..Default::default()
                })),
            ),
            attempt("running", None, AttemptOutcome::Running),
        ]);
        let usage = projected_usage(&state);
        assert!(!usage.cache_usage.has_incomplete_usage);
        assert_eq!(usage.cache_usage.input_tokens, 100);
        assert_eq!(usage.cache_usage.hit_rate, Some(0.5));
    }

    /// compaction 回执按原路径各纳入一次，重投影结果稳定不翻倍。
    #[test]
    fn cache_usage_includes_compaction_once_and_stays_stable_across_reprojection() {
        let mut state = snapshot(vec![attempt(
            "turn",
            None,
            AttemptOutcome::Committed(output_with_usage(ModelUsage {
                input_tokens: Some(100),
                output_tokens: Some(10),
                cache_read_tokens: Some(50),
                ..Default::default()
            })),
        )]);
        let receipt = serde_json::json!({
            "inferenceId": "studio.compaction",
            "binding": binding(FLASH_MODEL, None),
            "reasoningEffort": null,
            "contextWindow": null,
            "turnId": "turn",
            "accounting": serde_json::to_value(InferenceAccounting {
                usage: UsageReport {
                    input_tokens: Some(200),
                    output_tokens: Some(20),
                    cache_read_tokens: Some(150),
                    ..Default::default()
                },
                ..Default::default()
            })
            .expect("accounting serializes"),
            "implementation": "native",
            "error": null,
        });
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
        let first = projected_usage(&state);
        let second = projected_usage(&state);
        assert_eq!(first, second);
        assert_eq!(first.cache_usage.input_tokens, 300);
        assert_eq!(first.cache_usage.cache_read_tokens, 200);
        assert_eq!(first.cache_usage.hit_rate, Some(200.0 / 300.0));
    }
}
