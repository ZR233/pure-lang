use pl_protocol::ToolResultReceipt;

use super::super::tool_dispatch::ToolExecutionRecord;
use crate::working_set::canonical_content_hash;

pub(super) fn apply_batch_budget(
    tool_results: &mut [ToolExecutionRecord],
    remaining_context_tokens: Option<u64>,
) {
    if tool_results.len() <= 1 {
        return;
    }
    let token_budget = crate::tool::model_tool_output_batch_token_budget(
        tool_results.len(),
        remaining_context_tokens,
    );
    let original_results = tool_results
        .iter()
        .map(|tool_result| tool_result.result.clone())
        .collect::<Vec<_>>();
    let projected_results =
        crate::tool::model_visible_tool_output_batch_with_tokens(&original_results, token_budget);
    let original_bytes = original_results.iter().map(String::len).sum::<usize>();
    let projected_bytes = projected_results.iter().map(String::len).sum::<usize>();

    for ((tool_result, original_result), projected_result) in tool_results
        .iter_mut()
        .zip(original_results)
        .zip(projected_results)
    {
        if projected_result == original_result {
            continue;
        }
        let visible_bytes = projected_result.len() as u64;
        let mut metrics_updated = false;
        for event in &mut tool_result.runtime_events {
            if let crate::tool::ToolRuntimeEvent::OutputMetrics {
                model_visible_bytes,
                ..
            } = event
            {
                *model_visible_bytes = visible_bytes;
                metrics_updated = true;
                break;
            }
        }
        if !metrics_updated {
            tool_result
                .runtime_events
                .push(crate::tool::ToolRuntimeEvent::OutputMetrics {
                    raw_bytes: original_result.len() as u64,
                    model_visible_bytes: visible_bytes,
                    artifact_bytes: 0,
                    result_hash: canonical_content_hash(original_result.as_bytes()),
                });
        }
        tool_result.result = projected_result;
    }

    if projected_bytes < original_bytes {
        tracing::info!(
            target: "pl_core::tool_metrics",
            tool_count = tool_results.len(),
            token_budget,
            original_bytes,
            projected_bytes,
            "applied model-visible tool output batch budget"
        );
    }
}

pub(super) fn normalize_programmatic_results(
    tool_results: &mut [ToolExecutionRecord],
    tool_calls: &[pl_model::ToolCall],
) {
    for (tool_result, tool_call) in tool_results.iter_mut().zip(tool_calls) {
        if tool_call.caller.is_none() {
            continue;
        }
        tool_result.result = match serde_json::from_str::<serde_json::Value>(&tool_result.result) {
            Ok(serde_json::Value::Object(_)) => tool_result.result.clone(),
            Ok(value) => serde_json::json!({ "content": value }).to_string(),
            Err(_) => serde_json::json!({ "content": tool_result.result }).to_string(),
        };
    }
}

