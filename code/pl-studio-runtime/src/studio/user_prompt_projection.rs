use crate::{
    StudioEventKind, StudioMessage, StudioMessageRole, StudioMessageStatus, StudioPart,
    StudioPartStatus, StudioPartType, StudioTextChannel,
};
use anyhow::Result;
use pl_core::PendingAgentInput;

use crate::studio::ids::unix_seconds;
use crate::studio::{StudioEventRuntime, StudioStore};

/// 把已持久化的 framework 输入投影为 Studio 用户消息和消息部件。
pub(super) async fn project_user_prompt(
    store: &StudioStore,
    events: &StudioEventRuntime,
    session_id: &str,
    input: &PendingAgentInput,
) -> Result<()> {
    let now = unix_seconds();
    let turn_id = input.turn_id.as_str();
    let message_id = format!("{turn_id}:user");
    let presentation = input.metadata.get("userPrompt");
    let synthetic = presentation
        .and_then(|value| value.get("synthetic"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let ignored = presentation
        .and_then(|value| value.get("ignored"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let visible_prompt = presentation
        .and_then(|value| value.get("visiblePrompt"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&input.message);
    let attachment_ids = input
        .metadata
        .get("attachmentIds")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    let attachments = store
        .load_attachments(session_id, &attachment_ids)
        .await?
        .iter()
        .map(crate::studio::studio_attachment)
        .collect::<Vec<_>>();

    events
        .emit(
            None,
            Some(session_id.to_string()),
            Some(turn_id.to_string()),
            StudioEventKind::MessageUpdated {
                message: Box::new(StudioMessage {
                    message_id: message_id.clone(),
                    session_id: session_id.to_string(),
                    turn_id: turn_id.to_string(),
                    role: StudioMessageRole::User,
                    status: StudioMessageStatus::Completed,
                    created_at: now,
                    updated_at: now,
                    completed_at: Some(now),
                    error: None,
                    metadata: if synthetic || ignored {
                        serde_json::json!({
                            "synthetic": synthetic,
                            "ignored": ignored,
                        })
                    } else {
                        serde_json::json!({})
                    },
                }),
            },
        )
        .await?;
    events
        .emit(
            None,
            Some(session_id.to_string()),
            Some(turn_id.to_string()),
            StudioEventKind::MessagePartUpdated {
                part: Box::new(StudioPart {
                    part_id: format!("{turn_id}:user-text"),
                    message_id,
                    session_id: session_id.to_string(),
                    turn_id: turn_id.to_string(),
                    part_type: StudioPartType::Text,
                    order: 0,
                    revision: 0,
                    status: StudioPartStatus::Completed,
                    created_at: now,
                    updated_at: now,
                    completed_at: Some(now),
                    error: None,
                    text_channel: Some(StudioTextChannel::User),
                    activity_group_id: None,
                    text: visible_prompt.to_string(),
                    attachments,
                    tool: None,
                    agent: None,
                    inference: None,
                    plan: None,
                    file: None,
                    usage: None,
                    synthetic,
                    ignored,
                }),
            },
        )
        .await?;
    Ok(())
}
