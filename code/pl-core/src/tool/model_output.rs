use serde_json::{Value, json};

pub(crate) const TOKEN_ESTIMATE_BYTES: usize = 4;

pub const DEFAULT_MODEL_TOOL_OUTPUT_TOKENS: usize = 3_000;
pub const DEFAULT_MODEL_TOOL_OUTPUT_BATCH_TOKENS: usize = DEFAULT_MODEL_TOOL_OUTPUT_TOKENS * 2;
pub const MIN_MODEL_TOOL_OUTPUT_BATCH_TOKENS: usize = 128;
pub const MAX_MODEL_TOOL_OUTPUT_BYTES: usize = 12 * 1024;

pub fn model_visible_tool_output(output: &str) -> String {
    model_visible_tool_output_with_tokens(output, DEFAULT_MODEL_TOOL_OUTPUT_TOKENS)
}

pub fn model_visible_tool_output_with_tokens(output: &str, max_output_tokens: usize) -> String {
    let max_bytes = max_output_tokens
        .saturating_mul(TOKEN_ESTIMATE_BYTES)
        .clamp(1, MAX_MODEL_TOOL_OUTPUT_BYTES);
    project_model_visible_tool_output(output, max_bytes, MAX_MODEL_TOOL_OUTPUT_BYTES)
}

pub fn model_visible_tool_output_with_bytes(output: &str, max_output_bytes: usize) -> String {
    if max_output_bytes == 0 {
        return String::new();
    }
    let max_bytes = max_output_bytes.min(MAX_MODEL_TOOL_OUTPUT_BYTES);
    project_model_visible_tool_output(output, max_bytes, max_bytes)
}

fn project_model_visible_tool_output(
    output: &str,
    projection_bytes: usize,
    enforced_bytes: usize,
) -> String {
    if output.len() <= projection_bytes {
        return output.to_string();
    }
    let projected = if let Ok(value) = serde_json::from_str::<Value>(output) {
        bounded_json_tool_output(value, projection_bytes).to_string()
    } else {
        let (text, truncated, bytes_omitted, next_offset) =
            bounded_text(output, projection_bytes, 0);
        json!({
            "truncated": truncated,
            "bytesReturned": text.len(),
            "bytesOmitted": bytes_omitted,
            "nextOffset": next_offset,
            "text": text,
        })
        .to_string()
    };
    enforce_model_output_limit_with_cap(&projected, enforced_bytes)
}

/// 为同一模型响应产生的工具结果分配统一 token 预算。
///
/// 单结果沿用既有 3,000 token 上限。多结果默认合计不超过 6,000 token；
/// 若已知压缩阈值剩余空间，则最多使用剩余量的四分之一，同时尽量为每项
/// 保留 128 token 的最小可诊断份额。
pub fn model_tool_output_batch_token_budget(
    output_count: usize,
    remaining_context_tokens: Option<u64>,
) -> usize {
    if output_count <= 1 {
        return DEFAULT_MODEL_TOOL_OUTPUT_TOKENS;
    }
    let maximum = DEFAULT_MODEL_TOOL_OUTPUT_BATCH_TOKENS;
    let fair_floor = MIN_MODEL_TOOL_OUTPUT_BATCH_TOKENS
        .saturating_mul(output_count)
        .min(maximum);
    let contextual_budget = remaining_context_tokens.map_or(maximum, |remaining| {
        usize::try_from(remaining / 4).unwrap_or(usize::MAX)
    });
    contextual_budget.clamp(fair_floor, maximum)
}

/// 在固定总预算内投影一批模型可见工具结果，保持输入顺序和结果数量。
pub fn model_visible_tool_output_batch_with_tokens(
    outputs: &[String],
    max_total_tokens: usize,
) -> Vec<String> {
    if outputs.is_empty() {
        return Vec::new();
    }
    let max_total_bytes = max_total_tokens.saturating_mul(TOKEN_ESTIMATE_BYTES);
    let allocations = fair_output_byte_allocations(outputs, max_total_bytes);
    outputs
        .iter()
        .zip(allocations)
        .map(|(output, max_bytes)| model_visible_tool_output_with_bytes(output, max_bytes))
        .collect()
}

