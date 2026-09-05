//! Thread/Project 生命周期命令目录页：对外命令入口与 projects/threads 子模块组织。

mod projects;
mod threads;

use anyhow::Result;

use super::{StudioRuntime, StudioStartNewThreadRequest, StudioStartNewThreadResponse};

impl StudioRuntime {
    /// Creates a root Thread and accepts its first Turn from the shared API request.
    pub async fn create_thread_command(
        &self,
        project_id: String,
        request: pl_protocol::studio::CreateThreadRequest,
    ) -> Result<StudioStartNewThreadResponse> {
        let mode = pl_protocol::ThreadModeId::from_label(request.mode.trim()).map_err(|_| {
            anyhow::Error::new(pl_protocol::studio::StudioError::invalid_argument(
                "mode must be an available mode.* id",
            ))
        })?;
        self.start_new_thread(StudioStartNewThreadRequest {
            project_id,
            title: request.title,
            input: request.input,
            mode,
            options: super::StudioSubmitPromptOptions {
                turn_policy: pl_core::AgentTurnSubmitPolicy::StartOnly,
                ..super::StudioSubmitPromptOptions::default()
            },
        })
        .await
    }
}
