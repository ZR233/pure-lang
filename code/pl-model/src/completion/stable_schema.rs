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
