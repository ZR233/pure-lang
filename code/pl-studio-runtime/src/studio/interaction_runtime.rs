use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::{
    InteractionKind, InteractionRequest, InteractionResolution, InteractionStatus,
    PlanConfirmationResolution, ToolApprovalResolution,
};
use anyhow::{Context, Result};
use tokio::sync::{Mutex, oneshot};

use crate::InteractionCallback;
use crate::studio::StudioStore;
use crate::studio::ids::unix_seconds;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InteractionCancelScope {
    All,
    ToolApprovalOnly,
}

impl InteractionCancelScope {
    fn includes(self, interaction: &InteractionRequest) -> bool {
        match self {
            Self::All => true,
            Self::ToolApprovalOnly => interaction.kind == InteractionKind::ToolApproval,
        }
    }
}

pub type InteractionEmitterFuture = Pin<Box<dyn Future<Output = Result<()>> + Send>>;
pub type InteractionEmitter =
    Arc<dyn Fn(InteractionRequest) -> InteractionEmitterFuture + Send + Sync>;

#[derive(Clone)]
pub struct InteractionRuntime {
    store: StudioStore,
    waiters: Arc<Mutex<HashMap<String, InteractionWaiter>>>,
}

struct InteractionWaiter {
    sender: oneshot::Sender<InteractionResolution>,
}

