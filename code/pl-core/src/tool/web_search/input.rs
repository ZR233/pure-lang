use pl_model::SearchCommands;
use pl_protocol::PureError;
use serde_json::{Value, json};

use crate::tool::deserialize_tool_input;

use super::TOOL_WEB_SEARCH;

pub(super) fn parse_commands(mut arguments: Value) -> Result<SearchCommands, PureError> {
    normalize_command_arguments(&mut arguments);
    deserialize_tool_input(TOOL_WEB_SEARCH, arguments)
}

fn normalize_command_arguments(arguments: &mut Value) {
    let Some(commands) = arguments.as_object_mut() else {
        return;
    };
    for name in ["search_query", "image_query"] {
        let Some(value) = commands.get_mut(name) else {
            continue;
        };
        if let Some(query) = value.as_str() {
            *value = json!([{ "q": query }]);
        } else if value.is_object() {
            *value = Value::Array(vec![value.take()]);
        }
    }
    for name in [
        "open",
        "click",
        "find",
        "screenshot",
        "finance",
        "weather",
        "sports",
        "time",
    ] {
        let Some(value) = commands.get_mut(name) else {
            continue;
        };
        if value.is_object() {
            *value = Value::Array(vec![value.take()]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_string_search_query_from_runtime_log() {
        let commands = parse_commands(json!({
            "search_query": "Flutter 最新版本 更新变化 release notes 2025"
        }))
        .expect("string query should be normalized");

        let queries = commands.search_query.expect("search queries");
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].q, "Flutter 最新版本 更新变化 release notes 2025");
    }

    #[test]
    fn normalizes_single_command_objects() {
        let commands = parse_commands(json!({
            "search_query": { "q": "Pure Lang", "recency": 7 },
            "open": { "ref_id": "turn0search0" },
            "find": { "ref_id": "turn0search0", "pattern": "install" }
        }))
        .expect("single command objects should be normalized");

        assert_eq!(commands.search_query.expect("search query").len(), 1);
        assert_eq!(commands.open.expect("open operation").len(), 1);
        assert_eq!(commands.find.expect("find operation").len(), 1);
    }
}
