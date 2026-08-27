use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum StudioAttachmentModality {
    Image,
    Video,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", tag = "type", deny_unknown_fields)]
pub enum StudioAttachmentDraftSource {
    LocalFile {
        path: String,
    },
    RemoteUrl {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", tag = "type", deny_unknown_fields)]
pub enum StudioAttachmentAdmissionContext {
    ExistingThread { thread_id: String },
    NewThread { mode: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmitAttachmentDraftsRequest {
    pub context: StudioAttachmentAdmissionContext,
    pub sources: Vec<StudioAttachmentDraftSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StudioAttachmentDraft {
    pub draft_id: String,
    pub modality: StudioAttachmentModality,
    pub media_type: String,
    pub filename: String,
    pub byte_size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdmitAttachmentDraftsResponse {
    pub drafts: Vec<StudioAttachmentDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoveAttachmentDraftRequest {
    pub draft_id: String,
}
