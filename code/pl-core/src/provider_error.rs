pub(crate) fn is_provider_429_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    contains_standalone_status_code(&lower, "429")
}

fn contains_standalone_status_code(text: &str, code: &str) -> bool {
    text.match_indices(code).any(|(index, _)| {
        let before = text[..index].chars().next_back();
        let after = text[index + code.len()..].chars().next();
        !before.is_some_and(|ch| ch.is_ascii_digit())
            && !after.is_some_and(|ch| ch.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_provider_429_error_codes() {
        assert!(is_provider_429_error(
            "API error 429 Too Many Requests: concurrency limit reached"
        ));
        assert!(is_provider_429_error("provider returned status 429"));
        assert!(is_provider_429_error("429 Too Many Requests"));
        assert!(!is_provider_429_error("Too Many Requests"));
        assert!(!is_provider_429_error(
            "API error 500 internal server error"
        ));
        assert!(!is_provider_429_error("local tool failed with code 1429"));
    }
}
