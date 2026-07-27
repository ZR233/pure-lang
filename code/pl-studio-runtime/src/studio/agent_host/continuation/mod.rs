//! Task 产品事实信号与 executor delivery recovery 的宿主适配。

mod delivery_recovery;
mod signal;

use std::sync::Arc;

use pl_core::{AgentRuntimeHandle, SessionId};
use tokio::sync::RwLock;

use crate::studio::StudioStore;
use crate::studio::task_coordinator::TaskCoordinator;
use crate::{ErrorSeverity, SessionEventFact, SessionEventKind};

use super::resources::root_agent_id;

/// Task durable signal adapter 与 executor delivery recovery facade。
#[derive(Clone)]
pub(in crate::studio) struct StudioContinuationService {
    store: StudioStore,
    coordinator: Arc<TaskCoordinator>,
    runtime: Arc<RwLock<Option<AgentRuntimeHandle>>>,
}

impl StudioContinuationService {
    pub(in crate::studio) fn new(store: StudioStore, coordinator: Arc<TaskCoordinator>) -> Self {
        Self {
            store,
            coordinator,
            runtime: Default::default(),
        }
    }

    pub(in crate::studio) async fn attach(
        &self,
        runtime: AgentRuntimeHandle,
    ) -> anyhow::Result<()> {
        *self.runtime.write().await = Some(runtime);
        self.replay_durable_product_signals().await?;
        self.resume_pending_delivery_recoveries().await
    }

    pub(in crate::studio) async fn detach(&self) {
        *self.runtime.write().await = None;
    }

    async fn fail(&self, task_run_id: &str, error: anyhow::Error) {
        let diagnostic = format!("task continuation failed for {task_run_id}: {error:#}");
        let _ = self
            .coordinator
            .block_continuation_failure(task_run_id, diagnostic.clone())
            .await;
        if let Ok(Some(run)) = self.store.read_task_run(task_run_id).await {
            self.emit_error(&run.session_id, diagnostic);
        }
    }

    fn emit_error(&self, session_id: &str, message: String) {
        let runtime = self.runtime.clone();
        let session_id = session_id.to_string();
        tokio::spawn(async move {
            let Some(runtime) = runtime.read().await.clone() else {
                tracing::warn!("cannot record Studio continuation error before runtime attachment");
                return;
            };
            let emitted_at = crate::studio::ids::unix_seconds();
            let target = root_agent_id(&session_id);
            let session = match SessionId::new(session_id) {
                Ok(session) => session,
                Err(error) => {
                    tracing::warn!("invalid Studio continuation session: {error}");
                    return;
                }
            };
            if let Err(error) = runtime
                .record_session_facts(
                    target,
                    session,
                    vec![SessionEventFact::durable(
                        None,
                        None,
                        emitted_at,
                        SessionEventKind::ErrorOccurred {
                            message,
                            severity: ErrorSeverity::Recoverable,
                        },
                    )],
                )
                .await
            {
                tracing::warn!("failed to record Studio continuation error: {error}");
            }
        });
    }
}
