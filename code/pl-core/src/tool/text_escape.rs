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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::decode_json_escaped_fragment_once;

    #[test]
    fn decodes_json_escaped_fragment_once() {
        assert_eq!(
            decode_json_escaped_fragment_once(r#"Snippet: `\"unknown\\nusage\"`"#),
            Some("Snippet: `\"unknown\\nusage\"`".to_string())
        );
    }

    #[test]
    fn decodes_newline_escape() {
        assert_eq!(
            decode_json_escaped_fragment_once(r#"first\nsecond"#),
            Some("first\nsecond".to_string())
        );
    }

    #[test]
    fn leaves_plain_text_alone() {
        assert_eq!(decode_json_escaped_fragment_once("plain text"), None);
    }

    #[test]
    fn rejects_invalid_json_fragment() {
        assert_eq!(decode_json_escaped_fragment_once(r#"C:\Users\name"#), None);
    }
}
