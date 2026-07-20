use std::collections::BTreeSet;

const MAX_TOOL_NAME_BYTES: usize = 64;

pub(super) fn assign_exposed_tool_names<'a>(
    tools: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<String> {
    let mut used = BTreeSet::new();
    tools
        .into_iter()
        .map(|(server, tool)| unique_name(server, tool, &mut used))
        .collect()
}

fn unique_name(server: &str, tool: &str, used: &mut BTreeSet<String>) -> String {
    let server = normalized_component(server, "server");
    let tool = normalized_component(tool, "tool");
    let raw = format!("mcp__{server}__{tool}");
    let mut candidate = truncate_ascii(&raw, MAX_TOOL_NAME_BYTES);
    if used.insert(candidate.clone()) {
        return candidate;
    }
    let suffix = format!("_{:08x}", stable_hash(raw.as_bytes()));
    candidate = truncate_ascii(&raw, MAX_TOOL_NAME_BYTES.saturating_sub(suffix.len()));
    candidate.push_str(&suffix);
    let mut ordinal = 2_u32;
    while !used.insert(candidate.clone()) {
        let suffix = format!("_{:08x}_{ordinal}", stable_hash(raw.as_bytes()));
        candidate = truncate_ascii(&raw, MAX_TOOL_NAME_BYTES.saturating_sub(suffix.len()));
        candidate.push_str(&suffix);
        ordinal += 1;
    }
    candidate
}

fn normalized_component(value: &str, fallback: &str) -> String {
    let mut result = String::new();
    let mut replaced = false;
    for character in value.trim().chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
            result.push(character);
            replaced = false;
        } else if !replaced {
            result.push('_');
            replaced = true;
        }
    }
    let normalized = result.trim_matches('_');
    if normalized.is_empty() {
        fallback.to_string()
    } else {
        normalized.to_string()
    }
}

fn truncate_ascii(value: &str, max_bytes: usize) -> String {
    value.chars().take(max_bytes).collect()
}

fn stable_hash(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c9dc5_u32;
    for byte in bytes {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn names_are_normalized_bounded_and_collision_safe() {
        let names = assign_exposed_tool_names([
            ("future server", "read/page"),
            ("future_server", "read_page"),
            ("long", "a".repeat(100).as_str()),
        ]);

        assert_eq!(names[0], "mcp__future_server__read_page");
        assert_ne!(names[0], names[1]);
        assert!(names.iter().all(|name| name.len() <= MAX_TOOL_NAME_BYTES));
    }
}
