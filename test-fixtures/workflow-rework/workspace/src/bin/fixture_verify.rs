use workflow_live_fixture::normalize::{NormalizeError, normalize_key};

fn main() {
    assert_eq!(
        normalize_key(" --Release__Candidate  42-- "),
        Ok("release-candidate-42".into())
    );
    assert_eq!(normalize_key("\u{b}A\u{c}_B\r\n"), Ok("a-b".into()));
    assert_eq!(
        normalize_key("  a! "),
        Err(NormalizeError::InvalidCharacter {
            index: 3,
            byte: b'!'
        })
    );
    assert_eq!(normalize_key(" -_ "), Err(NormalizeError::Empty));
    assert_eq!(normalize_key(&"A".repeat(48)), Ok("a".repeat(48)));
    assert_eq!(normalize_key(&"a".repeat(49)), Err(NormalizeError::TooLong));
    println!("PURE_WORKFLOW_GUI_VERIFY_OK");
}
