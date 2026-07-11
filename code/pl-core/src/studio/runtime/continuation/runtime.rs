use std::sync::Arc;

use anyhow::{Context, Result, bail};
use pl_protocol::ErrorSeverity;
use pl_trace::AgentEvent;

use crate::studio::runtime::{
    StudioRuntime, StudioSubmitPromptOptions, StudioSubmitPromptRequest,
    StudioUserPromptPresentation,
};
use crate::studio::task_coordinator::TaskContinuationSnapshot;

use super::{
    ContinuationLaunch, ContinuationLauncher, ContinuationReason, ContinuationRequest,
    SessionTurnState,
};

impl StudioRuntime {
    pub(crate) async fn request_task_continuation(
        &self,
        task_run_id: String,
        reason: ContinuationReason,
    ) {
        let run = match self.store.read_task_run(&task_run_id).await {
            Ok(Some(run)) => run,
            Ok(None) => return,
            Err(error) => {
                self.fail_task_continuation(
                    ContinuationRequest {
                        task_run_id,
                        session_id: String::new(),
                        reason,
                    },
                    error.context("load task run for continuation"),
                )
                .await;
                return;
            }
        };
        if run.phase.is_terminal() {
            return;
        }
        let request = ContinuationRequest {
            task_run_id,
            session_id: run.session_id,
            reason,
        };
        let session_turn_state = if self.active_turns.contains(&request.session_id).await {
            SessionTurnState::Active
        } else {
            SessionTurnState::Idle
        };
        if let Some(request) = self
            .continuation_scheduler
            .request(request, session_turn_state)
            .await
        {
            self.spawn_task_continuation(request);
        }
    }

    pub(crate) async fn active_turn_removed(&self, session_id: &str) {
        self.active_turns.remove(session_id).await;
        if let Some(request) = self.continuation_scheduler.turn_removed(session_id).await {
            self.spawn_task_continuation(request);
        }
    }

    fn spawn_task_continuation(&self, request: ContinuationRequest) {
        let runtime = self.clone();
        tokio::spawn(async move {
            runtime.launch_task_continuation(request).await;
        });
    }

    async fn launch_task_continuation(&self, request: ContinuationRequest) {
        let result = self.prepare_task_continuation(&request).await;
        let launch = match result {
            Ok(Some(launch)) => launch,
            Ok(None) => {
                self.continuation_scheduler
                    .cancel_session(&request.session_id)
                    .await;
                return;
            }
            Err(error) => {
                self.fail_task_continuation(request, error).await;
                return;
            }
        };
        let result = match &self.continuation_launcher {
            Some(launcher) => launcher.launch(launch.clone()).await,
            None => self.launch_production_continuation(launch.clone()).await,
        };
        if let Err(error) = result {
            if self.active_turns.contains(&request.session_id).await {
                self.continuation_scheduler.defer(request).await;
            } else {
                self.fail_task_continuation(request, error).await;
            }
        }
    }

    async fn prepare_task_continuation(
        &self,
        request: &ContinuationRequest,
    ) -> Result<Option<ContinuationLaunch>> {
        let snapshot: TaskContinuationSnapshot = self
            .store
            .load_task_continuation_snapshot(&request.task_run_id)
            .await
            .context("load durable task continuation snapshot")?;
        if snapshot.run.phase.is_terminal() {
            return Ok(None);
        }
        if snapshot.run.session_id != request.session_id {
            bail!("task continuation session changed");
        }
        let session = self
            .store
            .read_session(&request.session_id)
            .await?
            .context("task continuation session not found")?;
        if session.mode != "task" {
            bail!("task continuation session is not in task mode");
        }
        Ok(Some(ContinuationLaunch {
            request: request.clone(),
            prompt: snapshot.render_prompt()?,
        }))
    }

    async fn launch_production_continuation(&self, launch: ContinuationLaunch) -> Result<()> {
        self.submit_prompt(StudioSubmitPromptRequest {
            session_id: launch.request.session_id,
            prompt: launch.prompt,
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions {
                user_prompt: StudioUserPromptPresentation::SyntheticIgnored {
                    visible_prompt: "继续任务".to_string(),
                },
                lifecycle: None,
            },
        })
        .await?;
        Ok(())
    }

    async fn fail_task_continuation(&self, request: ContinuationRequest, error: anyhow::Error) {
        self.continuation_scheduler
            .cancel_session(&request.session_id)
            .await;
        let diagnostic = format!(
            "task continuation failed for {}: {error:#}",
            request.task_run_id
        );
        let _ = self
            .task_coordinator
            .block_continuation_failure(&request.task_run_id, diagnostic.clone())
            .await;
        if !request.session_id.is_empty() {
            let _ = self
                .events
                .emit_agent_event(
                    &request.session_id,
                    AgentEvent::Error {
                        message: diagnostic.clone(),
                        severity: ErrorSeverity::Recoverable,
                    },
                )
                .await;
        }
    }
}

pub(crate) type SharedContinuationLauncher = Arc<dyn ContinuationLauncher>;
