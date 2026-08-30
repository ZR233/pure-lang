use tokio::sync::mpsc;

use pl_core::{AgentCommitObserver, AgentCommittedEvent, AgentRuntimeHandle};

use crate::studio::ProductEventBus;

use super::super::resources::StudioAgentResources;
use super::projector::StudioAgentEventProjector;

/// 把 framework 已提交事件投影到 Studio 目录；不再维护 Task continuation 或
/// planner wake 等第二套状态机。
#[derive(Clone)]
pub(in crate::studio) struct StudioAgentCommitObserver {
    sender: mpsc::UnboundedSender<AgentCommittedEvent>,
}

impl StudioAgentCommitObserver {
    pub(in crate::studio::agent_host) fn new(
        resources: StudioAgentResources,
        product_events: ProductEventBus,
    ) -> Self {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let projector = StudioAgentEventProjector {
            resources,
            product_events,
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
        Self { sender }
    }

    pub(in crate::studio::agent_host) async fn attach_runtime(&self, _runtime: AgentRuntimeHandle) {
    }

    pub(in crate::studio::agent_host) async fn detach_runtime(&self) {}
}

impl AgentCommitObserver for StudioAgentCommitObserver {
    async fn publish(&self, committed: AgentCommittedEvent) {
        if self.sender.send(committed).is_err() {
            tracing::warn!("Studio agent event projector is no longer running");
        }
    }
}
