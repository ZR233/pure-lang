use anyhow::{Context, Result};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseTransaction, QueryResult, Statement, Value,
};
use serde_json::{Map, Value as JsonValue, json};

pub(crate) async fn migrate_session_event_v3(tx: &DatabaseTransaction) -> Result<()> {
    if table_exists(tx, "session_event_journal").await? {
        migrate_journal(tx).await?;
    }
    if table_exists(tx, "session_view_snapshots").await? {
        migrate_snapshots(tx).await?;
    }
    if table_exists(tx, "agent_pending_inputs").await? {
        migrate_pending_inputs(tx).await?;
    }
    if table_exists(tx, "agent_active_inputs").await? {
        migrate_active_inputs(tx).await?;
    }
    Ok(())
}

async fn migrate_journal(tx: &DatabaseTransaction) -> Result<()> {
    let rows = query(
        tx,
        "SELECT session_id, sequence, event_json
         FROM session_event_journal
         ORDER BY session_id, sequence",
    )
    .await?;
    let mut events = Vec::with_capacity(rows.len());
    for row in rows {
        let session_id = text(&row, "session_id")?;
        let sequence = integer(&row, "sequence")?;
        let event_json = text(&row, "event_json")?;
        let event: JsonValue = serde_json::from_str(&event_json)
            .with_context(|| format!("invalid session event {session_id}:{sequence}"))?;
        events.push((session_id, sequence, event));
    }
    let interaction_hints = events
        .iter()
        .filter_map(|(session_id, sequence, event)| {
            pending_interaction(event).map(|(turn_id, activity)| {
                (session_id.clone(), turn_id.to_string(), *sequence, activity)
            })
        })
        .collect::<Vec<_>>();

    for (session_id, sequence, mut event) in events {
        let interaction_activity = legacy_waiting_turn_id(&event).and_then(|turn_id| {
            interaction_hints
                .iter()
                .filter(|(hint_session_id, hint_turn_id, _, _)| {
                    hint_session_id == &session_id && hint_turn_id == turn_id
                })
                .min_by_key(|(_, _, hint_sequence, _)| sequence.abs_diff(*hint_sequence))
                .map(|(_, _, _, activity)| *activity)
        });
        migrate_session_value_with_hint(&mut event, interaction_activity);
        execute(
            tx,
            "UPDATE session_event_journal SET event_json = ? WHERE session_id = ? AND sequence = ?",
            [
                serde_json::to_string(&event)?.into(),
                session_id.into(),
                sequence.into(),
            ],
        )
        .await?;
    }
    Ok(())
}

async fn migrate_snapshots(tx: &DatabaseTransaction) -> Result<()> {
    let rows = query(
        tx,
        "SELECT session_id, snapshot_json FROM session_view_snapshots",
    )
    .await?;
    for row in rows {
        let session_id = text(&row, "session_id")?;
        let snapshot_json = text(&row, "snapshot_json")?;
        let mut snapshot: JsonValue = serde_json::from_str(&snapshot_json)
            .with_context(|| format!("invalid session snapshot {session_id}"))?;
        migrate_session_value(&mut snapshot);
        if let Some(object) = snapshot.as_object_mut() {
            object.insert("schemaVersion".to_string(), json!(3));
        }
        execute(
            tx,
            "UPDATE session_view_snapshots SET snapshot_json = ? WHERE session_id = ?",
            [serde_json::to_string(&snapshot)?.into(), session_id.into()],
        )
        .await?;
    }
    Ok(())
}

async fn migrate_pending_inputs(tx: &DatabaseTransaction) -> Result<()> {
    let rows = query(
        tx,
        "SELECT agent_id, queue_position, input_json FROM agent_pending_inputs",
    )
    .await?;
    for row in rows {
        let agent_id = text(&row, "agent_id")?;
        let queue_position = integer(&row, "queue_position")?;
        let input_json = text(&row, "input_json")?;
        let mut input: JsonValue = serde_json::from_str(&input_json).with_context(|| {
            format!("invalid pending mailbox input {agent_id}:{queue_position}")
        })?;
        migrate_mailbox_presentation(&mut input);
        execute(
            tx,
            "UPDATE agent_pending_inputs SET input_json = ? WHERE agent_id = ? AND queue_position = ?",
            [
                serde_json::to_string(&input)?.into(),
                agent_id.into(),
                queue_position.into(),
            ],
        )
        .await?;
    }
    Ok(())
}

