use crate::studio::{InteractionService, ProductEventBus, StudioStore};
use crate::{
    InteractionKind, InteractionPayload, InteractionRequest, InteractionScope, InteractionStatus,
};
use futures::FutureExt;
use pl_core::{AgentLifecycleState, AgentRuntimeHandle, ThreadId};
use pl_protocol::ThreadNotification;
use pl_trace::TracePart;
use tokio::sync::watch;

use super::wait_for_runtime;
use crate::studio::store::RecoverablePlan;

#[derive(Clone)]
pub(super) struct StudioPlanConfirmationProjector {
    store: StudioStore,
    interactions: InteractionService,
    product_events: ProductEventBus,
    runtime: watch::Receiver<Option<AgentRuntimeHandle>>,
}

impl StudioPlanConfirmationProjector {
    pub(super) fn new(
        store: StudioStore,
        interactions: InteractionService,
        product_events: ProductEventBus,
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
        thread_id: &str,
        plan: &TracePart,
    ) -> anyhow::Result<bool> {
        self.project_plan(
            agent_id,
            thread_id,
            &plan.turn_id,
            &plan.item_id,
            &plan.content,
        )
        .await
    }

    async fn project_plan(
        &self,
        agent_id: &str,
        thread_id: &str,
        turn_id: &str,
        plan_id: &str,
        content: &str,
    ) -> anyhow::Result<bool> {
        if !self.is_task_root_planner(agent_id, thread_id).await? {
            tracing::debug!(
                agent_id,
                thread_id,
                plan_id,
                "ignored plan trace from non-root Task planner"
            );
            return Ok(false);
        }
        if content.trim().is_empty() {
            return Ok(false);
        }
        let interaction_id = format!("plan-confirmation-{plan_id}");
        if self
            .store
            .read_interaction(&interaction_id)
            .await?
            .is_some()
        {
            return Ok(false);
        }
        let now = crate::studio::ids::unix_seconds();
        let interaction = InteractionRequest {
            interaction_id,
            kind: InteractionKind::PlanConfirmation,
            status: InteractionStatus::Pending,
            scope: InteractionScope {
                thread_id: thread_id.to_string(),
                turn_id: turn_id.to_string(),
                item_id: Some(plan_id.to_string()),
                tool_id: None,
                agent_path: Some(agent_id.to_string()),
            },
            payload: InteractionPayload::PlanConfirmation {
                plan_id: plan_id.to_string(),
                content: content.to_string(),
            },
            created_at: now,
            updated_at: now,
            resolved_at: None,
            resolution: None,
        };
        let runtime = self.runtime.clone();
        let thread_id = thread_id.to_string();
        let agent_path = agent_id.to_string();
        let emitter: crate::studio::InteractionEmitter = std::sync::Arc::new(move |interaction| {
            let runtime = runtime.clone();
            let thread_id = thread_id.clone();
            let agent_path = agent_path.clone();
            async move {
                let runtime = wait_for_runtime(runtime).await?;
                let target_agent = pl_core::AgentId::new(agent_path.clone())?;
                let target_thread = ThreadId::new(thread_id)?;
                runtime
                    .record_thread_facts(
                        target_agent,
                        target_thread,
                        vec![pl_core::ThreadNotificationFact::durable(
                            interaction.updated_at,
                            ThreadNotification::InteractionChanged {
                                interaction: Box::new(interaction),
                            },
                        )],
                    )
                    .await?;
                Ok(())
            }
            .boxed()
        });
        self.interactions.create(interaction, emitter).await?;
        Ok(true)
    }

    async fn is_task_root_planner(&self, agent_id: &str, thread_id: &str) -> anyhow::Result<bool> {
        let Some(thread) = self.store.read_thread(thread_id).await? else {
            return Ok(false);
        };
        Ok(thread.mode == "task"
            && thread.parent_thread_id.is_none()
            && thread.id == thread.root_thread_id
            && thread.id == thread_id
            && thread.role == "planner"
            && thread.agent_path == agent_id)
    }

