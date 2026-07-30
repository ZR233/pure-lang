use crate::studio::{InteractionRuntime, StudioProductEventRuntime, StudioStore};
use crate::{
    InteractionKind, InteractionPayload, InteractionRequest, InteractionScope, InteractionStatus,
    PlanLifecycleEvent, PlanLifecycleState, SessionEventFact, SessionEventKind,
};
use pl_core::{AgentLifecycleState, AgentRuntimeHandle, SessionId};
use pl_trace::TracePart;
use tokio::sync::watch;

use super::wait_for_runtime;

#[derive(Clone)]
pub(super) struct StudioPlanConfirmationProjector {
    store: StudioStore,
    interactions: InteractionRuntime,
    product_events: StudioProductEventRuntime,
    runtime: watch::Receiver<Option<AgentRuntimeHandle>>,
}

impl StudioPlanConfirmationProjector {
    pub(super) fn new(
        store: StudioStore,
        interactions: InteractionRuntime,
        product_events: StudioProductEventRuntime,
        runtime: watch::Receiver<Option<AgentRuntimeHandle>>,
    ) -> Self {
        Self {
            store,
            interactions,
            product_events,
            runtime,
        }
    }

    pub(super) async fn project(
        &self,
        agent_id: &str,
        studio_session_id: &str,
        plan: &TracePart,
    ) -> anyhow::Result<bool> {
        if plan.content.trim().is_empty() {
            return Ok(false);
        }
        let interaction_id = format!("plan-confirmation-{}", plan.item_id);
        if self
            .store
            .read_interaction(&interaction_id)
            .await?
            .is_some()
        {
            return Ok(false);
        }
        let now = crate::studio::ids::unix_seconds();
        self.record_facts(
            agent_id,
            studio_session_id,
            vec![SessionEventFact::durable(
                Some(agent_id.to_string()),
                Some(plan.turn_id.clone()),
                now,
                SessionEventKind::PlanChanged {
                    event: PlanLifecycleEvent {
                        plan_id: plan.item_id.clone(),
                        state: PlanLifecycleState::PendingConfirmation,
                        turn_id: Some(plan.turn_id.clone()),
                        reason: None,
                        updated_at: now,
                    },
                },
            )],
        )
        .await?;
        let interaction = InteractionRequest {
            interaction_id,
            kind: InteractionKind::PlanConfirmation,
            status: InteractionStatus::Pending,
            scope: InteractionScope {
                session_id: studio_session_id.to_string(),
                turn_id: plan.turn_id.clone(),
                item_id: Some(plan.item_id.clone()),
                tool_id: None,
                agent_path: None,
            },
            payload: InteractionPayload::PlanConfirmation {
                plan_id: plan.item_id.clone(),
                content: plan.content.clone(),
            },
            created_at: now,
            updated_at: now,
            resolved_at: None,
            resolution: None,
        };
        let runtime = self.runtime.clone();
        let session_id = studio_session_id.to_string();
        let owner_agent_id = agent_id.to_string();
        let emitter: crate::studio::InteractionEmitter = std::sync::Arc::new(move |interaction| {
            let runtime = runtime.clone();
            let session_id = session_id.clone();
            let owner_agent_id = owner_agent_id.clone();
            Box::pin(async move {
                let runtime = wait_for_runtime(runtime).await?;
                let target_agent = pl_core::AgentId::new(owner_agent_id.clone())?;
                let target_session = SessionId::new(session_id)?;
                runtime
                    .record_session_facts(
                        target_agent,
                        target_session,
                        vec![SessionEventFact::durable(
                            Some(owner_agent_id),
                            Some(interaction.scope.turn_id.clone()),
                            interaction.updated_at,
                            SessionEventKind::InteractionChanged {
                                event: Box::new(crate::InteractionChangedEvent { interaction }),
                            },
                        )],
                    )
                    .await?;
                Ok(())
            })
        });
        self.interactions.create(interaction, emitter).await?;
        Ok(true)
    }

