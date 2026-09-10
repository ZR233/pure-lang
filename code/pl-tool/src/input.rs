//! JSON argument contracts owned by concrete tools.
use crate::tool_error;
/// Decodes a tool-owned JSON argument contract.
pub(crate) fn deserialize_tool_input<T: serde::de::DeserializeOwned + schemars::JsonSchema>(
    tool: &str,
    input: serde_json::Value,
) -> pl_protocol::Result<T> {
    let schema = typed_tool_input_schema::<T>();
    if let Some(arguments) = input.as_object()
        && let Some(properties) = schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
        && let Some(unknown) = arguments.keys().find(|key| !properties.contains_key(*key))
    {
        return Err(tool_error(
            tool,
            format!("invalid input: unknown field `{unknown}`"),
        ));
    }
    serde_json::from_value(input).map_err(|error| tool_error(tool, error))
}

/// Normalizes producer-owned JSON schemas for object-only function argument providers.
pub(crate) fn typed_tool_input_schema<T: schemars::JsonSchema>() -> serde_json::Value {
    let mut schema = schemars::schema_for!(T).to_value();
    if let Some(object) = schema.as_object_mut() {
        object.remove("$schema");
        object.remove("title");
        let object_union = ["oneOf", "anyOf"].iter().any(|key| {
            object
                .get(*key)
                .and_then(serde_json::Value::as_array)
                .is_some_and(|variants| {
                    !variants.is_empty()
                        && variants.iter().all(|variant| {
                            variant.get("type").and_then(serde_json::Value::as_str)
                                == Some("object")
                                || variant.get("properties").is_some()
                        })
                })
        });
        let properties = object.contains_key("properties");
        if properties
            || object_union
            || object.get("type").and_then(serde_json::Value::as_str) == Some("object")
        {
            object
                .entry("type")
                .or_insert_with(|| serde_json::Value::String("object".into()));
            if !object_union || properties {
                object.insert(
                    "additionalProperties".into(),
                    serde_json::Value::Bool(false),
                );
            }
        }
    }
    schema
}
