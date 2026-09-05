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
        let role = match context {
            StudioAttachmentAdmissionContext::ExistingThread { thread_id } => {
                let thread = self.read_owned_thread(thread_id).await?;
                StudioRole::from_key(&thread.role).context("Thread has an invalid model role")?
            }
            StudioAttachmentAdmissionContext::NewThread { mode } => {
                pl_protocol::ThreadModeId::from_label(mode)
                    .map_err(|_| anyhow::anyhow!("mode must be an available mode.* id"))?;
                StudioRole::Planner
            }
        };
        let config = self.config_runtime.read()?;
        let route = config.config.models.resolve(&role.id())?;
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
        self.store
            .read_attachment_bytes(&thread_id, &attachment_id)
            .await
    }
}
