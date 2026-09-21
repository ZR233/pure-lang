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
        let Some(current) = self.read_interaction_for_resolve(&interaction_id).await? else {
            // The owner and the checkpoint no longer retain this terminal record: adjudicate the
            // repeat from the durable receipt instead of reporting "not found", minting a new
            // revision or granting a new execution permission.
            return self
                .durable_interaction_receipt(&interaction_id, &resolution)
                .await;
        };
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

    /// Answers a repeated terminal interaction command from its durable receipt.
    ///
    /// The receipt payload is the exact committed core record, so a matching request returns the
    /// original receipt while a different resolution for the same identity is an explicit conflict.
    /// Nothing is re-executed, no permission is granted and no Turn is started: the durable record
    /// is read, projected through the same product mapping and compared only.
    async fn durable_interaction_receipt(
        &self,
        interaction_id: &str,
        resolution: &InteractionResolution,
    ) -> Result<StudioResolveInteractionResponse> {
        let (thread_id, local_id) =
            crate::thread_assembler::decode_interaction_key(interaction_id)?;
        let history = self.store.history(thread_id).await?;
        for kind in ["interaction", "permission"] {
            let Some(receipt) = history
                .latest_fact_receipt(&crate::studio::thread_projection::order::receipt_id(
                    kind, local_id,
                ))
                .await?
            else {
                continue;
            };
            let mut state = pl_core::thread::ThreadSnapshot::default();
            match receipt.kind.as_str() {
                "interaction" => {
                    state
                        .interactions
                        .insert(local_id.to_owned(), serde_json::from_str(&receipt.payload)?);
                }
                "permission" => {
                    state
                        .permissions
                        .insert(local_id.to_owned(), serde_json::from_str(&receipt.payload)?);
                }
                _ => continue,
            }
            let projected =
                crate::thread_assembler::project_thread_interaction(thread_id, local_id, &state)?
                    .context("durable interaction receipt cannot be projected")?;
            anyhow::ensure!(
                projected.resolution().as_ref() == Some(resolution),
                "interaction already has a different terminal resolution"
            );
            return Ok(StudioResolveInteractionResponse {
                thread_id: thread_id.to_owned(),
                interaction: projected,
            });
        }
        anyhow::bail!("interaction not found")
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
        let thread = self.read_protocol_thread(thread_id).await?;
        let Some(checkpoint) =
            crate::studio::thread_factory::recovery::load_checkpoint(&self.store, &thread).await?
        else {
            return Ok(None);
        };
        Ok(crate::thread_assembler::project_thread_interaction(
            thread_id,
            local_id,
            &checkpoint.state,
        )?)
    }
}
