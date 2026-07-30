use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use anyhow::{Context, Result};
use pl_core::{AgentActivityState, AgentLifecycleState, AgentSnapshot, SessionEventHub};
use pl_protocol::{SessionEventEnvelope, SessionEventPosition};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseTransaction, QueryResult, Statement, Value,
};

mod session_event_v3;

pub(super) use session_event_v3::migrate_session_event_v3;

pub(super) async fn split_v2_agent_sessions(tx: &DatabaseTransaction) -> Result<()> {
    let roots = load_root_sessions(tx).await?;
    let snapshots = load_agent_snapshots(tx).await?;
    let runtime_sessions = load_runtime_sessions(tx).await?;
    let sessions_by_agent = runtime_sessions_by_agent(&runtime_sessions);
    let root_ids = roots.keys().cloned().collect::<BTreeSet<_>>();
    let mut owner_by_session = roots
        .values()
        .map(|root| (root.id.clone(), format!("studio:{}", root.id)))
        .collect::<HashMap<_, _>>();

    for runtime_session in &runtime_sessions {
        if roots.contains_key(&runtime_session.session_id)
            || owner_by_session.contains_key(&runtime_session.session_id)
        {
            continue;
        }
        let Some(root_id) = resolve_root_session(
            &runtime_session.agent_id,
            &snapshots,
            &sessions_by_agent,
            &root_ids,
        ) else {
            continue;
        };
        let Some(root) = roots.get(&root_id) else {
            continue;
        };
        let snapshot = snapshots.get(&runtime_session.agent_id);
        let parent_session_id = snapshot
            .and_then(|snapshot| snapshot.identity.parent_id.as_ref())
            .and_then(|parent| {
                let parent = parent.as_str();
                parent
                    .strip_prefix("studio:")
                    .map(str::to_string)
                    .or_else(|| {
                        sessions_by_agent
                            .get(parent)
                            .and_then(|sessions| sessions.last())
                            .map(|session| session.session_id.clone())
                    })
            })
            .unwrap_or_else(|| root_id.clone());
        let role = snapshot
            .map(|snapshot| snapshot.identity.role.as_str().to_string())
            .unwrap_or_else(|| "agent".to_string());
        let status = snapshot
            .map(agent_status_label)
            .unwrap_or_else(|| "archived".to_string());
        let title = runtime_session
            .metadata
            .get("taskName")
            .or_else(|| runtime_session.metadata.get("name"))
            .or_else(|| runtime_session.metadata.get("title"))
            .and_then(serde_json::Value::as_str)
            .filter(|title| !title.trim().is_empty())
            .unwrap_or(&role)
            .chars()
            .take(80)
            .collect::<String>();
        let archived = i64::from(snapshot.is_none());
        let visibility = if archived == 0 { "active" } else { "archived" };
        tx.execute(statement(
            "INSERT INTO sessions (
                 id, project_id, title, mode, created_at, updated_at, archived,
                 instruction_snapshot_json, visibility, parent_session_id,
                 root_session_id, session_kind, owner_agent_id, owner_role,
                 agent_status, agent_summary, agent_error, agent_updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'agent', ?, ?, ?, NULL, NULL, ?)",
            [
                runtime_session.session_id.clone().into(),
                root.project_id.clone().into(),
                title.into(),
                root.mode.clone().into(),
                runtime_session.updated_at.into(),
                runtime_session.updated_at.into(),
                archived.into(),
                root.instruction_snapshot_json.clone().into(),
                visibility.to_string().into(),
                parent_session_id.into(),
                root_id.into(),
                runtime_session.agent_id.clone().into(),
                role.into(),
                status.into(),
                runtime_session.updated_at.into(),
            ],
        ))
        .await?;
        owner_by_session.insert(
            runtime_session.session_id.clone(),
            runtime_session.agent_id.clone(),
        );
    }

    let turn_sessions = load_turn_sessions(tx).await?;
    rebuild_session_journals(tx, &owner_by_session, &sessions_by_agent, &turn_sessions).await?;
    rebind_interactions(tx).await?;
    Ok(())
}