pub(super) fn receipt(result: &ToolExecutionRecord) -> ToolResultReceipt {
    let artifacts = result
        .runtime_events
        .iter()
        .filter_map(|event| match event {
            crate::tool::ToolRuntimeEvent::OutputArtifacts { artifacts } => {
                Some(artifacts.as_slice())
            }
            crate::tool::ToolRuntimeEvent::InteractionRequested { .. }
            | crate::tool::ToolRuntimeEvent::SkillActivated { .. }
            | crate::tool::ToolRuntimeEvent::ToolResultRevision { .. }
            | crate::tool::ToolRuntimeEvent::AuditMetadata { .. }
            | crate::tool::ToolRuntimeEvent::ExecutionFailed
            | crate::tool::ToolRuntimeEvent::CacheHit { .. }
            | crate::tool::ToolRuntimeEvent::OutputMetrics { .. }
            | crate::tool::ToolRuntimeEvent::OutputBudget { .. }
            | crate::tool::ToolRuntimeEvent::EndTurn { .. } => None,
        })
        .flatten()
        .map(compact_artifact_reference)
        .collect::<Vec<_>>();
    let cache_hit = result.runtime_events.iter().find_map(|event| match event {
        crate::tool::ToolRuntimeEvent::CacheHit {
            reused_from_call_id,
            result_hash,
            total_bytes,
        } => Some((reused_from_call_id, result_hash, *total_bytes)),
        crate::tool::ToolRuntimeEvent::InteractionRequested { .. }
        | crate::tool::ToolRuntimeEvent::SkillActivated { .. }
        | crate::tool::ToolRuntimeEvent::ToolResultRevision { .. }
        | crate::tool::ToolRuntimeEvent::OutputArtifacts { .. }
        | crate::tool::ToolRuntimeEvent::AuditMetadata { .. }
        | crate::tool::ToolRuntimeEvent::ExecutionFailed
        | crate::tool::ToolRuntimeEvent::OutputMetrics { .. }
        | crate::tool::ToolRuntimeEvent::OutputBudget { .. }
        | crate::tool::ToolRuntimeEvent::EndTurn { .. } => None,
    });
    let metrics = result.runtime_events.iter().find_map(|event| match event {
        crate::tool::ToolRuntimeEvent::OutputMetrics {
            raw_bytes,
            model_visible_bytes,
            artifact_bytes: _,
            result_hash,
        } => Some((*raw_bytes, *model_visible_bytes, result_hash)),
        crate::tool::ToolRuntimeEvent::InteractionRequested { .. }
        | crate::tool::ToolRuntimeEvent::SkillActivated { .. }
        | crate::tool::ToolRuntimeEvent::ToolResultRevision { .. }
        | crate::tool::ToolRuntimeEvent::OutputArtifacts { .. }
        | crate::tool::ToolRuntimeEvent::AuditMetadata { .. }
        | crate::tool::ToolRuntimeEvent::ExecutionFailed
        | crate::tool::ToolRuntimeEvent::CacheHit { .. }
        | crate::tool::ToolRuntimeEvent::OutputBudget { .. }
        | crate::tool::ToolRuntimeEvent::EndTurn { .. } => None,
    });
    ToolResultReceipt {
        call_id: result.call_id.clone(),
        tool_name: result.name.clone(),
        arguments_hash: serde_json::from_str(&result.arguments).map_or_else(
            |_| canonical_content_hash(result.arguments.as_bytes()),
            |value| crate::canonical_json_hash(&value),
        ),
        result_hash: cache_hit.map_or_else(
            || {
                metrics.map_or_else(
                    || canonical_content_hash(result.result.as_bytes()),
                    |(_, _, hash)| hash.clone(),
                )
            },
            |(_, hash, _)| hash.clone(),
        ),
        total_bytes: cache_hit.map_or_else(
            || metrics.map_or(result.result.len() as u64, |(raw, _, _)| raw),
            |(_, _, bytes)| bytes,
        ),
        visible_bytes: metrics.map_or(result.result.len() as u64, |(_, visible, _)| visible),
        truncated: cache_hit.is_some()
            || metrics.is_some_and(|(raw, visible, _)| raw > visible)
            || result.result.len() >= crate::tool::MAX_MODEL_TOOL_OUTPUT_BYTES,
        artifacts,
        continuation: continuation(&result.result),
        reused_from_call_id: cache_hit.map(|(call_id, _, _)| call_id.clone()),
    }
}

fn compact_artifact_reference(artifact: &serde_json::Value) -> serde_json::Value {
    const REFERENCE_FIELDS: [&str; 20] = [
        "artifactId",
        "artifact_id",
        "callId",
        "call_id",
        "contentHash",
        "content_hash",
        "id",
        "kind",
        "mediaType",
        "media_type",
        "mimeType",
        "mime_type",
        "name",
        "path",
        "sha256",
        "size",
        "sizeBytes",
        "size_bytes",
        "stream",
        "uri",
    ];
    let serialized = serde_json::to_vec(artifact).unwrap_or_default();
    let mut reference = serde_json::Map::new();
    if let Some(object) = artifact.as_object() {
        for field in REFERENCE_FIELDS {
            if let Some(value) = object.get(field)
                && matches!(
                    value,
                    serde_json::Value::Null
                        | serde_json::Value::Bool(_)
                        | serde_json::Value::Number(_)
                        | serde_json::Value::String(_)
                )
            {
                reference.insert(field.to_string(), value.clone());
            }
        }
    }
    reference.insert(
        "receiptHash".to_string(),
        serde_json::Value::String(canonical_content_hash(&serialized)),
    );
    reference.insert(
        "receiptBytes".to_string(),
        serde_json::Value::from(serialized.len() as u64),
    );
    serde_json::Value::Object(reference)
}

