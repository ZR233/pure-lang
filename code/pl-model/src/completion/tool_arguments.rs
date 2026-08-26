use crate::completion::ToolCall;
use serde::Deserializer as _;
use serde::de::{Error as _, MapAccess, Visitor};
use std::collections::HashSet;
use std::fmt;

pub(crate) fn function_tool_call_from_raw(
    id: String,
    tool_name: String,
    arguments: String,
    call_id: String,
) -> ToolCall {
    match parse_unique_function_arguments(&arguments) {
        Ok(arguments) => ToolCall::function(id, tool_name, arguments, call_id),
        Err(error) => {
            ToolCall::invalid_function(id, tool_name, arguments, error.to_string(), call_id)
        }
    }
}

fn parse_unique_function_arguments(
    arguments: &str,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(arguments);
    deserializer.deserialize_map(UniqueTopLevelObject)?;
    deserializer.end()?;
    serde_json::from_str(arguments)
}

struct UniqueTopLevelObject;

impl<'de> Visitor<'de> for UniqueTopLevelObject {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("one JSON object with unique top-level fields")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut keys = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key.clone()) {
                return Err(A::Error::custom(format!(
                    "duplicate top-level field `{key}`"
                )));
            }
            map.next_value::<serde_json::Value>()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicate_function_argument_fields() {
        let call = function_tool_call_from_raw(
            "item-1".to_string(),
            "plan_exit".to_string(),
            r##"{"plan":"# One","plan":"# Two"}"##.to_string(),
            "call-1".to_string(),
        );

        assert!(
            call.invalid_arguments_message()
                .is_some_and(|message| message.contains("duplicate top-level field `plan`"))
        );
    }
}
