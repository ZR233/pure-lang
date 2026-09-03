//! Durable mailbox input 的 runtime metadata 编解码。

use pl_core::DurableMailboxEnvelope;
use pl_core::MailboxMetadata;
use pl_protocol::PureError;

use super::store_error;

pub(super) const RUNTIME_INPUT_METADATA_KEY: &str = "$plAgentRuntime";
const INPUT_METADATA_PAYLOAD_KEY: &str = "payload";
const INPUT_METADATA_BUDGET_ACTION_KEY: &str = "budgetAction";
const INPUT_METADATA_SOURCE_KEY: &str = "inputSource";

pub(super) fn serialize_input_metadata(
    input: &DurableMailboxEnvelope,
) -> Result<String, PureError> {
    if input.queue_coalescing_key.is_none()
        && input.budget_action == pl_core::MailboxBudgetAction::Preserve
        && input.payload.source == pl_core::MailboxInputSource::User
    {
        return Ok(serde_json::to_string(&input.payload.metadata)?);
    }
    let mut runtime = serde_json::Map::new();
    if let Some(key) = input.queue_coalescing_key.as_deref() {
        runtime.insert(
            "queueCoalescingKey".to_string(),
            serde_json::Value::String(key.to_string()),
        );
    }
    if input.budget_action != pl_core::MailboxBudgetAction::Preserve {
        runtime.insert(
            INPUT_METADATA_BUDGET_ACTION_KEY.to_string(),
            serde_json::Value::String(input.budget_action.as_str().to_string()),
        );
    }
    if input.payload.source != pl_core::MailboxInputSource::User {
        runtime.insert(
            INPUT_METADATA_SOURCE_KEY.to_string(),
            serde_json::Value::String(input.payload.source.as_str().to_string()),
        );
    }
    let value = serde_json::json!({
        RUNTIME_INPUT_METADATA_KEY: runtime,
        INPUT_METADATA_PAYLOAD_KEY: input.payload.metadata,
    });
    Ok(serde_json::to_string(&value)?)
}

pub(super) fn deserialize_input_metadata(
    input: &str,
) -> Result<
    (
        MailboxMetadata,
        Option<String>,
        pl_core::MailboxBudgetAction,
        pl_core::MailboxInputSource,
    ),
    PureError,
> {
    let mut value: serde_json::Value = serde_json::from_str(input)?;
    let Some(object) = value.as_object_mut() else {
        return Ok((
            value.into(),
            None,
            pl_core::MailboxBudgetAction::Preserve,
            pl_core::MailboxInputSource::User,
        ));
    };
    let Some(runtime) = object.get(RUNTIME_INPUT_METADATA_KEY) else {
        return Ok((
            value.into(),
            None,
            pl_core::MailboxBudgetAction::Preserve,
            pl_core::MailboxInputSource::User,
        ));
    };
    let key = runtime
        .get("queueCoalescingKey")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    let budget_action = match runtime
        .get(INPUT_METADATA_BUDGET_ACTION_KEY)
        .and_then(serde_json::Value::as_str)
    {
        Some(value) => pl_core::MailboxBudgetAction::from_persisted_str(value)
            .ok_or_else(|| store_error(format!("unknown mailbox budget action {value}")))?,
        None => pl_core::MailboxBudgetAction::Preserve,
    };
    let source = match runtime
        .get(INPUT_METADATA_SOURCE_KEY)
        .and_then(serde_json::Value::as_str)
    {
        Some(value) => pl_core::MailboxInputSource::from_persisted_str(value)
            .ok_or_else(|| store_error(format!("unknown mailbox input source {value}")))?,
        None => pl_core::MailboxInputSource::User,
    };
    let payload = object
        .remove(INPUT_METADATA_PAYLOAD_KEY)
        .unwrap_or(serde_json::Value::Null);
    Ok((payload.into(), key, budget_action, source))
}

#[cfg(test)]
mod tests {
    use pl_core::{DurableMailboxEnvelope, MailboxDeliveryState, MessagePresentation};
    use pl_core::{ThreadId, TurnId};

    use super::*;

    #[test]
    fn input_metadata_round_trips_runtime_fields_and_parent_agent_source() {
        for (queue_coalescing_key, budget_action) in [
            (
                Some("task-run:wakes".to_string()),
                pl_core::MailboxBudgetAction::Preserve,
            ),
            (None, pl_core::MailboxBudgetAction::Refresh),
        ] {
            let input = DurableMailboxEnvelope {
                mail_id: "mail:wake".to_string(),
                turn_id: TurnId::new("turn-wake").unwrap(),
                thread_id: ThreadId::new("thread-wake").unwrap(),
                payload: pl_core::MailboxInputPayload {
                    message: "wake".to_string(),
                    attachments: Vec::new(),
                    source: pl_core::MailboxInputSource::ParentAgent,
                    presentation: MessagePresentation::Hidden,
                    metadata: serde_json::json!({"kind": "taskWake"}).into(),
                },
                queue_coalescing_key: queue_coalescing_key.clone(),
                budget_action,
                delivery_state: MailboxDeliveryState::default(),
                queued_at: 1,
            };

            let stored = serialize_input_metadata(&input).unwrap();
            assert!(stored.contains(r#""inputSource":"parentAgent""#));
            let (metadata, key, restored_budget_action, restored_source) =
                deserialize_input_metadata(&stored).unwrap();

            assert_eq!(metadata, input.payload.metadata);
            assert_eq!(key, queue_coalescing_key);
            assert_eq!(restored_budget_action, budget_action);
            assert_eq!(restored_source, input.payload.source);
        }
    }

    #[test]
    fn payload_only_input_metadata_remains_unwrapped() {
        let stored = r#"{"kind":"taskWake"}"#;
        let (metadata, key, budget_action, source) = deserialize_input_metadata(stored).unwrap();

        assert_eq!(
            metadata,
            pl_core::MailboxMetadata::from(serde_json::json!({"kind": "taskWake"}))
        );
        assert_eq!(key, None);
        assert_eq!(budget_action, pl_core::MailboxBudgetAction::Preserve);
        assert_eq!(source, pl_core::MailboxInputSource::User);
    }
}