impl InteractionRuntime {
    pub fn new(store: StudioStore) -> Self {
        Self {
            store,
            waiters: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn callback(&self, session_id: String, emitter: InteractionEmitter) -> InteractionCallback {
        let runtime = self.clone();
        Arc::new(move |request: InteractionRequest| {
            let runtime = runtime.clone();
            let emitter = emitter.clone();
            let session_id = session_id.clone();
            Box::pin(async move { runtime.ask(session_id, request, emitter).await })
        })
    }

    pub async fn create(
        &self,
        interaction: InteractionRequest,
        emitter: InteractionEmitter,
    ) -> Result<InteractionRequest> {
        self.persist_and_emit(interaction.clone(), emitter).await?;
        Ok(interaction)
    }

    pub async fn recover_user_input(
        &self,
        mut interaction: InteractionRequest,
        emitter: InteractionEmitter,
    ) -> Result<InteractionRequest> {
        anyhow::ensure!(
            interaction.kind == InteractionKind::UserInput
                && interaction.status == InteractionStatus::Cancelled,
            "only a cancelled user input can be recovered"
        );
        let now = unix_seconds();
        interaction.status = InteractionStatus::Pending;
        interaction.updated_at = now;
        interaction.resolved_at = None;
        interaction.resolution = None;
        self.persist_and_emit(interaction.clone(), emitter).await?;
        Ok(interaction)
    }

    pub async fn resolve(
        &self,
        interaction_id: &str,
        resolution: InteractionResolution,
        emitter: InteractionEmitter,
    ) -> Result<InteractionRequest> {
        let mut interaction = self
            .store
            .read_interaction(interaction_id)
            .await?
            .context("interaction not found")?;
        if interaction.status != InteractionStatus::Pending {
            return Ok(interaction);
        }
        if !resolution_matches_kind(&interaction.kind, &resolution) {
            anyhow::bail!("interaction resolution kind mismatch");
        }
        let now = unix_seconds();
        interaction.status = InteractionStatus::Resolved;
        interaction.updated_at = now;
        interaction.resolved_at = Some(now);
        interaction.resolution = Some(resolution);
        self.persist_and_emit(interaction.clone(), emitter).await?;
        if let Some(waiter) = self.waiters.lock().await.remove(interaction_id)
            && let Some(resolution) = interaction.resolution.clone()
        {
            let _ = waiter.sender.send(resolution);
        }
        Ok(interaction)
    }

    pub async fn cancel_session(
        &self,
        session_id: &str,
        reason: &str,
        emitter: InteractionEmitter,
    ) -> Result<()> {
        self.cancel_pending_interactions(session_id, reason, emitter, InteractionCancelScope::All)
            .await
    }

    pub async fn cancel_recovered_tool_approvals(
        &self,
        session_id: &str,
        reason: &str,
        emitter: InteractionEmitter,
    ) -> Result<()> {
        self.cancel_pending_interactions(
            session_id,
            reason,
            emitter,
            InteractionCancelScope::ToolApprovalOnly,
        )
        .await
    }

    async fn cancel_pending_interactions(
        &self,
        session_id: &str,
        reason: &str,
        emitter: InteractionEmitter,
        scope: InteractionCancelScope,
    ) -> Result<()> {
        let pending = self.store.list_pending_interactions(session_id).await?;
        for mut interaction in pending {
            if !scope.includes(&interaction) {
                continue;
            }
            let resolution = cancelled_resolution(&interaction.kind, reason);
            let now = unix_seconds();
            interaction.status = InteractionStatus::Cancelled;
            interaction.updated_at = now;
            interaction.resolved_at = Some(now);
            interaction.resolution = Some(resolution.clone());
            self.persist_and_emit(interaction.clone(), emitter.clone())
                .await?;
            if let Some(waiter) = self
                .waiters
                .lock()
                .await
                .remove(&interaction.interaction_id)
            {
                let _ = waiter.sender.send(resolution);
            }
        }
        Ok(())
    }

    async fn ask(
        &self,
        session_id: String,
        mut request: InteractionRequest,
        emitter: InteractionEmitter,
    ) -> InteractionResolution {
        request.scope.session_id = session_id;
        if request.scope.turn_id.trim().is_empty() {
            request.scope.turn_id = request.interaction_id.clone();
        }
        let (sender, receiver) = oneshot::channel();
        let interaction_id = request.interaction_id.clone();
        if let Some(waiter) = self
            .waiters
            .lock()
            .await
            .insert(interaction_id.clone(), InteractionWaiter { sender })
        {
            let _ = waiter.sender.send(cancelled_resolution(
                &request.kind,
                "interaction replaced by a newer request",
            ));
        }
        if let Err(error) = self.persist_and_emit(request.clone(), emitter).await {
            self.waiters.lock().await.remove(&interaction_id);
            tracing::error!(
                interaction_id,
                %error,
                "failed to persist Studio interaction"
            );
            return cancelled_resolution(&request.kind, "interaction persistence failed");
        }
        receiver
            .await
            .unwrap_or_else(|_| cancelled_resolution(&request.kind, "interaction channel closed"))
    }

    async fn persist_and_emit(
        &self,
        interaction: InteractionRequest,
        emitter: InteractionEmitter,
    ) -> Result<()> {
        self.store.upsert_interaction(&interaction).await?;
        emitter(interaction).await
    }
}

pub fn resolution_matches_kind(kind: &InteractionKind, resolution: &InteractionResolution) -> bool {
    match (kind, resolution) {
        (InteractionKind::UserInput, InteractionResolution::UserInput { .. })
        | (InteractionKind::ToolApproval, InteractionResolution::ToolApproval { .. })
        | (InteractionKind::PlanConfirmation, InteractionResolution::PlanConfirmation { .. }) => {
            true
        }
        (InteractionKind::UserInput, InteractionResolution::ToolApproval { .. })
        | (InteractionKind::UserInput, InteractionResolution::PlanConfirmation { .. })
        | (InteractionKind::ToolApproval, InteractionResolution::UserInput { .. })
        | (InteractionKind::ToolApproval, InteractionResolution::PlanConfirmation { .. })
        | (InteractionKind::PlanConfirmation, InteractionResolution::UserInput { .. })
        | (InteractionKind::PlanConfirmation, InteractionResolution::ToolApproval { .. }) => false,
    }
}

fn cancelled_resolution(kind: &InteractionKind, reason: &str) -> InteractionResolution {
    match kind {
        InteractionKind::UserInput => InteractionResolution::UserInput {
            answers: Default::default(),
        },
        InteractionKind::ToolApproval => InteractionResolution::ToolApproval {
            decision: ToolApprovalResolution::Denied,
            reason: Some(reason.to_string()),
        },
        InteractionKind::PlanConfirmation => InteractionResolution::PlanConfirmation {
            decision: PlanConfirmationResolution::Dismiss,
            content: None,
            reason: Some(reason.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use crate::{InteractionPayload, InteractionScope, ToolApprovalResolution, UserInputAnswer};
    use pretty_assertions::assert_eq;
    use tokio::sync::Mutex;

    use super::*;
    use crate::StudioMode;

    async fn store_with_session() -> (StudioStore, String) {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/interactions").await.unwrap();
        let session = store
            .create_session(&project.id, "Interaction test", StudioMode::Simple)
            .await
            .unwrap();
        (store, session.id)
    }

    async fn wait_pending(store: &StudioStore, session_id: &str) -> Vec<InteractionRequest> {
        for _ in 0..100 {
            let pending = store.list_pending_interactions(session_id).await.unwrap();
            if !pending.is_empty() {
                return pending;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        store.list_pending_interactions(session_id).await.unwrap()
    }

    async fn wait_event_count(
        events: &Arc<Mutex<Vec<InteractionRequest>>>,
        expected: usize,
    ) -> usize {
        for _ in 0..100 {
            let count = events.lock().await.len();
            if count >= expected {
                return count;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        events.lock().await.len()
    }

    fn emitter(events: Arc<Mutex<Vec<InteractionRequest>>>) -> InteractionEmitter {
        Arc::new(move |interaction| {
            let events = events.clone();
            Box::pin(async move {
                events.lock().await.push(interaction);
                Ok(())
            })
        })
    }

    fn user_input_interaction(id: &str) -> InteractionRequest {
        InteractionRequest {
            interaction_id: id.to_string(),
            kind: InteractionKind::UserInput,
            status: InteractionStatus::Pending,
            scope: InteractionScope {
                session_id: String::new(),
                turn_id: "turn-1".to_string(),
                item_id: Some("tool-1".to_string()),
                tool_id: Some("tool-1".to_string()),
                agent_path: Some("/root/child".to_string()),
            },
            payload: InteractionPayload::UserInput {
                questions: Vec::new(),
            },
            created_at: 1,
            updated_at: 1,
            resolved_at: None,
            resolution: None,
        }
    }

    fn tool_approval_interaction(session_id: &str, id: &str) -> InteractionRequest {
        InteractionRequest {
            interaction_id: id.to_string(),
            kind: InteractionKind::ToolApproval,
            status: InteractionStatus::Pending,
            scope: InteractionScope {
                session_id: session_id.to_string(),
                turn_id: "turn-1".to_string(),
                item_id: Some(id.to_string()),
                tool_id: Some(id.to_string()),
                agent_path: None,
            },
            payload: InteractionPayload::ToolApproval {
                name: "exec".to_string(),
                arguments: serde_json::json!({"command": "echo hi"}),
                working_directory: None,
                parent_agent_id: None,
            },
            created_at: 1,
            updated_at: 1,
            resolved_at: None,
            resolution: None,
        }
    }

    #[tokio::test]
    async fn callback_persists_pending_and_waits_for_resolution() {
        let (store, session_id) = store_with_session().await;
        let runtime = InteractionRuntime::new(store.clone());
        let events = Arc::new(Mutex::new(Vec::new()));
        let callback = runtime.callback(session_id.clone(), emitter(events.clone()));
        let waiter = tokio::spawn(callback(user_input_interaction("ask-1")));

        let pending = wait_pending(&store, &session_id).await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].scope.session_id, session_id);
        assert_eq!(pending[0].scope.agent_path.as_deref(), Some("/root/child"));
        assert_eq!(wait_event_count(&events, 1).await, 1);

        let resolution = InteractionResolution::UserInput {
            answers: HashMap::from([(
                "mode".to_string(),
                UserInputAnswer {
                    answers: vec!["Fast".to_string()],
                },
            )]),
        };
        let resolved = runtime
            .resolve("ask-1", resolution.clone(), emitter(events.clone()))
            .await
            .unwrap();

        assert_eq!(resolved.status, InteractionStatus::Resolved);
        assert_eq!(waiter.await.unwrap(), resolution);
        assert!(
            store
                .list_pending_interactions(&session_id)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(wait_event_count(&events, 2).await, 2);
    }

    #[tokio::test]
    async fn cancel_session_marks_pending_and_releases_waiters() {
        let (store, session_id) = store_with_session().await;
        let runtime = InteractionRuntime::new(store.clone());
        let events = Arc::new(Mutex::new(Vec::new()));
        let callback = runtime.callback(session_id.clone(), emitter(events.clone()));
        let waiter = tokio::spawn(callback(tool_approval_interaction(&session_id, "call-1")));
        assert_eq!(wait_pending(&store, &session_id).await.len(), 1);

        runtime
            .cancel_session(&session_id, "interrupted by test", emitter(events.clone()))
            .await
            .unwrap();
        let resolution = waiter.await.unwrap();
        assert_eq!(
            resolution,
            InteractionResolution::ToolApproval {
                decision: ToolApprovalResolution::Denied,
                reason: Some("interrupted by test".to_string()),
            }
        );

        let stored = store.read_interaction("call-1").await.unwrap().unwrap();
        assert_eq!(stored.status, InteractionStatus::Cancelled);
        assert_eq!(stored.resolution, Some(resolution));
        assert!(
            store
                .list_pending_interactions(&session_id)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(wait_event_count(&events, 2).await, 2);
    }

    #[tokio::test]
    async fn duplicate_interaction_id_releases_replaced_waiter() {
        let (store, session_id) = store_with_session().await;
        let runtime = InteractionRuntime::new(store.clone());
        let events = Arc::new(Mutex::new(Vec::new()));
        let callback = runtime.callback(session_id.clone(), emitter(events.clone()));
        let first_waiter = tokio::spawn(callback(user_input_interaction("ask-1")));
        assert_eq!(wait_pending(&store, &session_id).await.len(), 1);

        let second_waiter = tokio::spawn(callback(user_input_interaction("ask-1")));
        let first_resolution = first_waiter.await.unwrap();
        assert_eq!(
            first_resolution,
            InteractionResolution::UserInput {
                answers: HashMap::new()
            }
        );

        let resolution = InteractionResolution::UserInput {
            answers: HashMap::from([(
                "mode".to_string(),
                UserInputAnswer {
                    answers: vec!["Careful".to_string()],
                },
            )]),
        };
        runtime
            .resolve("ask-1", resolution.clone(), emitter(events.clone()))
            .await
            .unwrap();

        assert_eq!(second_waiter.await.unwrap(), resolution);
    }

    #[tokio::test]
    async fn restart_cancellation_preserves_user_input_and_plan_confirmation() {
        let (store, session_id) = store_with_session().await;
        let runtime = InteractionRuntime::new(store.clone());
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut user_input = user_input_interaction("ask-1");
        user_input.scope.session_id = session_id.clone();
        let mut plan = user_input_interaction("plan-1");
        plan.kind = InteractionKind::PlanConfirmation;
        plan.scope.session_id = session_id.clone();
        plan.payload = InteractionPayload::PlanConfirmation {
            plan_id: "turn-1-plan".to_string(),
            content: "1. Inspect\n2. Implement".to_string(),
        };
        let approval = tool_approval_interaction(&session_id, "approval-1");

        runtime
            .create(user_input.clone(), emitter(events.clone()))
            .await
            .unwrap();
        runtime
            .create(plan.clone(), emitter(events.clone()))
            .await
            .unwrap();
        runtime
            .create(approval.clone(), emitter(events.clone()))
            .await
            .unwrap();
        runtime
            .cancel_recovered_tool_approvals(
                &session_id,
                "application restarted",
                emitter(events.clone()),
            )
            .await
            .unwrap();

        let ask = store.read_interaction("ask-1").await.unwrap().unwrap();
        let stored_plan = store.read_interaction("plan-1").await.unwrap().unwrap();
        let stored_approval = store
            .read_interaction("approval-1")
            .await
            .unwrap()
            .unwrap();
        let pending = store.list_pending_interactions(&session_id).await.unwrap();

        assert_eq!(ask.status, InteractionStatus::Pending);
        assert_eq!(stored_plan.status, InteractionStatus::Pending);
        assert_eq!(stored_approval.status, InteractionStatus::Cancelled);
        assert_eq!(
            stored_approval.resolution,
            Some(InteractionResolution::ToolApproval {
                decision: ToolApprovalResolution::Denied,
                reason: Some("application restarted".to_string()),
            })
        );
        assert_eq!(pending, vec![stored_plan, ask]);
    }

    #[tokio::test]
    async fn create_persists_plan_confirmation_without_waiter() {
        let (store, session_id) = store_with_session().await;
        let runtime = InteractionRuntime::new(store.clone());
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut interaction = user_input_interaction("plan-1");
        interaction.kind = InteractionKind::PlanConfirmation;
        interaction.scope.session_id = session_id;
        interaction.payload = InteractionPayload::PlanConfirmation {
            plan_id: "plan-item-1".to_string(),
            content: "1. Inspect\n2. Implement".to_string(),
        };

        runtime
            .create(interaction.clone(), emitter(events.clone()))
            .await
            .unwrap();

        assert_eq!(
            store.read_interaction("plan-1").await.unwrap(),
            Some(interaction.clone())
        );
        assert_eq!(events.lock().await.as_slice(), &[interaction]);
    }
}
