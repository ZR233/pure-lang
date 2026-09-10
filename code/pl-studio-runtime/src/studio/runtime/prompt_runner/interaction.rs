//! 交互读取、收束与重启后的 pending 交互恢复。

use anyhow::{Context, Result};

use crate::{InteractionRequest, InteractionResolution, InteractionStatus};

use super::super::{StudioResolveInteractionResponse, StudioRuntime};

impl StudioRuntime {
    pub async fn resolve_interaction(
        &self,
        interaction_id: String,
        resolution: InteractionResolution,
    ) -> Result<StudioResolveInteractionResponse> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let current = self
            .read_interaction_for_resolve(&interaction_id)
            .await?
            .context("interaction not found")?;
        if current.status() != InteractionStatus::Pending {
            anyhow::ensure!(
                current.resolution().as_ref() == Some(&resolution),
                "interaction already has a different terminal resolution"
            );
            return Ok(StudioResolveInteractionResponse {
                thread_id: current.scope.thread_id.clone(),
                interaction: current,
            });
        }
        self.ensure_thread_owner(&current.scope.thread_id).await?;
        let interaction = self
            .threads
            .resolve_product_interaction(&interaction_id, resolution)
            .await?;
        Ok(StudioResolveInteractionResponse {
            thread_id: interaction.scope.thread_id.clone(),
            interaction,
        })
    }

    /// 内存优先读取交互：pending 交互必须来自驻留 actor 的权威快照；
    /// 已离开快照的历史交互（非 pending）回 SQLite 冷源。
    pub(in crate::studio) async fn read_interaction_for_resolve(
        &self,
        interaction_id: &str,
    ) -> Result<Option<InteractionRequest>> {
        if let Some(interaction) = self.threads.read_product_interaction(interaction_id)? {
            return Ok(Some(interaction));
        }
        let (thread_id, local_id) =
            crate::thread_assembler::decode_interaction_key(interaction_id)?;
        let journal = self.store.sessions().read_thread_journal(thread_id).await?;
        if journal.is_empty() {
            return Ok(None);
        }
        let snapshot = pl_core::thread::journal::replay(&journal)?;
        Ok(crate::thread_assembler::project_thread_interaction(
            thread_id, local_id, &snapshot,
        )?)
    }
}
