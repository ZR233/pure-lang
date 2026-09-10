//! Product interactions are projected from the immutable Thread journal.
use crate::studio::store::StudioStore;
use crate::{InteractionRequest, InteractionStatus};
use anyhow::Result;

impl StudioStore {
    pub async fn read_interaction(
        &self,
        interaction_id: &str,
    ) -> Result<Option<InteractionRequest>> {
        for id in self.sessions().session_ids().await? {
            let state = self.sessions().replay_thread(&id).await?;
            if let Some(interaction) =
                crate::thread_assembler::project_thread_interaction(&id, interaction_id, &state)?
            {
                return Ok(Some(interaction));
            }
        }
        Ok(None)
    }

    pub async fn list_pending_interactions(
        &self,
        thread_id: &str,
    ) -> Result<Vec<InteractionRequest>> {
        let state = self.sessions().replay_thread(thread_id).await?;
        let mut pending = Vec::new();
        for id in state.permissions.keys().chain(state.interactions.keys()) {
            if let Some(interaction) =
                crate::thread_assembler::project_thread_interaction(thread_id, id, &state)?
                && interaction.status() == InteractionStatus::Pending
            {
                pending.push(interaction);
            }
        }
        pending.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.interaction_id.cmp(&left.interaction_id))
        });
        Ok(pending)
    }

    pub async fn list_threads_with_transient_pending_interactions(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        for id in self.sessions().session_ids().await? {
            if !self.list_pending_interactions(&id).await?.is_empty() {
                ids.push(id);
            }
        }
        Ok(ids)
    }
}
