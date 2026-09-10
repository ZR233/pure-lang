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
    fn decodes_only_valid_json_escaped_fragments_once() {
        for (input, expected) in [
            (
                r#"Snippet: `\"unknown\\nusage\"`"#,
                Some("Snippet: `\"unknown\\nusage\"`"),
            ),
            (r#"first\nsecond"#, Some("first\nsecond")),
            ("plain text", None),
            (r#"C:\Users\name"#, None),
        ] {
            assert_eq!(
                decode_json_escaped_fragment_once(input).as_deref(),
                expected
            );
        }
    }
}
