//! Durable mailbox input 的 runtime metadata 编解码。

use pl_core::DurableMailboxEnvelope;
use pl_core::MailboxMetadata;
use pl_protocol::PureError;

use super::store_error;

pub(super) const RUNTIME_INPUT_METADATA_KEY: &str = "$plAgentRuntime";
const INPUT_METADATA_PAYLOAD_KEY: &str = "payload";
const INPUT_METADATA_BUDGET_ACTION_KEY: &str = "budgetAction";

pub(super) fn serialize_input_metadata(
    input: &DurableMailboxEnvelope,
) -> Result<String, PureError> {
    if input.queue_coalescing_key.is_none()
        && input.budget_action == pl_core::MailboxBudgetAction::Preserve
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
    ),
    PureError,
> {
    let mut value: serde_json::Value = serde_json::from_str(input)?;
    let Some(object) = value.as_object_mut() else {
        return Ok((value.into(), None, pl_core::MailboxBudgetAction::Preserve));
    };
    let Some(runtime) = object.get(RUNTIME_INPUT_METADATA_KEY) else {
        return Ok((value.into(), None, pl_core::MailboxBudgetAction::Preserve));
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
    let payload = object
        .remove(INPUT_METADATA_PAYLOAD_KEY)
        .unwrap_or(serde_json::Value::Null);
    Ok((payload.into(), key, budget_action))
}
