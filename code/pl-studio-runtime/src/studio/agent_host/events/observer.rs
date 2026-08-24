use tokio::sync::{mpsc, watch};

use pl_core::{AgentCommitObserver, AgentCommittedEvent, AgentRuntimeHandle};

use crate::studio::task_coordinator::TaskCoordinator;
use crate::studio::{InteractionService, ProductEventBus, StudioStore};

use super::super::resources::StudioAgentResources;
use super::continuation::recover_executor_continuation;
use super::planner_wake::materialize_pending_task_planner_wakes;
use super::projector::StudioAgentEventProjector;

/// 把已提交的 framework event/trace 投影到 Studio durable event stream。
#[derive(Clone)]
pub(in crate::studio) struct StudioAgentCommitObserver {
    sender: mpsc::UnboundedSender<AgentCommittedEvent>,
    runtime: watch::Sender<Option<AgentRuntimeHandle>>,
    store: StudioStore,
    coordinator: std::sync::Arc<TaskCoordinator>,
}

impl StudioAgentCommitObserver {
    pub(in crate::studio::agent_host) fn new(
        store: StudioStore,
        _interactions: InteractionService,
        coordinator: std::sync::Arc<TaskCoordinator>,
        resources: StudioAgentResources,
        product_events: ProductEventBus,
    ) -> Self {
        let (runtime, runtime_receiver) = watch::channel(None);
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let projector = StudioAgentEventProjector {
            resources,
            product_events,
            coordinator: coordinator.clone(),
            runtime: runtime_receiver,
        };
        tokio::spawn(async move {
            while let Some(committed) = receiver.recv().await {
                if let Err(error) = projector.project(committed).await {
                    tracing::warn!(
                        stage = error.stage,
                        error_bytes = error.source.to_string().len(),
                        "failed to project durable Studio agent event"
                    );
                }
            }
        });
        Self {
            sender,
            runtime,
            store,
            coordinator,
        }
    }

    pub(in crate::studio::agent_host) async fn attach_runtime(&self, runtime: AgentRuntimeHandle) {
        self.runtime.send_replace(Some(runtime.clone()));
        match self.store.list_pending_executor_continuations().await {
            Ok(continuations) => {
                for continuation in continuations {
                    if let Err(error) = recover_executor_continuation(
                        &runtime,
                        &self.store,
                        &self.coordinator.task_runtime(),
                        &continuation,
                    )
                    .await
                        && let Err(store_error) = self
                            .coordinator
                            .task_runtime()
                            .fail_executor_continuation(&continuation, &error.to_string())
                            .await
                    {
                        tracing::warn!(
                            error_bytes = store_error.to_string().len(),
                            "failed to persist executor continuation recovery failure"
                        );
                    }
                }
            }
            Err(error) => tracing::warn!(
                error_bytes = error.to_string().len(),
                "failed to list pending executor continuations"
            ),
        }
        if let Err(error) =
            materialize_pending_task_planner_wakes(&runtime, &self.coordinator.task_runtime(), None)
                .await
        {
            tracing::warn!(
                error_bytes = error.to_string().len(),
                "failed to recover pending Task Planner wakes"
            );
        }
    }

    pub(in crate::studio::agent_host) async fn detach_runtime(&self) {
        self.runtime.send_replace(None);
    }
}

impl AgentCommitObserver for StudioAgentCommitObserver {
    async fn publish(&self, committed: AgentCommittedEvent) {
        if self.sender.send(committed).is_err() {
            tracing::warn!("Studio agent event projector is no longer running");
        }
    }
}