async fn migrate_active_inputs(tx: &DatabaseTransaction) -> Result<()> {
    let rows = query(tx, "SELECT agent_id, input_json FROM agent_active_inputs").await?;
    for row in rows {
        let agent_id = text(&row, "agent_id")?;
        let input_json = text(&row, "input_json")?;
        let mut input: JsonValue = serde_json::from_str(&input_json)
            .with_context(|| format!("invalid active mailbox input {agent_id}"))?;
        migrate_mailbox_presentation(&mut input);
        execute(
            tx,
            "UPDATE agent_active_inputs SET input_json = ? WHERE agent_id = ?",
            [serde_json::to_string(&input)?.into(), agent_id.into()],
        )
        .await?;
    }
    Ok(())
}

fn migrate_session_value(value: &mut JsonValue) {
    let interaction_activity = snapshot_interaction_activity(value);
    migrate_session_value_with_hint(value, interaction_activity);
}

fn migrate_session_value_with_hint(
    value: &mut JsonValue,
    interaction_activity: Option<&'static str>,
) {
    match value {
        JsonValue::Object(object) => {
            migrate_reasoning_content(object);
            migrate_turn(object, interaction_activity);
            for child in object.values_mut() {
                migrate_session_value_with_hint(child, interaction_activity);
            }
        }
        JsonValue::Array(values) => {
            for child in values {
                migrate_session_value_with_hint(child, interaction_activity);
            }
        }
        _ => {}
    }
}

fn migrate_reasoning_content(object: &mut Map<String, JsonValue>) {
    if object.get("type").and_then(JsonValue::as_str) != Some("reasoning") {
        return;
    }
    let text = object
        .remove("text")
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default();
    object.entry("summary".to_string()).or_insert_with(|| {
        if text.is_empty() {
            json!([])
        } else {
            json!([text])
        }
    });
    object
        .entry("content".to_string())
        .or_insert_with(|| json!([]));
}

fn migrate_turn(object: &mut Map<String, JsonValue>, interaction_activity: Option<&'static str>) {
    if !object.contains_key("turnId")
        || !object.contains_key("sessionId")
        || !object.contains_key("updatedAt")
        || object.contains_key("createdAt")
        || object.contains_key("messageId")
        || object.contains_key("partId")
        || object.contains_key("state")
    {
        return;
    }
    let Some(status) = object
        .remove("status")
        .and_then(|value| value.as_str().map(str::to_string))
    else {
        return;
    };
    let reason = object
        .remove("reason")
        .and_then(|value| value.as_str().map(str::to_string))
        .filter(|value| !value.is_empty());
    object.insert(
        "state".to_string(),
        migrated_turn_state(&status, reason, interaction_activity),
    );
}

fn migrated_turn_state(
    status: &str,
    reason: Option<String>,
    interaction_activity: Option<&'static str>,
) -> JsonValue {
    match status {
        "queued" => json!({ "status": "queued" }),
        "contextLoading" => in_progress("preparing"),
        "waitingForModel" => in_progress("thinking"),
        "streaming" => in_progress("responding"),
        "waitingForInteraction" => {
            in_progress(interaction_activity.unwrap_or("waitingForUserInput"))
        }
        "runningTool" => in_progress("runningTool"),
        "persisting" => in_progress("persisting"),
        "completed" => json!({ "status": "completed" }),
        "cancelled" => json!({
            "status": "cancelled",
            "reason": reason.unwrap_or_else(|| "turn cancelled".to_string()),
        }),
        "failed" | _ => json!({
            "status": "failed",
            "reason": reason.unwrap_or_else(|| format!("legacy turn status: {status}")),
        }),
    }
}

