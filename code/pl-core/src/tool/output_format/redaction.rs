use serde_json::Value;

pub const SECRET_REDACTION_REPLACEMENT: &str = "<redacted>";
pub const MAX_TOOL_UI_PREVIEW_BYTES: usize = 2 * 1024;

/// 对产品层注入的明确 secret 做稳定遮蔽。
///
/// pl-core 的 trace preview 会根据字段名做启发式遮蔽；该类型用于 Git token、
/// MCP token 等产品层已知 secret。构造时会过滤空 secret，并按长度从长到短替换，
/// 避免重叠 token 被短前缀提前部分遮蔽。
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SecretRedaction {
    secrets: Vec<String>,
}

impl std::fmt::Debug for SecretRedaction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SecretRedaction")
            .field("secret_count", &self.secrets.len())
            .finish()
    }
}

impl SecretRedaction {
    pub fn new<S>(secrets: impl IntoIterator<Item = S>) -> Self
    where
        S: AsRef<str>,
    {
        let mut collected = Vec::new();
        for secret in secrets {
            let secret = secret.as_ref();
            if secret.is_empty() || collected.iter().any(|existing: &String| existing == secret) {
                continue;
            }
            collected.push(secret.to_string());
        }
        collected.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
        Self { secrets: collected }
    }

    pub fn is_empty(&self) -> bool {
        self.secrets.is_empty()
    }

    pub fn redact_str(&self, value: &str) -> String {
        if self.secrets.is_empty() {
            return value.to_string();
        }
        let mut redacted = value.to_string();
        for secret in &self.secrets {
            redacted = redacted.replace(secret, SECRET_REDACTION_REPLACEMENT);
        }
        redacted
    }

    pub fn redact_json_value(&self, value: Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.into_iter()
                    .map(|(key, value)| (self.redact_str(&key), self.redact_json_value(value)))
                    .collect(),
            ),
            Value::Array(items) => Value::Array(
                items
                    .into_iter()
                    .map(|value| self.redact_json_value(value))
                    .collect(),
            ),
            Value::String(value) => Value::String(self.redact_str(&value)),
            Value::Null | Value::Bool(_) | Value::Number(_) => value,
        }
    }
}

pub fn trace_preview_value(value: &Value, max: usize) -> String {
    let redacted = redacted_trace_preview_value(value);
    let serialized =
        serde_json::to_string_pretty(&redacted).unwrap_or_else(|_| redacted.to_string());
    preview(&serialized, max.min(MAX_TOOL_UI_PREVIEW_BYTES))
}

pub fn trace_preview_output(output: &str, max: usize) -> String {
    serde_json::from_str::<Value>(output)
        .map(|value| trace_preview_value(&value, max))
        .unwrap_or_else(|_| {
            preview(
                &redact_preview_string(output),
                max.min(MAX_TOOL_UI_PREVIEW_BYTES),
            )
        })
}

pub fn redacted_trace_preview_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, value) in map {
                if is_sensitive_key(key) {
                    out.insert(key.clone(), Value::String("<redacted>".to_string()));
                } else {
                    out.insert(key.clone(), redacted_trace_preview_value(value));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .take(20)
                .map(redacted_trace_preview_value)
                .chain(
                    (items.len() > 20)
                        .then(|| Value::String(format!("<{} more items>", items.len() - 20))),
                )
                .collect(),
        ),
        Value::String(value) => Value::String(redact_preview_string(value)),
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
    }
}

fn preview(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    const MARKER: &str = "...";
    if max <= MARKER.len() {
        return MARKER[..max].to_string();
    }
    let mut end = max - MARKER.len();
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}{MARKER}", &value[..end])
}

fn redact_preview_string(value: &str) -> String {
    if value.len() > 240 && looks_like_base64(value) {
        return format!("<base64 elided: {} chars>", value.len());
    }
    if value.len() > 800 {
        return format!("{}...", value.chars().take(800).collect::<String>());
    }
    value.to_string()
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || key.contains("authorization")
        || key.contains("api_key")
        || key.ends_with("_key")
        || key.contains("base64")
}

fn looks_like_base64(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.len() > 240
        && trimmed.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=' | b'\n' | b'\r')
        })
}

#[cfg(test)]
mod tests {
    #[test]
    fn trace_preview_never_exceeds_ui_budget() {
        let preview = super::trace_preview_output(
            &"a".repeat(super::MAX_TOOL_UI_PREVIEW_BYTES * 2),
            usize::MAX,
        );

        assert!(preview.len() <= super::MAX_TOOL_UI_PREVIEW_BYTES);
        assert_eq!(super::preview(&"a".repeat(32), 10), "aaaaaaa...");
    }
}
