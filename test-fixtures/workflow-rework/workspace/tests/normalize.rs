use workflow_live_fixture::normalize::{NormalizeError, normalize_key};

#[test]
fn normalizes_external_labels() {
    assert_eq!(
        normalize_key(" --Hello__World-- "),
        Ok("hello-world".into())
    );
}

#[test]
fn rejects_empty_labels() {
    assert_eq!(normalize_key(""), Err(NormalizeError::Empty));
}

#[test]
fn invalid_character_precedes_an_overlong_normalized_result() {
    let input = format!("{}!", "A".repeat(49));
    assert_eq!(
        normalize_key(&input),
        Err(NormalizeError::InvalidCharacter {
            index: 49,
            byte: b'!'
        }),
    );
}
