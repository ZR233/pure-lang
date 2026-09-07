//! Cold interaction reads are owned by the core session database.
use crate::studio::store::StudioStore;
use crate::{InteractionRequest, InteractionStatus};
use anyhow::Result;

impl StudioStore {
    pub async fn read_interaction(
        &self,
        interaction_id: &str,
    ) -> Result<Option<InteractionRequest>> {
        Ok(self.sessions().read_interaction(interaction_id).await?)
    }

    pub async fn list_pending_interactions(
        &self,
        thread_id: &str,
    ) -> Result<Vec<InteractionRequest>> {
        let records = self
            .sessions()
            .read_entries(thread_id, Some("pl.interaction"))
            .await?;
        let mut pending = Vec::new();
        for record in records {
            let interaction: InteractionRequest = serde_json::from_value(record.payload)?;
            if interaction.status() == InteractionStatus::Pending {
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