#[derive(Clone)]
struct RootSession {
    id: String,
    project_id: String,
    mode: String,
    instruction_snapshot_json: Option<String>,
}

#[derive(Clone)]
struct RuntimeSession {
    agent_id: String,
    session_id: String,
    metadata: serde_json::Value,
    updated_at: i64,
}

#[derive(Clone)]
struct StoredEvent {
    original_sequence: i64,
    emitted_at: i64,
    event: SessionEventEnvelope,
}

async fn load_root_sessions(tx: &DatabaseTransaction) -> Result<BTreeMap<String, RootSession>> {
    let rows = tx
        .query_all(statement(
            "SELECT id, project_id, mode, instruction_snapshot_json
             FROM sessions WHERE session_kind = 'root' ORDER BY created_at, id",
            [],
        ))
        .await?;
    rows.into_iter()
        .map(|row| {
            let id = text(&row, "id")?;
            Ok((
                id.clone(),
                RootSession {
                    id,
                    project_id: text(&row, "project_id")?,
                    mode: text(&row, "mode")?,
                    instruction_snapshot_json: optional_text(&row, "instruction_snapshot_json")?,
                },
            ))
        })
        .collect()
}

async fn load_agent_snapshots(tx: &DatabaseTransaction) -> Result<HashMap<String, AgentSnapshot>> {
    let rows = tx
        .query_all(statement(
            "SELECT agent_id, snapshot_json FROM agent_runtime_states ORDER BY agent_id",
            [],
        ))
        .await?;
    rows.into_iter()
        .map(|row| {
            let agent_id = text(&row, "agent_id")?;
            let snapshot = serde_json::from_str(&text(&row, "snapshot_json")?)
                .with_context(|| format!("invalid runtime snapshot for {agent_id}"))?;
            Ok((agent_id, snapshot))
        })
        .collect()
}

async fn load_runtime_sessions(tx: &DatabaseTransaction) -> Result<Vec<RuntimeSession>> {
    let rows = tx
        .query_all(statement(
            "SELECT agent_id, session_id, metadata_json, updated_at
             FROM agent_runtime_sessions ORDER BY updated_at, agent_id, session_id",
            [],
        ))
        .await?;
    rows.into_iter()
        .map(|row| {
            let metadata_json = text(&row, "metadata_json")?;
            Ok(RuntimeSession {
                agent_id: text(&row, "agent_id")?,
                session_id: text(&row, "session_id")?,
                metadata: serde_json::from_str(&metadata_json).unwrap_or(serde_json::Value::Null),
                updated_at: integer(&row, "updated_at")?,
            })
        })
        .collect()
}

fn runtime_sessions_by_agent(sessions: &[RuntimeSession]) -> HashMap<String, Vec<RuntimeSession>> {
    let mut by_agent = HashMap::<String, Vec<RuntimeSession>>::new();
    for session in sessions {
        by_agent
            .entry(session.agent_id.clone())
            .or_default()
            .push(session.clone());
    }
    by_agent
}

fn resolve_root_session(
    agent_id: &str,
    snapshots: &HashMap<String, AgentSnapshot>,
    sessions_by_agent: &HashMap<String, Vec<RuntimeSession>>,
    root_ids: &BTreeSet<String>,
) -> Option<String> {
    let mut current = agent_id.to_string();
    let mut visited = HashSet::new();
    while visited.insert(current.clone()) {
        if let Some(root_id) = current.strip_prefix("studio:")
            && root_ids.contains(root_id)
        {
            return Some(root_id.to_string());
        }
        if let Some(root_id) = sessions_by_agent
            .get(&current)
            .into_iter()
            .flatten()
            .map(|session| &session.session_id)
            .find(|session_id| root_ids.contains(*session_id))
        {
            return Some(root_id.clone());
        }
        current = snapshots
            .get(&current)?
            .identity
            .parent_id
            .as_ref()?
            .as_str()
            .to_string();
    }
    None
}

async fn load_turn_sessions(tx: &DatabaseTransaction) -> Result<HashMap<(String, String), String>> {
    let rows = tx
        .query_all(statement(
            "SELECT agent_id, turn_id, session_id FROM agent_turns",
            [],
        ))
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok((
                (text(&row, "agent_id")?, text(&row, "turn_id")?),
                text(&row, "session_id")?,
            ))
        })
        .collect()
}

