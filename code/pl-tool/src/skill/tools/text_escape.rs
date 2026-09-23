pub(super) fn decode_json_escaped_fragment_once(input: &str) -> Option<String> {
    if !looks_like_json_escaped_fragment(input) {
        return None;
    }

    let decoded = serde_json::from_str::<String>(&format!("\"{input}\"")).ok()?;
    (decoded != input).then_some(decoded)
}

fn looks_like_json_escaped_fragment(input: &str) -> bool {
    input.contains("\\\"")
        || input.contains("\\\\")
        || input.contains("\\/")
        || input.contains("\\b")
        || input.contains("\\f")
        || input.contains("\\n")
        || input.contains("\\r")
        || input.contains("\\t")
        || input.contains("\\u")
}