fn fair_output_byte_allocations(outputs: &[String], max_total_bytes: usize) -> Vec<usize> {
    let output_count = outputs.len();
    let total_bytes = outputs
        .iter()
        .map(String::len)
        .fold(0_usize, usize::saturating_add);
    if total_bytes <= max_total_bytes {
        return outputs.iter().map(String::len).collect();
    }

    let fair_share_bytes = MIN_MODEL_TOOL_OUTPUT_BATCH_TOKENS * TOKEN_ESTIMATE_BYTES;
    let initial_share = fair_share_bytes.min(max_total_bytes / output_count);
    let mut allocations = outputs
        .iter()
        .map(|output| output.len().min(initial_share))
        .collect::<Vec<_>>();
    let mut remaining = max_total_bytes.saturating_sub(allocations.iter().sum::<usize>());

    while remaining > 0 {
        let active = outputs
            .iter()
            .zip(&allocations)
            .filter(|(output, allocated)| output.len() > **allocated)
            .count();
        if active == 0 {
            break;
        }
        let share = remaining.div_ceil(active).max(1);
        let mut distributed = 0_usize;
        for (output, allocated) in outputs.iter().zip(&mut allocations) {
            if remaining == 0 {
                break;
            }
            let addition = output
                .len()
                .saturating_sub(*allocated)
                .min(share)
                .min(remaining);
            *allocated = allocated.saturating_add(addition);
            remaining -= addition;
            distributed = distributed.saturating_add(addition);
        }
        if distributed == 0 {
            break;
        }
    }

    allocations
}

/// 对所有工具输出执行最终字节预算，任何工具或产品 adapter 都不能绕过。
///
/// 硬上限被夹紧到 [`MAX_MODEL_TOOL_OUTPUT_BYTES`] 安全阈值。需要更大预算的只读
/// 概览工具（如 `task_status`、`read_agent_submissions`、`read_review_round`）
/// 应改用 [`enforce_model_output_limit_with_cap`]。
pub fn enforce_model_output_limit(output: &str, requested_max_bytes: usize) -> String {
    enforce_model_output_limit_inner(
        output,
        requested_max_bytes.clamp(1, MAX_MODEL_TOOL_OUTPUT_BYTES),
    )
}

/// 与 [`enforce_model_output_limit`] 相同的投影逻辑，但允许调用方显式越过
/// [`MAX_MODEL_TOOL_OUTPUT_BYTES`] 默认安全阈值。
///
/// 仅用于已通过分页或结构化概览控制总体体积、且业务上必须完整返回的只读工具。
pub fn enforce_model_output_limit_with_cap(output: &str, max_bytes: usize) -> String {
    enforce_model_output_limit_inner(output, max_bytes.max(1))
}

fn enforce_model_output_limit_inner(output: &str, max_bytes: usize) -> String {
    if output.len() <= max_bytes {
        return output.to_string();
    }
    if serde_json::from_str::<Value>(output).is_ok() {
        let mut preview_bytes = max_bytes.saturating_sub(192).max(1);
        loop {
            let head_budget = preview_bytes / 3;
            let tail_budget = preview_bytes - head_budget;
            let preview = format!(
                "{}\n… truncated …\n{}",
                utf8_prefix(output, head_budget),
                utf8_suffix(output, tail_budget)
            );
            let candidate = json!({
                "truncated": true,
                "bytesReturned": preview.len(),
                "bytesOmitted": output.len().saturating_sub(preview.len()),
                "nextOffset": preview.len(),
                "jsonPreview": preview,
            })
            .to_string();
            if candidate.len() <= max_bytes {
                return candidate;
            }
            if preview_bytes <= 1 {
                return utf8_prefix("{}", max_bytes).to_string();
            }
            preview_bytes = (preview_bytes / 2).max(1);
        }
    }
    const MARKER: &str = "\n… output truncated by pl-core …\n";
    if max_bytes <= MARKER.len() {
        return utf8_prefix(MARKER, max_bytes).to_string();
    }
    let available = max_bytes - MARKER.len();
    let head_budget = available / 3;
    let tail_budget = available - head_budget;
    let head = utf8_prefix(output, head_budget);
    let tail = utf8_suffix(output, tail_budget);
    format!("{head}{MARKER}{tail}")
}