fn snapshot_interaction_activity(value: &JsonValue) -> Option<&'static str> {
    let turn_id = value
        .get("turn")
        .and_then(|turn| turn.get("turnId"))
        .and_then(JsonValue::as_str)?;
    value
        .get("interactions")
        .and_then(JsonValue::as_array)?
        .iter()
        .find_map(|interaction| {
            let (interaction_turn_id, activity) = pending_interaction_value(interaction)?;
            (interaction_turn_id == turn_id).then_some(activity)
        })
}

fn pending_interaction(value: &JsonValue) -> Option<(&str, &'static str)> {
    pending_interaction_value(value.pointer("/kind/event/interaction")?)
}

fn pending_interaction_value(value: &JsonValue) -> Option<(&str, &'static str)> {
    if value.get("status").and_then(JsonValue::as_str) != Some("pending") {
        return None;
    }
    let turn_id = value.pointer("/scope/turnId").and_then(JsonValue::as_str)?;
    let activity = match value.get("kind").and_then(JsonValue::as_str)? {
        "userInput" => "waitingForUserInput",
        "toolApproval" => "waitingForApproval",
        "planConfirmation" => "waitingForPlanConfirmation",
        _ => return None,
    };
    Some((turn_id, activity))
}

fn legacy_waiting_turn_id(value: &JsonValue) -> Option<&str> {
    let turn = value.pointer("/kind/turn")?;
    (turn.get("status").and_then(JsonValue::as_str) == Some("waitingForInteraction"))
        .then(|| turn.get("turnId").and_then(JsonValue::as_str))
        .flatten()
}

fn in_progress(activity: &str) -> JsonValue {
    json!({ "status": "inProgress", "activity": activity })
}

fn migrate_mailbox_presentation(input: &mut JsonValue) {
    let Some(object) = input.as_object_mut() else {
        return;
    };
    if object.contains_key("presentation") {
        return;
    }
    let message = object
        .get("message")
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
        .to_string();
    let legacy = object
        .get_mut("metadata")
        .and_then(JsonValue::as_object_mut)
        .and_then(|metadata| metadata.remove("userPrompt"));
    let presentation = legacy
        .as_ref()
        .and_then(JsonValue::as_object)
        .map(|legacy| {
            let synthetic = legacy
                .get("synthetic")
                .and_then(JsonValue::as_bool)
                .unwrap_or(false);
            let ignored = legacy
                .get("ignored")
                .and_then(JsonValue::as_bool)
                .unwrap_or(false);
            if synthetic && ignored {
                json!({ "type": "syntheticHidden" })
            } else if synthetic {
                let prompt = legacy
                    .get("visiblePrompt")
                    .and_then(JsonValue::as_str)
                    .unwrap_or(&message);
                json!({ "type": "syntheticVisible", "prompt": prompt })
            } else {
                json!({ "type": "user" })
            }
        })
        .unwrap_or_else(|| json!({ "type": "user" }));
    object.insert("presentation".to_string(), presentation);
}

async fn table_exists(tx: &DatabaseTransaction, table: &str) -> Result<bool> {
    let row = tx
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?
            ) AS present",
            [table.into()],
        ))
        .await?
        .with_context(|| format!("failed to inspect Studio table {table}"))?;
    Ok(row.try_get::<i64>("", "present")? != 0)
}

async fn query(tx: &DatabaseTransaction, sql: &str) -> Result<Vec<QueryResult>> {
    Ok(tx
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            sql.to_string(),
        ))
        .await?)
}

async fn execute<const N: usize>(
    tx: &DatabaseTransaction,
    sql: &str,
    values: [Value; N],
) -> Result<()> {
    tx.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        sql,
        values,
    ))
    .await?;
    Ok(())
}

fn text(row: &QueryResult, column: &str) -> Result<String> {
    Ok(row.try_get("", column)?)
}

