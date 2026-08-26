use std::fmt::Write;

use sha2::{Digest, Sha256};

const MAX_TOOL_NAME_BYTES: usize = 64;

pub(super) fn assign_exposed_tool_names<'a>(
    tools: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<String> {
    tools
        .into_iter()
        .map(|(server, tool)| exposed_name(server, tool))
        .collect()
}

fn exposed_name(server: &str, tool: &str) -> String {
    let normalized_server = normalized_component(server, "server");
    let normalized_tool = normalized_component(tool, "tool");
    let normalized = format!("mcp__{normalized_server}__{normalized_tool}");
    let changed = normalized_server != server || normalized_tool != tool;
    if !changed && normalized.len() <= MAX_TOOL_NAME_BYTES {
        return normalized;
    }

    let suffix = format!("_{}", stable_hash(server, tool));
    let mut candidate = truncate_ascii(
        &normalized,
        MAX_TOOL_NAME_BYTES.saturating_sub(suffix.len()),
    );
    candidate.push_str(&suffix);
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

fn stable_hash(server: &str, tool: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(server.as_bytes());
    hasher.update([0]);
    hasher.update(tool.as_bytes());
    let digest = hasher.finalize();
    let mut hash = String::with_capacity(20);
    for byte in &digest[..10] {
        write!(&mut hash, "{byte:02x}").expect("writing to String cannot fail");
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

        assert!(names[0].starts_with("mcp__future_server__read_page_"));
        assert_eq!(names[1], "mcp__future_server__read_page");
        assert_ne!(names[0], names[1]);
        assert!(names.iter().all(|name| name.len() <= MAX_TOOL_NAME_BYTES));
        assert_eq!(
            names,
            assign_exposed_tool_names([
                ("future server", "read/page"),
                ("future_server", "read_page"),
                ("long", "a".repeat(100).as_str()),
            ])
        );
    }
}
