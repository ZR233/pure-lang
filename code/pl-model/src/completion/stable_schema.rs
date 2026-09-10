//! Deterministic encoding for supported typed tool declarations.
use pl_protocol::ToolSpec;
use std::collections::BTreeMap;

/// Orders declarations by stable tool identity and recursively sorts JSON object keys.
/// Arrays retain producer-specified order; this function does not decode opaque tool inputs.
pub fn stable_tool_schemas(mut tools: Vec<ToolSpec>) -> Vec<ToolSpec> {
    for tool in &mut tools {
        match tool {
            ToolSpec::Function {
                input_schema,
                output_schema,
                ..
            } => {
                canonicalize_json(input_schema);
                if let Some(output) = output_schema {
                    canonicalize_json(output);
                }
            }
            ToolSpec::Custom { output_schema, .. } => {
                if let Some(output) = output_schema {
                    canonicalize_json(output);
                }
            }
            ToolSpec::ProgrammaticToolCalling | ToolSpec::WebSearch { .. } => {}
        }
    }
    tools.sort_by(|left, right| left.name().cmp(right.name()));
    tools
}

pub(crate) fn canonicalize_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            let mut sorted = object
                .iter_mut()
                .map(|(key, value)| {
                    canonicalize_json(value);
                    (key.clone(), value.clone())
                })
                .collect::<BTreeMap<_, _>>();
            object.clear();
            object.extend(
                sorted
                    .iter_mut()
                    .map(|(key, value)| (key.clone(), value.take())),
            );
        }
        serde_json::Value::Array(items) => {
            for item in items {
                canonicalize_json(item);
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    #[test]
    fn tools_are_sorted_by_model_visible_name() {
        let tools = stable_tool_schemas(vec![
            ToolSpec::function("zeta", "", serde_json::json!({"b": 2, "a": 1})),
            ToolSpec::function("alpha", "", serde_json::json!({})),
        ]);

        assert_eq!(
            tools.iter().map(ToolSpec::name).collect::<Vec<_>>(),
            vec!["alpha", "zeta"]
        );
    }

    #[test]
    fn tool_schemas_are_canonicalized_and_sorted_across_key_orders() {
        let first = stable_tool_schemas(vec![
            ToolSpec::function(
                "git_status",
                "status",
                serde_json::json!({"type": "object", "properties": {}}),
            ),
            ToolSpec::function(
                "git_diff",
                "diff",
                serde_json::json!({
                    "type": "object",
                    "properties": {"path": {"type": "string", "description": "path"}}
                }),
            ),
        ]);
        let second = stable_tool_schemas(vec![
            ToolSpec::function(
                "git_diff",
                "diff",
                serde_json::json!({
                    "properties": {"path": {"description": "path", "type": "string"}},
                    "type": "object"
                }),
            ),
            ToolSpec::function(
                "git_status",
                "status",
                serde_json::json!({"properties": {}, "type": "object"}),
            ),
        ]);

        assert_eq!(
            serde_json::to_vec(&first).unwrap(),
            serde_json::to_vec(&second).unwrap()
        );
    }

    #[test]
    fn schema_normalization_preserves_array_semantics() {
        let input = serde_json::json!({"type":"object", "required":["z", "a"], "oneOf":[{"title":"second"},{"title":"first"}]});
        let tools = stable_tool_schemas(vec![ToolSpec::function(
            "fixture",
            "Fixture",
            input.clone(),
        )]);
        let ToolSpec::Function { input_schema, .. } = &tools[0] else {
            panic!("function");
        };
        assert_eq!(input_schema["required"], input["required"]);
        assert_eq!(input_schema["oneOf"], input["oneOf"]);
    }
}