    pub(super) async fn recover_missing(&self) -> anyhow::Result<()> {
        for candidate in self.store.list_latest_task_plan_traces().await? {
            if let Err(error) = self
                .recover_candidate(&candidate.agent_id, &candidate.session_id, &candidate.plan)
                .await
            {
                tracing::warn!(
                    session_id = %candidate.session_id,
                    plan_id = %candidate.plan.item_id,
                    "failed to recover pending plan confirmation: {error:#}"
                );
            }
        }
        Ok(())
    }

    async fn recover_candidate(
        &self,
        agent_id: &str,
        session_id: &str,
        plan: &TracePart,
    ) -> anyhow::Result<()> {
        let runtime = wait_for_runtime(self.runtime.clone()).await?;
        let snapshot = runtime
            .snapshot(pl_core::AgentId::new(agent_id.to_string())?)
            .await?;
        if snapshot.lifecycle != AgentLifecycleState::Active
            || snapshot.active_turn_id.is_some()
            || snapshot.pending_inputs != 0
        {
            return Ok(());
        }
        if self
            .store
            .find_active_task_run_for_session(session_id)
            .await?
            .is_some()
        {
            return Ok(());
        }
        if self
            .store
            .list_pending_interactions(session_id)
            .await?
            .iter()
            .any(|interaction| interaction.kind == InteractionKind::PlanConfirmation)
        {
            return Ok(());
        }
        if self
            .store
            .read_session_view_snapshot(session_id)
            .await?
            .is_some_and(|snapshot| {
                snapshot.plan_events.iter().any(|event| {
                    event.plan_id == plan.item_id
                        && event.state != PlanLifecycleState::PendingConfirmation
                })
            })
        {
            return Ok(());
        }
        if !self.project(agent_id, session_id, plan).await? {
            return Ok(());
        }
        if let Some(session) = self.store.read_session(session_id).await? {
            self.store
                .update_agent_session_status(
                    session_id,
                    "waiting",
                    session.agent_summary,
                    None,
                    crate::studio::ids::unix_seconds(),
                )
                .await?;
            self.product_events
                .emit_session_list(&session.project_id)
                .await?;
        }
        Ok(())
    }

