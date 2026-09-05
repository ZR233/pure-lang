//! prompt 停止与按预期 Turn 身份的中断入口。

use anyhow::Result;

use super::super::{StudioInterruptPromptResponse, StudioRuntime, StudioStopPromptResponse};

impl StudioRuntime {
    pub async fn stop_prompt(&self, thread_id: String) -> Result<StudioStopPromptResponse> {
        let framework = self.agent_framework().await?;
        let handle = framework.handle();
        let agent_id = self.thread_agent_path(&thread_id).await?;
        let snapshot = match handle.snapshot(agent_id.clone()).await {
            Ok(snapshot) => snapshot,
            Err(pl_core::AgentRuntimeError::NotFound(_)) => {
                return Ok(StudioStopPromptResponse {
                    thread_id,
                    stopped: false,
                });
            }
            Err(error) => return Err(anyhow::anyhow!(error)),
        };
        let Some(turn_id) = snapshot.active_turn_id().cloned() else {
            return Ok(StudioStopPromptResponse {
                thread_id,
                stopped: false,
            });
        };
        match handle.cancel_turn(agent_id, turn_id).await {
            Ok(()) => {}
            Err(pl_core::AgentRuntimeError::NoActiveTurn(_))
            | Err(pl_core::AgentRuntimeError::TurnMismatch { .. }) => {
                return Ok(StudioStopPromptResponse {
                    thread_id,
                    stopped: false,
                });
            }
            Err(error) => return Err(anyhow::anyhow!(error)),
        }
        let emitter = self.interaction_emitter(thread_id.clone());
        self.agent_facility
            .interactions
            .cancel_thread(
                self.pending_thread_interactions(&thread_id).await?,
                "interrupted by user",
                emitter,
            )
            .await?;
        Ok(StudioStopPromptResponse {
            thread_id,
            stopped: true,
        })
    }

    /// Interrupts a Turn only when the caller's expected identity still matches the active Turn.
    pub async fn interrupt_prompt(
        &self,
        thread_id: String,
        expected_turn_id: String,
    ) -> Result<StudioInterruptPromptResponse> {
        let snapshot = self.thread_snapshot(&thread_id).await?;
        let active_turn_id = snapshot.active_turn.as_ref().map(|turn| turn.id.as_str());
        if active_turn_id.is_some_and(|active| active != expected_turn_id) {
            return Err(anyhow::Error::new(
                pl_protocol::studio::StudioError::invalid_argument(
                    "expected Turn does not match the active Turn",
                ),
            ));
        }
        let response = self.stop_prompt(thread_id).await?;
        Ok(StudioInterruptPromptResponse {
            thread_id: response.thread_id,
            turn_id: expected_turn_id,
            interrupted: response.stopped,
        })
    }
}