/// 为需要完整返回的只读工具同时指定软 token 预算与硬字节上限。
///
/// 软预算控制常规投影体积；硬上限允许越过 [`MAX_MODEL_TOOL_OUTPUT_BYTES`]
/// 但仍保证最终输出有界。调用方应同时提供分页，避免单次返回过大。
pub fn model_visible_tool_output_with_budget(
    output: &str,
    max_output_tokens: usize,
    max_output_bytes: usize,
) -> String {
    let projection_bytes = max_output_tokens
        .saturating_mul(TOKEN_ESTIMATE_BYTES)
        .clamp(1, max_output_bytes);
    project_model_visible_tool_output(output, projection_bytes, max_output_bytes)
}

fn bounded_json_tool_output(mut value: Value, max_bytes: usize) -> Value {
    match &mut value {
        Value::Object(map) => {
            for key in [
                "stdout",
                "stderr",
                "body",
                "text",
                "tarBase64",
                "contentBase64",
            ] {
                if let Some(Value::String(text)) = map.get_mut(key) {
                    let (bounded, truncated, bytes_omitted, next_offset) =
                        bounded_text(text, max_bytes, 0);
                    if truncated {
                        let bytes_returned = bounded.len();
                        *text = bounded;
                        map.insert("truncated".to_string(), Value::Bool(true));
                        map.insert("bytesReturned".to_string(), json!(bytes_returned));
                        map.insert("bytesOmitted".to_string(), json!(bytes_omitted));
                        map.insert("nextOffset".to_string(), json!(next_offset));
                        break;
                    }
                }
            }
            if serde_json::to_string(&*map).map_or(0, |value| value.len()) > max_bytes {
                let array_count = map.values().filter(|value| value.is_array()).count().max(1);
                let array_budget = max_bytes.saturating_sub(256) / array_count;
                for value in map.values_mut() {
                    let Value::Array(items) = value else {
                        continue;
                    };
                    if serde_json::to_string(&*items).map_or(0, |value| value.len()) > array_budget
                    {
                        *value = bounded_json_array(std::mem::take(items), array_budget);
                    }
                }
            }
            if value.to_string().len() > max_bytes {
                json_preview(value, max_bytes)
            } else {
                value
            }
        }
        Value::Array(items) => bounded_json_array(std::mem::take(items), max_bytes),
        Value::String(_) | Value::Bool(_) | Value::Number(_) | Value::Null => {
            let serialized = value.to_string();
            if serialized.len() <= max_bytes {
                value
            } else {
                json_preview(Value::String(serialized), max_bytes)
            }
        }
    }
}

fn bounded_json_array(items: Vec<Value>, max_bytes: usize) -> Value {
    let total = items.len();
    let item_budget = max_bytes.saturating_sub(192).max(1);
    let mut retained = Vec::new();
    let mut used = 0usize;
    for item in items {
        let mut serialized = item.to_string();
        let item = if serialized.len() > item_budget {
            let preview = json_preview(item, item_budget);
            serialized = preview.to_string();
            preview
        } else {
            item
        };
        if !retained.is_empty() && used.saturating_add(serialized.len()) > item_budget {
            break;
        }
        used = used.saturating_add(serialized.len());
        retained.push(item);
    }
    let returned = retained.len();
    json!({
        "truncated": returned < total,
        "itemsReturned": returned,
        "itemsOmitted": total.saturating_sub(returned),
        "items": retained,
    })
}

fn json_preview(value: Value, max_bytes: usize) -> Value {
    let serialized = match value {
        Value::String(text) => text,
        other => other.to_string(),
    };
    let (text, _, bytes_omitted, next_offset) = bounded_text(&serialized, max_bytes, 0);
    json!({
        "truncated": true,
        "bytesReturned": text.len(),
        "bytesOmitted": bytes_omitted,
        "nextOffset": next_offset,
        "jsonPreview": text,
    })
}