async fn rebuild_session_journals(
    tx: &DatabaseTransaction,
    owner_by_session: &HashMap<String, String>,
    sessions_by_agent: &HashMap<String, Vec<RuntimeSession>>,
    turn_sessions: &HashMap<(String, String), String>,
) -> Result<()> {
    let known_sessions = owner_by_session.keys().cloned().collect::<BTreeSet<_>>();
    let rows = tx
        .query_all(statement(
            "SELECT session_id, sequence, event_json, emitted_at
             FROM session_event_journal ORDER BY emitted_at, session_id, sequence",
            [],
        ))
        .await?;
    let mut events_by_session = BTreeMap::<String, Vec<StoredEvent>>::new();
    for row in rows {
        let original_session_id = text(&row, "session_id")?;
        if !known_sessions.contains(&original_session_id) {
            continue;
        }
        let original_sequence = integer(&row, "sequence")?;
        let emitted_at = integer(&row, "emitted_at")?;
        let mut event: SessionEventEnvelope = serde_json::from_str(&text(&row, "event_json")?)?;
        let target_session_id = target_session_for_event(
            &event,
            &original_session_id,
            owner_by_session,
            sessions_by_agent,
            turn_sessions,
        );
        rebind_event_session(&mut event, &target_session_id)?;
        events_by_session
            .entry(target_session_id)
            .or_default()
            .push(StoredEvent {
                original_sequence,
                emitted_at,
                event,
            });
    }

    for session_id in known_sessions {
        let mut events = events_by_session.remove(&session_id).unwrap_or_default();
        events.sort_by(|left, right| {
            left.emitted_at
                .cmp(&right.emitted_at)
                .then(left.original_sequence.cmp(&right.original_sequence))
                .then(left.event.event_id.cmp(&right.event.event_id))
        });
        let mut seen = HashSet::new();
        events.retain(|event| seen.insert(event.event.event_id.clone()));
        let existing_snapshot = tx
            .query_one(statement(
                "SELECT snapshot_json FROM session_view_snapshots WHERE session_id = ?",
                [session_id.clone().into()],
            ))
            .await?
            .map(|row| -> Result<pl_protocol::SessionViewSnapshot> {
                Ok(serde_json::from_str(&text(&row, "snapshot_json")?)?)
            })
            .transpose()?;
        tx.execute(statement(
            "DELETE FROM session_event_journal WHERE session_id = ?",
            [session_id.clone().into()],
        ))
        .await?;

        let hub = SessionEventHub::default();
        for (index, stored) in events.iter_mut().enumerate() {
            let sequence = u64::try_from(index + 1)?;
            stored.event.session_id = session_id.clone();
            stored.event.position = SessionEventPosition::Durable { sequence };
            hub.publish_durable(stored.event.clone())?;
            tx.execute(statement(
                "INSERT INTO session_event_journal
                 (session_id, sequence, event_json, emitted_at) VALUES (?, ?, ?, ?)",
                [
                    session_id.clone().into(),
                    i64::try_from(sequence)?.into(),
                    serde_json::to_string(&stored.event)?.into(),
                    stored.emitted_at.into(),
                ],
            ))
            .await?;
        }
        let mut snapshot = if events.is_empty() {
            existing_snapshot.unwrap_or(hub.snapshot(&session_id)?)
        } else {
            hub.snapshot(&session_id)?
        };
        snapshot.session_id = session_id.clone();
        if let Some(owner_agent_id) = owner_by_session.get(&session_id) {
            let role = tx
                .query_one(statement(
                    "SELECT owner_role FROM sessions WHERE id = ?",
                    [session_id.clone().into()],
                ))
                .await?
                .map(|row| text(&row, "owner_role"))
                .transpose()?;
            snapshot.owner = Some(pl_protocol::SessionOwnerSnapshot {
                agent_id: owner_agent_id.clone(),
                role,
            });
        }
        tx.execute(statement(
            "INSERT INTO session_view_snapshots
             (session_id, through_sequence, snapshot_json, updated_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(session_id) DO UPDATE SET
               through_sequence = excluded.through_sequence,
               snapshot_json = excluded.snapshot_json,
               updated_at = excluded.updated_at",
            [
                session_id.clone().into(),
                i64::try_from(snapshot.through_sequence)?.into(),
                serde_json::to_string(&snapshot)?.into(),
                events
                    .last()
                    .map(|event| event.emitted_at)
                    .unwrap_or_default()
                    .into(),
            ],
        ))
        .await?;
        tx.execute(statement(
            "UPDATE agent_runtime_sessions SET session_event_sequence = ?
             WHERE session_id = ?",
            [
                i64::try_from(snapshot.through_sequence)?.into(),
                session_id.into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

fn target_session_for_event(
    event: &SessionEventEnvelope,
    original_session_id: &str,
    owner_by_session: &HashMap<String, String>,
    sessions_by_agent: &HashMap<String, Vec<RuntimeSession>>,
    turn_sessions: &HashMap<(String, String), String>,
) -> String {
    let Some(source_agent_id) = event.source_agent_id.as_ref() else {
        return original_session_id.to_string();
    };
    if owner_by_session.get(original_session_id) == Some(source_agent_id) {
        return original_session_id.to_string();
    }
    if let Some(turn_id) = event.turn_id.as_ref()
        && let Some(session_id) = turn_sessions.get(&(source_agent_id.clone(), turn_id.clone()))
    {
        return session_id.clone();
    }
    sessions_by_agent
        .get(source_agent_id)
        .and_then(|sessions| sessions.last())
        .map(|session| session.session_id.clone())
        .unwrap_or_else(|| original_session_id.to_string())
}

fn rebind_event_session(event: &mut SessionEventEnvelope, session_id: &str) -> Result<()> {
    let mut value = serde_json::to_value(&*event)?;
    rebind_session_values(&mut value, session_id);
    *event = serde_json::from_value(value)?;
    event.session_id = session_id.to_string();
    Ok(())
}

fn rebind_session_values(value: &mut serde_json::Value, session_id: &str) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                if key == "sessionId" && value.is_string() {
                    *value = serde_json::Value::String(session_id.to_string());
                } else {
                    rebind_session_values(value, session_id);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                rebind_session_values(value, session_id);
            }
        }
        _ => {}
    }
}

async fn rebind_interactions(tx: &DatabaseTransaction) -> Result<()> {
    tx.execute(statement(
        "UPDATE interactions
         SET session_id = (
             SELECT turns.session_id
             FROM agent_turns turns
             WHERE turns.turn_id = interactions.turn_id
             ORDER BY turns.finished_at DESC, turns.started_at DESC
             LIMIT 1
         )
         WHERE EXISTS (
             SELECT 1 FROM agent_turns turns
             WHERE turns.turn_id = interactions.turn_id
               AND turns.session_id <> interactions.session_id
         )",
        [],
    ))
    .await?;
    Ok(())
}

fn agent_status_label(snapshot: &AgentSnapshot) -> String {
    match snapshot.lifecycle {
        AgentLifecycleState::Closing | AgentLifecycleState::Closed => "shutdown",
        AgentLifecycleState::Faulted => "errored",
        AgentLifecycleState::Active => match snapshot.activity {
            AgentActivityState::Idle => "idle",
            AgentActivityState::Queued => "queued",
            AgentActivityState::Running => "running",
            AgentActivityState::WaitingTool
            | AgentActivityState::WaitingInteraction
            | AgentActivityState::WaitingAgents => "waiting",
        },
    }
    .to_string()
}

fn statement<const N: usize>(sql: &str, values: [Value; N]) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

fn text(row: &QueryResult, column: &str) -> Result<String> {
    Ok(row.try_get("", column)?)
}

fn optional_text(row: &QueryResult, column: &str) -> Result<Option<String>> {
    Ok(row.try_get("", column)?)
}

fn integer(row: &QueryResult, column: &str) -> Result<i64> {
    Ok(row.try_get("", column)?)
}