fn integer(row: &QueryResult, column: &str) -> Result<i64> {
    Ok(row.try_get("", column)?)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn migrates_v2_reasoning_and_turn_state_to_v3() {
        let mut snapshot = json!({
            "schemaVersion": 2,
            "sessionId": "session-1",
            "turn": {
                "turnId": "turn-1",
                "sessionId": "session-1",
                "status": "waitingForModel",
                "updatedAt": 7
            },
            "parts": [{
                "partId": "reasoning-1",
                "content": {
                    "type": "reasoning",
                    "text": "summary from v2"
                }
            }]
        });

        migrate_session_value(&mut snapshot);

        assert_eq!(
            snapshot["turn"]["state"],
            json!({"status": "inProgress", "activity": "thinking"})
        );
        assert_eq!(
            snapshot["parts"][0]["content"],
            json!({
                "type": "reasoning",
                "summary": ["summary from v2"],
                "content": []
            })
        );
    }

    #[test]
    fn migrates_v2_terminal_reason_and_unknown_status_without_silent_loss() {
        let mut cancelled = json!({
            "turnId": "turn-1",
            "sessionId": "session-1",
            "status": "cancelled",
            "reason": "stopped",
            "updatedAt": 7
        });
        let mut unknown = json!({
            "turnId": "turn-2",
            "sessionId": "session-1",
            "status": "futureStatus",
            "updatedAt": 8
        });

        migrate_session_value(&mut cancelled);
        migrate_session_value(&mut unknown);

        assert_eq!(
            cancelled["state"],
            json!({"status": "cancelled", "reason": "stopped"})
        );
        assert_eq!(
            unknown["state"],
            json!({
                "status": "failed",
                "reason": "legacy turn status: futureStatus"
            })
        );
    }

    #[test]
    fn migrates_generic_waiting_state_from_pending_interaction_kind() {
        let mut snapshot = json!({
            "schemaVersion": 2,
            "sessionId": "session-1",
            "turn": {
                "turnId": "turn-1",
                "sessionId": "session-1",
                "status": "waitingForInteraction",
                "updatedAt": 7
            },
            "interactions": [{
                "interactionId": "interaction-1",
                "kind": "toolApproval",
                "status": "pending",
                "scope": {
                    "sessionId": "session-1",
                    "turnId": "turn-1"
                }
            }]
        });

        migrate_session_value(&mut snapshot);

        assert_eq!(
            snapshot["turn"]["state"],
            json!({"status": "inProgress", "activity": "waitingForApproval"})
        );
    }

    #[test]
    fn generic_waiting_state_falls_back_when_interaction_kind_is_unavailable() {
        let mut turn = json!({
            "turnId": "turn-1",
            "sessionId": "session-1",
            "status": "waitingForInteraction",
            "updatedAt": 7
        });

        migrate_session_value(&mut turn);

        assert_eq!(
            turn["state"],
            json!({"status": "inProgress", "activity": "waitingForUserInput"})
        );
    }

    #[test]
    fn does_not_migrate_message_status_as_turn_state() {
        let mut message = json!({
            "messageId": "message-1",
            "turnId": "turn-1",
            "sessionId": "session-1",
            "status": "completed",
            "createdAt": 6,
            "updatedAt": 7
        });

        migrate_session_value(&mut message);

        assert_eq!(message["status"], json!("completed"));
        assert!(message.get("state").is_none());
    }

    #[test]
    fn migrates_legacy_mailbox_visibility_to_typed_presentation() {
        let mut user = json!({
            "message": "hello",
            "metadata": {}
        });
        let mut visible = json!({
            "message": "internal",
            "metadata": {
                "userPrompt": {
                    "synthetic": true,
                    "ignored": false,
                    "visiblePrompt": "visible"
                }
            }
        });
        let mut hidden = json!({
            "message": "internal",
            "metadata": {
                "userPrompt": {
                    "synthetic": true,
                    "ignored": true
                }
            }
        });

        migrate_mailbox_presentation(&mut user);
        migrate_mailbox_presentation(&mut visible);
        migrate_mailbox_presentation(&mut hidden);

        assert_eq!(user["presentation"], json!({"type": "user"}));
        assert_eq!(
            visible["presentation"],
            json!({"type": "syntheticVisible", "prompt": "visible"})
        );
        assert_eq!(hidden["presentation"], json!({"type": "syntheticHidden"}));
        assert!(visible["metadata"].get("userPrompt").is_none());
        assert!(hidden["metadata"].get("userPrompt").is_none());
    }
}