    async fn record_facts(
        &self,
        agent_id: &str,
        session_id: &str,
        facts: Vec<SessionEventFact>,
    ) -> anyhow::Result<()> {
        let runtime = wait_for_runtime(self.runtime.clone()).await?;
        runtime
            .record_session_facts(
                pl_core::AgentId::new(agent_id.to_string())?,
                SessionId::new(session_id.to_string())?,
                facts,
            )
            .await
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    use crate::StudioMode;
    use crate::config::{ConfigPaths, ConfigStore};
    use crate::studio::{StudioRuntime, StudioStore};
    use pl_core::{
        AgentIdentity, AgentRegistration, AgentRoleId, AgentSessionState, AgentWakePolicy,
    };
    use pl_trace::{
        TraceEvent, TraceEventKind, TracePart, TracePartKind, TracePartSource, TracePartStatus,
    };

    use super::*;
    use crate::studio::agent_host::root_agent_id;

    #[tokio::test]
    async fn recovers_latest_ephemeral_task_plan_without_duplicate_confirmation() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/recover-plan").await.unwrap();
        let session = store
            .create_session(&project.id, "Recover plan", StudioMode::Task)
            .await
            .unwrap();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("pure-plan-recovery-{unique}"));
        let studio = StudioRuntime::new(
            store.clone(),
            ConfigStore::new(ConfigPaths::from_home(&home)),
        );
        let framework = studio.agent_framework().await.unwrap();
        let handle = framework.handle();
        let agent_id = root_agent_id(&session.id);
        handle
            .register(AgentRegistration {
                identity: AgentIdentity {
                    id: agent_id.clone(),
                    parent_id: None,
                    role: AgentRoleId::new("planner").unwrap(),
                    depth: 0,
                },
                wake_policy: AgentWakePolicy::RuntimeTerminal,
                sessions: vec![AgentSessionState::empty(
                    pl_core::SessionId::new(session.id.clone()).unwrap(),
                )],
            })
            .await
            .unwrap();
        let plan = TracePart {
            turn_id: "turn-plan".to_string(),
            item_id: "plan-item".to_string(),
            started_sequence: 1,
            revision: 1,
            kind: TracePartKind::Plan,
            status: TracePartStatus::Completed,
            created_at: 1,
            updated_at: 2,
            source: TracePartSource::Model,
            text_channel: None,
            content: "# Plan\n\nImplement the fix.".to_string(),
            attachments: Vec::new(),
            thinking_chunks: Vec::new(),
            reasoning_content_chunks: Vec::new(),
            tool: None,
            agent: None,
            inference: None,
            usage: None,
        };
        let trace = TraceEvent {
            session_id: session.id.clone(),
            sequence: 2,
            timestamp: 2,
            kind: TraceEventKind::TracePartCompleted { item: plan },
        };
        store
            .database()
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT INTO agent_turns
                 (agent_id, turn_id, session_id, status, reason, usage_json, metadata_json,
                  started_at, finished_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                [
                    agent_id.to_string().into(),
                    "turn-plan".to_string().into(),
                    session.id.clone().into(),
                    "completed".to_string().into(),
                    Option::<String>::None.into(),
                    serde_json::to_string(&pl_model::TokenUsage::default())
                        .unwrap()
                        .into(),
                    serde_json::json!({"historyPolicy": "ephemeral"})
                        .to_string()
                        .into(),
                    1_i64.into(),
                    2_i64.into(),
                ],
            ))
            .await
            .unwrap();
        store
            .database()
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT INTO agent_runtime_traces
                 (agent_id, session_id, sequence, payload_json, created_at)
                 VALUES (?, ?, ?, ?, ?)",
                [
                    agent_id.to_string().into(),
                    session.id.clone().into(),
                    2_i64.into(),
                    serde_json::to_string(&trace).unwrap().into(),
                    2_i64.into(),
                ],
            ))
            .await
            .unwrap();
        assert_eq!(
            store
                .agent_turn_metadata(agent_id.as_str(), "turn-plan")
                .await
                .unwrap()
                .unwrap()["historyPolicy"],
            "ephemeral"
        );

        framework.host().attach_runtime(handle.clone()).await;
        framework.host().attach_runtime(handle).await;

        let interaction = store
            .read_interaction("plan-confirmation-plan-item")
            .await
            .unwrap()
            .expect("missing plan confirmation should be recovered");
        assert_eq!(interaction.kind, InteractionKind::PlanConfirmation);
        assert_eq!(interaction.status, InteractionStatus::Pending);
        assert_eq!(
            interaction.payload,
            InteractionPayload::PlanConfirmation {
                plan_id: "plan-item".to_string(),
                content: "# Plan\n\nImplement the fix.".to_string(),
            }
        );
        assert_eq!(
            store.list_pending_interactions(&session.id).await.unwrap(),
            vec![interaction]
        );
        let snapshot = studio.session_event_snapshot(&session.id).await.unwrap();
        assert_eq!(
            snapshot
                .plan_events
                .iter()
                .filter(|event| event.plan_id == "plan-item")
                .count(),
            1
        );
        assert_eq!(
            snapshot
                .interactions
                .iter()
                .filter(|interaction| {
                    interaction.interaction_id == "plan-confirmation-plan-item"
                })
                .count(),
            1
        );
        assert_eq!(
            store
                .read_session(&session.id)
                .await
                .unwrap()
                .unwrap()
                .agent_status,
            "waiting"
        );

        studio.shutdown().await;
        let _ = tokio::fs::remove_dir_all(home).await;
    }
}
