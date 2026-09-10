//! Product wire identity; core interaction IDs remain local to their Thread.
const PREFIX: &str = "studio-interaction:";

#[derive(Debug, thiserror::Error)]
#[error("invalid Studio interaction identity")]
pub struct InteractionKeyError;

pub(super) fn encode(thread_id: &str, local_id: &str) -> String {
    format!("{PREFIX}{}:{thread_id}{local_id}", thread_id.len())
}

/// Splits a product wire identity without interpreting either owner-defined component.
///
/// # Errors
/// Rejects missing prefixes, invalid lengths, empty components and non-UTF-8 boundaries.
pub fn decode_interaction_key(value: &str) -> Result<(&str, &str), InteractionKeyError> {
    let value = value.strip_prefix(PREFIX).ok_or(InteractionKeyError)?;
    let (length, components) = value.split_once(':').ok_or(InteractionKeyError)?;
    let parsed = length.parse::<usize>().map_err(|_| InteractionKeyError)?;
    if parsed.to_string() != length {
        return Err(InteractionKeyError);
    }
    let length = parsed;
    if length == 0 || length >= components.len() {
        return Err(InteractionKeyError);
    }
    let thread = components.get(..length).ok_or(InteractionKeyError)?;
    let local = components.get(length..).ok_or(InteractionKeyError)?;
    Ok((thread, local))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_are_unambiguous_with_unicode_colons_and_repeated_local_calls() {
        let id = encode("任务:甲", "permission:call");
        assert_eq!(
            decode_interaction_key(&id).unwrap(),
            ("任务:甲", "permission:call")
        );
        assert_ne!(id, encode("任务:乙", "permission:call"));
        for invalid in [
            "permission:call",
            "studio-interaction:1:任务:call",
            "studio-interaction:0:call",
            "studio-interaction:100:x",
        ] {
            assert!(decode_interaction_key(invalid).is_err());
        }
    }
}