fn bounded_text(
    value: &str,
    max_bytes: usize,
    offset: usize,
) -> (String, bool, usize, Option<usize>) {
    if value.len() <= max_bytes {
        return (value.to_string(), false, 0, None);
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let text = value[..end].to_string();
    let omitted = value.len().saturating_sub(end);
    (text, true, omitted, Some(offset.saturating_add(end)))
}

fn utf8_prefix(value: &str, max_bytes: usize) -> &str {
    let mut end = max_bytes.min(value.len());
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    &value[..end]
}

fn utf8_suffix(value: &str, max_bytes: usize) -> &str {
    let mut start = value.len().saturating_sub(max_bytes);
    while start < value.len() && !value.is_char_boundary(start) {
        start += 1;
    }
    &value[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_projection_preserves_small_results_and_order() {
        let outputs = vec!["small".to_string(), "x".repeat(8_000), "tail".to_string()];

        let projected = model_visible_tool_output_batch_with_tokens(&outputs, 512);

        assert_eq!(projected.len(), outputs.len());
        assert_eq!(projected[0], "small");
        assert_eq!(projected[2], "tail");
        assert!(projected.iter().map(String::len).sum::<usize>() <= 512 * 4);
    }

    #[test]
    fn batch_projection_fairly_shares_budget_between_large_results() {
        let outputs = vec!["甲".repeat(4_000), "乙".repeat(4_000)];

        let projected = model_visible_tool_output_batch_with_tokens(&outputs, 256);

        assert_eq!(projected[0].len(), projected[1].len());
        assert!(projected.iter().map(String::len).sum::<usize>() <= 256 * 4);
        assert!(
            projected
                .iter()
                .all(|output| output.is_char_boundary(output.len()))
        );
    }

    #[test]
    fn batch_projection_strictly_bounds_json_results() {
        let outputs = vec![
            json!({ "stdout": "x".repeat(8_000) }).to_string(),
            json!({ "items": vec!["y".repeat(2_000); 8] }).to_string(),
        ];

        let projected = model_visible_tool_output_batch_with_tokens(&outputs, 128);

        assert!(projected.iter().map(String::len).sum::<usize>() <= 128 * 4);
    }

    #[test]
    fn batch_budget_adapts_to_remaining_context_with_a_fair_floor() {
        assert_eq!(model_tool_output_batch_token_budget(0, None), 3_000);
        assert_eq!(model_tool_output_batch_token_budget(1, Some(1)), 3_000);
        assert_eq!(model_tool_output_batch_token_budget(2, None), 6_000);
        assert_eq!(model_tool_output_batch_token_budget(2, Some(20_000)), 5_000);
        assert_eq!(model_tool_output_batch_token_budget(2, Some(100)), 256);
    }

    #[test]
    fn byte_projection_honors_tiny_strict_limits() {
        let projected = model_visible_tool_output_with_bytes("{\"value\":true}", 1);

        assert!(projected.len() <= 1);
    }

    #[test]
    fn default_enforce_keeps_twelve_kb_ceiling_for_oversized_output() {
        let oversized = "x".repeat(MAX_MODEL_TOOL_OUTPUT_BYTES * 2);

        let projected = enforce_model_output_limit(&oversized, MAX_MODEL_TOOL_OUTPUT_BYTES * 4);

        assert!(projected.len() <= MAX_MODEL_TOOL_OUTPUT_BYTES);
    }

    #[test]
    fn with_budget_raises_hard_ceiling_while_still_bounded() {
        let hard_bytes = MAX_MODEL_TOOL_OUTPUT_BYTES * 4;
        let oversized = json!({ "text": "x".repeat(hard_bytes / 2 + 1) }).to_string();
        assert!(oversized.len() > MAX_MODEL_TOOL_OUTPUT_BYTES);

        let projected = model_visible_tool_output_with_budget(&oversized, 16_000, hard_bytes);

        assert!(projected.len() <= hard_bytes);
        assert!(projected.len() > MAX_MODEL_TOOL_OUTPUT_BYTES);
    }

    #[test]
    fn with_budget_still_truncates_when_output_exceeds_custom_cap() {
        let hard_bytes = MAX_MODEL_TOOL_OUTPUT_BYTES * 2;
        let oversized = "x".repeat(hard_bytes * 2);

        let projected = model_visible_tool_output_with_budget(&oversized, 16_000, hard_bytes);

        assert!(projected.len() <= hard_bytes);
        assert!(serde_json::from_str::<Value>(&projected).is_ok());
    }
}