    pub(super) async fn recover_missing(&self) -> anyhow::Result<()> {
        for candidate in self.store.list_latest_task_plan_traces().await? {
            if let Err(error) = self
                .recover_candidate(&candidate.agent_id, &candidate.thread_id, &candidate.plan)
                .await
            {
                tracing::warn!(
                    thread_id = %candidate.thread_id,
                    plan_id = %candidate.plan.item_id,
                    error_bytes = error.to_string().len(),
                    "failed to recover pending plan confirmation"
                );
            }
        }
        Ok(())
    }

    async fn recover_candidate(
        &self,
        agent_id: &str,
        thread_id: &str,
        plan: &RecoverablePlan,
    ) -> anyhow::Result<()> {
        let interaction_id = format!("plan-confirmation-{}", plan.item_id);
        if self
            .store
            .read_interaction(&interaction_id)
            .await?
            .is_some()
        {
            return Ok(());
        }
        if self
            .store
            .find_active_task_run_for_root_thread(thread_id)
            .await?
            .is_some()
        {
            return Ok(());
        }
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
            .list_pending_interactions(thread_id)
            .await?
            .iter()
            .any(|interaction| interaction.kind == InteractionKind::PlanConfirmation)
        {
            return Ok(());
        }
        if !self
            .project_plan(
                agent_id,
                thread_id,
                &plan.turn_id,
                &plan.item_id,
                &plan.content,
            )
            .await?
        {
            return Ok(());
        }
        if let Some(thread) = self.store.read_thread(thread_id).await? {
            self.store
                .update_thread_status(
                    thread_id,
                    "waiting",
                    thread.summary,
                    None,
                    crate::studio::ids::unix_seconds(),
                )
                .await?;
            self.product_events
                .emit_thread_delta_for(&[thread_id.to_string()])
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    use crate::StudioMode;
    use crate::config::{ConfigPaths, ConfigStore};
    use crate::studio::{
        ChildThreadSpec, InteractionService, ProductEventBus, StudioRuntime, StudioStore,
    };
    use pl_protocol::{ThreadItem, ThreadItemContent, ThreadItemStatus, ThreadNotification};

    use super::*;

    #[tokio::test]
    async fn child_plan_trace_never_creates_plan_confirmation() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/child-plan").await.unwrap();
        let session = store
            .create_thread(&project.id, "Child plan", StudioMode::Task)
            .await
            .unwrap();
        let child_id = "agent-child-explorer".to_string();
        store
            .create_child_thread(ChildThreadSpec {
                id: child_id.clone(),
                parent_thread_id: session.id.clone(),
                agent_path: child_id.clone(),
                role: "explorer".to_string(),
                title: "Explore".to_string(),
            })
            .await
            .unwrap();
        let (_runtime_tx, runtime_rx) = watch::channel(None);
        let projector = StudioPlanConfirmationProjector::new(
            store.clone(),
            InteractionService::new(store.clone()),
            ProductEventBus::new(store.clone()),
            runtime_rx,
        );

        assert!(
            !projector
                .project_plan(
                    &child_id,
                    &child_id,
                    "turn-child-plan",
                    "child-plan-item",
                    "# 子代理错误计划",
                )
                .await
                .unwrap()
        );
        assert!(
            store
                .read_interaction("plan-confirmation-child-plan-item")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .list_pending_interactions(&child_id)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn recovers_latest_ephemeral_task_plan_without_duplicate_confirmation() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/recover-plan").await.unwrap();
        let session = store
            .create_thread(&project.id, "Recover plan", StudioMode::Task)
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
        )
        .unwrap();
        let framework = studio.agent_framework().await.unwrap();
        let (handle, agent_id) = studio.ensure_thread_agent(&session.id).await.unwrap();
        let plan_item = ThreadItem {
            id: "plan-item".to_string(),
            thread_id: session.id.clone(),
            turn_id: "turn-plan".to_string(),
            ordinal: 1,
            revision: 1,
            status: ThreadItemStatus::Completed,
            created_at: 1,
            updated_at: 2,
            completed_at: Some(2),
            error: None,
            content: ThreadItemContent::Plan {
                content: "# Plan\n\nImplement the fix.".to_string(),
            },
            usage: None,
        };
        store
            .database()
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT INTO turns
                 (id, thread_id, ordinal, revision, status, phase, reason, model_json,
                  usage_json, failure_json, budget_limit_json, rollover_compacted,
                  rollover_compaction_error, metadata_json, started_at, updated_at, completed_at)
                 VALUES (?, ?, 1, 1, ?, NULL, ?, NULL, ?, NULL, NULL, 0, NULL, ?, ?, ?, ?)",
                [
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
                    2_i64.into(),
                ],
            ))
            .await
            .unwrap();
        handle
            .record_thread_facts(
                agent_id.clone(),
                ThreadId::new(session.id.clone()).unwrap(),
                vec![pl_core::ThreadNotificationFact::durable(
                    2,
                    ThreadNotification::ItemCompleted {
                        item: Box::new(plan_item),
                    },
                )],
            )
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
            interaction.scope.agent_path.as_deref(),
            Some(agent_id.as_str())
        );
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
        let snapshot = studio.thread_snapshot(&session.id).await.unwrap();
        assert_eq!(
            snapshot
                .items
                .iter()
                .filter(|item| {
                    item.id == "plan-item"
                        && matches!(&item.content, ThreadItemContent::Plan { .. })
                })
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
                .read_thread(&session.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "waiting"
        );

        studio.shutdown().await;
        let _ = tokio::fs::remove_dir_all(home).await;
    }

    #[tokio::test]
    async fn resolved_plan_confirmation_is_skipped_without_a_runtime_actor() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/resolved-plan").await.unwrap();
        let thread = store
            .create_thread(&project.id, "Resolved plan", StudioMode::Task)
            .await
            .unwrap();
        let plan = RecoverablePlan {
            turn_id: "turn-resolved-plan".to_string(),
            item_id: "resolved-plan-item".to_string(),
            content: "# Plan\n\nAlready resolved.".to_string(),
        };
        let interaction_id = format!("plan-confirmation-{}", plan.item_id);
        store
            .upsert_interaction(&InteractionRequest {
                interaction_id: interaction_id.clone(),
                kind: InteractionKind::PlanConfirmation,
                status: InteractionStatus::Resolved,
                scope: InteractionScope {
                    thread_id: thread.id.clone(),
                    turn_id: plan.turn_id.clone(),
                    item_id: Some(plan.item_id.clone()),
                    tool_id: None,
                    agent_path: Some(thread.id.clone()),
                },
                payload: InteractionPayload::PlanConfirmation {
                    plan_id: plan.item_id.clone(),
                    content: plan.content.clone(),
                },
                created_at: 1,
                updated_at: 2,
                resolved_at: Some(2),
                resolution: Some(crate::InteractionResolution::PlanConfirmation {
                    decision: crate::PlanConfirmationResolution::Dismiss,
                    content: None,
                    reason: None,
                }),
            })
            .await
            .unwrap();
        let (_runtime_tx, runtime_rx) = watch::channel(None);
        let projector = StudioPlanConfirmationProjector::new(
            store.clone(),
            InteractionService::new(store.clone()),
            ProductEventBus::new(store.clone()),
            runtime_rx,
        );

        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            projector.recover_candidate(&thread.id, &thread.id, &plan),
        )
        .await
        .expect("resolved confirmation must not wait for a runtime actor")
        .unwrap();

        let interaction = store
            .read_interaction(&interaction_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(interaction.status, InteractionStatus::Resolved);
        assert!(
            store
                .list_pending_interactions(&thread.id)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
