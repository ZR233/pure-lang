use crate::tool::{ToolDirective, ToolResult, model_visible_tool_output};

#[derive(Debug, Clone)]
pub(super) struct ToolCacheEntry {
    pub(super) tool_name: String,
    call_id: String,
    output: ToolResult,
    result_hash: String,
    total_bytes: u64,
}

pub(super) fn cache_entry(tool_name: &str, call_id: String, output: &ToolResult) -> ToolCacheEntry {
    let canonical_output = output.canonical_output();
    let total_bytes = canonical_output.len() as u64;
    let result_hash = output
        .runtime_events
        .iter()
        .find_map(|event| match event {
            ToolDirective::OutputMetrics { result_hash, .. } => Some(result_hash.clone()),
            ToolDirective::InteractionRequested { .. }
            | ToolDirective::SkillActivated { .. }
            | ToolDirective::OutputArtifacts { .. }
            | ToolDirective::RevealTools { .. }
            | ToolDirective::AuditMetadata { .. }
            | ToolDirective::ExecutionFailed
            | ToolDirective::CacheHit { .. }
            | ToolDirective::OutputBudget { .. }
            | ToolDirective::EndTurn { .. } => None,
        })
        .unwrap_or_else(|| crate::working_set::canonical_content_hash(canonical_output.as_bytes()));
    ToolCacheEntry {
        tool_name: tool_name.to_string(),
        call_id,
        output: output.clone(),
        result_hash,
        total_bytes,
    }
}

pub(super) fn cache_hit(entry: &ToolCacheEntry) -> ToolResult {
    let mut output = entry.output.clone();
    // File content may have been truncated or removed from the model context.
    // Reuse the IO result, but do not replace the requested text with a receipt.
    output.model_output = if entry.tool_name == "read_file" {
        model_visible_tool_output(&entry.output.canonical_output())
    } else {
        let summary = model_visible_tool_output(entry.output.model_output());
        let summary = summary.chars().take(512).collect::<String>();
        serde_json::json!({
            "cacheHit": true,
            "reusedFromCallId": entry.call_id,
            "resultHash": entry.result_hash,
            "totalBytes": entry.total_bytes,
            "reuseKind": "exact",
            "summary": summary,
        })
        .to_string()
    };
    // 媒体上下文只在首次成功读取时注入；缓存重放不重复插入附件。
    output.model_attachments.clear();
    let (artifact_bytes, result_hash) = output
        .runtime_events
        .iter()
        .find_map(|event| match event {
            ToolDirective::OutputMetrics {
                artifact_bytes,
                result_hash,
                ..
            } => Some((*artifact_bytes, result_hash.clone())),
            ToolDirective::InteractionRequested { .. }
            | ToolDirective::SkillActivated { .. }
            | ToolDirective::OutputArtifacts { .. }
            | ToolDirective::RevealTools { .. }
            | ToolDirective::AuditMetadata { .. }
            | ToolDirective::ExecutionFailed
            | ToolDirective::CacheHit { .. }
            | ToolDirective::OutputBudget { .. }
            | ToolDirective::EndTurn { .. } => None,
        })
        .unwrap_or((0, entry.result_hash.clone()));
    output
        .runtime_events
        .retain(|event| !matches!(event, ToolDirective::OutputMetrics { .. }));
    output.runtime_events.push(ToolDirective::CacheHit {
        reused_from_call_id: entry.call_id.clone(),
        result_hash: entry.result_hash.clone(),
        total_bytes: entry.total_bytes,
    });
    output.runtime_events.push(ToolDirective::OutputMetrics {
        raw_bytes: entry.total_bytes,
        model_visible_bytes: output.model_output.len() as u64,
        artifact_bytes,
        result_hash,
    });
    output
}