fn continuation(output: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(output).ok()?;
    ["nextStartLine", "nextStartByte", "nextCursor", "nextOffset"]
        .into_iter()
        .find_map(|key| {
            value
                .get(key)
                .filter(|value| !value.is_null())
                .map(|value| serde_json::json!({ "field": key, "value": value }).to_string())
        })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn tool_result(id: &str, result: String) -> ToolExecutionRecord {
        ToolExecutionRecord {
            id: id.to_string(),
            call_id: format!("call-{id}"),
            name: "read_file".to_string(),
            kind: pl_protocol::ToolCallKind::Function,
            display_result: result.clone(),
            result,
            arguments: "{}".to_string(),
            outcome: crate::core::tool_dispatch::ToolExecutionOutcome::Succeeded,
            exit_code: Some(0),
            timed_out: false,
            runtime_events: Vec::new(),
            execution_millis: 0,
        }
    }

    #[test]
    fn artifact_receipt_keeps_identity_but_not_large_payload() {
        let artifact = serde_json::json!({
            "kind": "webSearch",
            "id": "artifact-1",
            "results": "x".repeat(64 * 1024),
        });

        let reference = compact_artifact_reference(&artifact);

        assert_eq!(reference["kind"], "webSearch");
        assert_eq!(reference["id"], "artifact-1");
        assert_eq!(reference.get("results"), None);
        assert!(reference["receiptBytes"].as_u64().unwrap() > 64 * 1024);
        assert!(
            reference["receiptHash"]
                .as_str()
                .unwrap()
                .starts_with("sha256:")
        );
    }

    #[test]
    fn audit_metadata_does_not_become_an_artifact_receipt() {
        let mut result = tool_result("mcp", r#"{"answer":42}"#.to_string());
        result
            .runtime_events
            .push(crate::tool::ToolRuntimeEvent::AuditMetadata {
                metadata: serde_json::json!({
                    "kind": "mcpCallToolResult",
                    "result": { "structuredContent": { "answer": 42 } },
                }),
            });

        assert!(receipt(&result).artifacts.is_empty());
    }

    #[test]
    fn batch_budget_updates_receipts_without_changing_display_results() {
        let first_result = "a".repeat(2_000);
        let second_result = "b".repeat(2_000);
        let mut results = vec![
            tool_result("first", first_result.clone()),
            tool_result("second", second_result.clone()),
        ];
        results[0]
            .runtime_events
            .push(crate::tool::ToolRuntimeEvent::OutputMetrics {
                raw_bytes: 5_000,
                model_visible_bytes: first_result.len() as u64,
                artifact_bytes: 7,
                result_hash: "sha256:original".to_string(),
            });

        apply_batch_budget(&mut results, Some(100));

        assert_eq!(results[0].display_result, first_result);
        assert_eq!(results[1].display_result, second_result);
        assert!(
            results
                .iter()
                .map(|result| result.result.len())
                .sum::<usize>()
                <= 256 * 4
        );

        let first_receipt = receipt(&results[0]);
        assert_eq!(first_receipt.total_bytes, 5_000);
        assert_eq!(first_receipt.result_hash, "sha256:original");
        assert_eq!(first_receipt.visible_bytes, results[0].result.len() as u64);
        assert!(first_receipt.truncated);

        let second_receipt = receipt(&results[1]);
        assert_eq!(second_receipt.total_bytes, 2_000);
        assert_eq!(second_receipt.visible_bytes, results[1].result.len() as u64);
        assert!(second_receipt.truncated);
    }

    #[test]
    fn programmatic_tool_results_are_json_objects() {
        let mut results = vec![
            tool_result("programmatic", "plain text".to_string()),
            tool_result("direct", "plain text".to_string()),
        ];
        let calls = vec![
            pl_model::ToolCall::function(
                "programmatic",
                "read_file",
                serde_json::json!({}),
                "call-programmatic",
            )
            .with_caller(Some(pl_protocol::ToolCallCaller::Program {
                caller_id: "program-1".to_string(),
            })),
            pl_model::ToolCall::function(
                "direct",
                "read_file",
                serde_json::json!({}),
                "call-direct",
            ),
        ];

        normalize_programmatic_results(&mut results, &calls);

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&results[0].result).unwrap(),
            serde_json::json!({"content": "plain text"})
        );
        assert_eq!(results[1].result, "plain text");
    }
}
