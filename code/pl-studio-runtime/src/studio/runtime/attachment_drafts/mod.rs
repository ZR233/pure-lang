//! 附件草稿对外 API 适配层：解析准入上下文对应的模型路由，并把 preflight、准入、读取与移除请求转发给草稿运行时。

mod normalize;
mod runtime;
mod source;
mod validate;

pub(super) use runtime::AttachmentDraftRuntime;

use anyhow::{Context, Result};
use pl_model::model::ModelInfo;
use pl_protocol::studio::{
    AdmitAttachmentDraftsRequest, AdmitAttachmentDraftsResponse, StudioAttachmentAdmissionContext,
};

use crate::config::StudioRole;
use crate::resource_store::{FileResourceStore, RESOURCE_ID_PREFIX};

use super::StudioRuntime;
use validate::{admission_rejection, preflight_sources};

impl StudioRuntime {
    pub async fn preflight_attachment_drafts(
        &self,
        request: &AdmitAttachmentDraftsRequest,
    ) -> Result<()> {
        let model = self.attachment_model_for_context(&request.context).await?;
        preflight_sources(&request.sources, &model)
            .map(|_| ())
            .map_err(admission_rejection)
    }

    pub async fn admit_attachment_drafts(
        &self,
        request: AdmitAttachmentDraftsRequest,
    ) -> Result<AdmitAttachmentDraftsResponse> {
        let model = self.attachment_model_for_context(&request.context).await?;
        self.attachment_drafts.admit(request.sources, &model).await
    }

    async fn attachment_model_for_context(
        &self,
        context: &StudioAttachmentAdmissionContext,
    ) -> Result<ModelInfo> {
        let config = self.config_runtime.read()?;
        let route = match context {
            StudioAttachmentAdmissionContext::ExistingThread { thread_id } => {
                let thread = self.read_owned_thread(thread_id).await?;
                let state = self.read_thread_state(thread_id).await?;
                let (_, selector) = crate::studio::model_route::route_record(
                    &state,
                    thread.parent_thread_id.is_some(),
                )?
                .context("Thread has no saved model route")?;
                let role = if thread.parent_thread_id.is_some() {
                    pl_protocol::AgentRoleId::new(thread.role)?
                } else {
                    StudioRole::Planner.id()
                };
                config.config.models.resolve_route(role, &selector)?
            }
            StudioAttachmentAdmissionContext::NewThread { mode } => {
                let mode = pl_protocol::ThreadModeId::from_label(mode)
                    .map_err(|_| anyhow::anyhow!("mode must be an available mode.* id"))?;
                config.config.resolve_mode_model_route(&mode)?
            }
        };
        Ok(route.model)
    }

    pub async fn remove_attachment_draft(&self, draft_id: String) -> Result<bool> {
        self.attachment_drafts.remove(&draft_id).await
    }

    pub async fn read_attachment_draft(&self, draft_id: String) -> Result<Vec<u8>> {
        self.attachment_drafts.read(&draft_id).await
    }

    pub async fn read_thread_attachment(
        &self,
        thread_id: String,
        attachment_id: String,
    ) -> Result<Vec<u8>> {
        self.read_owned_thread(&thread_id).await?;
        if attachment_id.starts_with(RESOURCE_ID_PREFIX) {
            let facts = self.read_thread_state(&thread_id).await?;
            let mut deliveries = facts.deliveries.to_vec();
            // A completed delivery leaves the resident snapshot. Read pending effects first:
            // once the writer releases an effect from this window, its full delivery is already
            // committed in history. The following history read therefore closes that race.
            if let Some((_, handle)) = self
                .threads
                .observed_threads()
                .into_iter()
                .find(|(id, _)| id == &thread_id)
            {
                for effect in handle.effects().await? {
                    deliveries.extend(effect.deliveries.iter().cloned());
                }
            }
            deliveries.extend(
                self.store
                    .history(&thread_id)
                    .await?
                    .tool_media_deliveries(&attachment_id)
                    .await?,
            );
            let store = FileResourceStore::new(self.store.session_resources_dir(&thread_id));
            return crate::studio::thread_projection::read_persisted_media(
                &store,
                &deliveries,
                &attachment_id,
            )
            .await;
        }
        self.store
            .read_attachment_bytes(&thread_id, &attachment_id)
            .await
    }
}
