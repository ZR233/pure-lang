use workflow_live_fixture::normalize::{NormalizeError, normalize_key};
use workflow_live_fixture::validate::{ValidationError, validate_key};

fn main() {
    let key = normalize_key("  Release__Candidate 42 ")
        .expect("fixture normalization must accept the canonical example");
    assert_eq!(key, "release-candidate-42");
    validate_key(&key).expect("the normalized fixture key must validate");
    assert_eq!(
        normalize_key("  -Release__Candidate 42-_  "),
        Ok("release-candidate-42".to_string())
    );
    assert_eq!(normalize_key(" _- \t"), Err(NormalizeError::Empty));
    assert_eq!(
        normalize_key("release!candidate"),
        Err(NormalizeError::InvalidCharacter {
            index: 7,
            byte: b'!'
        })
    );
    assert_eq!(normalize_key(&"a".repeat(49)), Err(NormalizeError::TooLong));
    assert_eq!(validate_key("release-candidate-42"), Ok(()));
    assert_eq!(
        validate_key("release--candidate"),
        Err(ValidationError::InvalidSeparator { index: 7 })
    );
    println!("PURE_WORKFLOW_GUI_VERIFY_OK");
}
