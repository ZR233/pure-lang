use super::ToolExecutionOutcome;

pub(super) fn display_result_for_tool(
    tool_call: &pl_model::completion::ToolCall,
    tool_name: &str,
    result: &str,
    outcome: ToolExecutionOutcome,
) -> String {
    if tool_name == "request_user_input" && outcome == ToolExecutionOutcome::Succeeded {
        return redact_user_input_display_result(&tool_call.arguments_for_tool(), result);
    }
    result.to_string()
}

pub(super) fn redact_user_input_display_result(
    arguments: &serde_json::Value,
    result: &str,
) -> String {
    let secret_ids = arguments
        .get("questions")
        .and_then(serde_json::Value::as_array)
        .map(|questions| {
            questions
                .iter()
                .filter(|question| {
                    question
                        .get("isSecret")
                        .or_else(|| question.get("is_secret"))
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false)
                })
                .filter_map(|question| question.get("id").and_then(serde_json::Value::as_str))
                .map(ToOwned::to_owned)
                .collect::<std::collections::HashSet<_>>()
        })
        .unwrap_or_default();
    if secret_ids.is_empty() {
        return result.to_string();
    }
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(result) else {
        return "[redacted user input]".to_string();
    };
    if let Some(answers) = value
        .get_mut("answers")
        .and_then(serde_json::Value::as_object_mut)
    {
        for id in secret_ids {
            if let Some(answer) = answers.get_mut(&id)
                && let Some(answer_object) = answer.as_object_mut()
            {
                answer_object.insert("answers".to_string(), serde_json::json!(["[redacted]"]));
            }
        }
    }
    serde_json::to_string(&value).unwrap_or_else(|_| "[redacted user input]".to_string())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    #[test]
    fn redacts_secret_user_input_answers_for_display() {
        let arguments = serde_json::json!({
            "questions": [
                { "id": "api_key", "header": "Key", "question": "API key?", "isSecret": true },
                { "id": "mode", "header": "Mode", "question": "Mode?", "isSecret": false }
            ]
        });
        let result = serde_json::json!({
            "answers": {
                "api_key": { "answers": ["sk-secret"] },
                "mode": { "answers": ["Fast"] }
            }
        })
        .to_string();

        let display = redact_user_input_display_result(&arguments, &result);

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&display).unwrap(),
            serde_json::json!({
                "answers": {
                    "api_key": { "answers": ["[redacted]"] },
                    "mode": { "answers": ["Fast"] }
                }
            })
        );
    }
}
