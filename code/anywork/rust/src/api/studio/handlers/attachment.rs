use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::types::{
    BridgeAttachmentAdmissionContext, BridgeAttachmentDraft, BridgeAttachmentDraftSource,
    BridgeError,
};

pub async fn admit_attachment_drafts(
    context: BridgeAttachmentAdmissionContext,
    sources: Vec<BridgeAttachmentDraftSource>,
) -> Result<Vec<BridgeAttachmentDraft>, BridgeError> {
    let bridge = active_bridge().await?;
    let response = bridge
        .studio
        .admit_attachment_drafts(pl_protocol::studio::AdmitAttachmentDraftsRequest {
            context: context.into(),
            sources: sources.into_iter().map(Into::into).collect(),
        })
        .await?;
    Ok(response.drafts.into_iter().map(Into::into).collect())
}

pub async fn remove_attachment_draft(draft_id: String) -> Result<bool, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge.studio.remove_attachment_draft(draft_id).await?)
}

pub async fn read_attachment_draft(draft_id: String) -> Result<Vec<u8>, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge.studio.read_attachment_draft(draft_id).await?)
}

pub async fn read_thread_attachment(
    thread_id: String,
    attachment_id: String,
) -> Result<Vec<u8>, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge
        .studio
        .read_thread_attachment(thread_id, attachment_id)
        .await?)
}
