//! Historical producer payload conversion, used only by the offline database migration.
use pl_core::{persistence::SessionStoreError, thread::journal::ThreadCommit};
use serde_json::Value;

pub(super) fn convert(commit: &mut ThreadCommit) -> Result<(), SessionStoreError> {
    // Walking the typed envelope never parses user text or unknown opaque payload content.
    // Original tool IDs, arguments and provider bindings remain historical execution facts.
    let mut encoded = serde_json::to_value(&*commit)?;
    payloads(&mut encoded)?;
    *commit = serde_json::from_value(encoded)?;
    Ok(())
}

fn payloads(value: &mut Value) -> Result<(), SessionStoreError> {
    match value {
        Value::Array(items) => {
            for item in items {
                payloads(item)?;
            }
        }
        Value::Object(fields) => {
            if fields.len() == 3
                && fields.contains_key("format")
                && fields.contains_key("version")
                && fields.contains_key("content")
            {
                let Some(format) = fields.get("format").and_then(Value::as_str) else {
                    return Ok(());
                };
                let format = format.to_owned();
                if !matches!(
                    format.as_str(),
                    "pl.tool.complete"
                        | "pl.studio.agent-progress"
                        | "pl.studio.child-notification"
                ) {
                    return Ok(());
                }
                if fields.get("version").and_then(Value::as_u64) != Some(1) {
                    return Err(SessionStoreError::Invalid(format!(
                        "unsupported historical {format} version"
                    )));
                }
                let content = fields
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        SessionStoreError::Invalid("invalid historical payload".into())
                    })?;
                if format == "pl.tool.complete" {
                    #[derive(serde::Deserialize)]
                    struct Completion {
                        summary: String,
                        #[serde(default)]
                        evidence: Vec<String>,
                    }
                    let old: Completion = serde_json::from_str(content)?;
                    let mut message = old.summary;
                    if !old.evidence.is_empty() {
                        message.push_str("\n\n历史验证证据：\n");
                        message.push_str(&old.evidence.join("\n"));
                    }
                    fields.insert("format".into(), "pl.tool.finish-turn".into());
                    fields.insert(
                        "content".into(),
                        serde_json::to_string(&pl_tool::finish_turn::FinishTurnInput { message })?
                            .into(),
                    );
                } else {
                    // These are history, never current completion authority. The exact original
                    // stage, revision, text and notification identifiers remain recoverable.
                    fields.insert(
                        "format".into(),
                        if format == "pl.studio.agent-progress" {
                            "pl.studio.historical-progress"
                        } else {
                            "pl.studio.historical-notification"
                        }
                        .into(),
                    );
                }
            } else {
                for child in fields.values_mut() {
                    payloads(child)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
