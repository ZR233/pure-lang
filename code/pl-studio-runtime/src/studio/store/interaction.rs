//! Product interactions are projected from current Thread checkpoints.
use crate::studio::store::StudioStore;
use crate::{InteractionRequest, InteractionStatus};
use anyhow::Result;

impl StudioStore {
    pub async fn read_interaction(
        &self,
        interaction_id: &str,
    ) -> Result<Option<InteractionRequest>> {
        let (thread_id, local_id) =
            crate::thread_assembler::decode_interaction_key(interaction_id)?;
        let Some(record) = self.read_thread_association(thread_id).await? else {
            return Ok(None);
        };
        let thread = pl_protocol::Thread::from(record);
        let Some(checkpoint) =
            crate::studio::thread_factory::recovery::load_checkpoint(self, &thread).await?
        else {
            return Ok(None);
        };
        Ok(crate::thread_assembler::project_thread_interaction(
            thread_id,
            local_id,
            &checkpoint.state,
        )?)
    }

    pub async fn list_pending_interactions(
        &self,
        thread_id: &str,
    ) -> Result<Vec<InteractionRequest>> {
        let Some(record) = self.read_thread_association(thread_id).await? else {
            return Ok(Vec::new());
        };
        let thread = pl_protocol::Thread::from(record);
        let Some(checkpoint) =
            crate::studio::thread_factory::recovery::load_checkpoint(self, &thread).await?
        else {
            return Ok(Vec::new());
        };
        let state = checkpoint.state;
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

    /// 恢复诊断：逐会话回读 checkpoint 取持久化的交互身份。
    ///
    /// 交互身份不在目录摘要中，且目录摘要的状态与 checkpoint 不保证同序耐久，因此这里不能用
    /// `catalog.toml` 的 status 过滤，仍需按目录身份回读 checkpoint；这是恢复边界而非目录查询。
    pub async fn list_threads_with_transient_pending_interactions(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        for entry in self.catalog().entries() {
            if !self.list_pending_interactions(&entry.id).await?.is_empty() {
                ids.push(entry.id);
            }
        }
        Ok(ids)
    }
}
