use super::BridgeThreadMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeStudioPromptInput {
    pub text: String,
    pub attachment_draft_ids: Vec<String>,
}

impl From<BridgeStudioPromptInput> for pl_protocol::studio::StudioPromptInput {
    fn from(value: BridgeStudioPromptInput) -> Self {
        Self {
            text: value.text,
            attachment_draft_ids: value.attachment_draft_ids,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeAttachmentAdmissionContext {
    ExistingThread { thread_id: String },
    NewThread { mode: BridgeThreadMode },
}

impl From<BridgeAttachmentAdmissionContext>
    for pl_protocol::studio::StudioAttachmentAdmissionContext
{
    fn from(value: BridgeAttachmentAdmissionContext) -> Self {
        match value {
            BridgeAttachmentAdmissionContext::ExistingThread { thread_id } => {
                Self::ExistingThread { thread_id }
            }
            BridgeAttachmentAdmissionContext::NewThread { mode } => Self::NewThread {
                mode: match mode {
                    BridgeThreadMode::Simple => "simple",
                    BridgeThreadMode::Task => "task",
                }
                .to_string(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeAttachmentDraftSource {
    LocalFile {
        path: String,
    },
    RemoteUrl {
        url: String,
        filename: Option<String>,
    },
}

impl From<BridgeAttachmentDraftSource> for pl_protocol::studio::StudioAttachmentDraftSource {
    fn from(value: BridgeAttachmentDraftSource) -> Self {
        match value {
            BridgeAttachmentDraftSource::LocalFile { path } => Self::LocalFile { path },
            BridgeAttachmentDraftSource::RemoteUrl { url, filename } => {
                Self::RemoteUrl { url, filename }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeAttachmentModality {
    Image,
    Video,
    File,
}

impl From<pl_protocol::studio::StudioAttachmentModality> for BridgeAttachmentModality {
    fn from(value: pl_protocol::studio::StudioAttachmentModality) -> Self {
        match value {
            pl_protocol::studio::StudioAttachmentModality::Image => Self::Image,
            pl_protocol::studio::StudioAttachmentModality::Video => Self::Video,
            pl_protocol::studio::StudioAttachmentModality::File => Self::File,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeAttachmentDraft {
    pub draft_id: String,
    pub modality: BridgeAttachmentModality,
    pub media_type: String,
    pub filename: String,
    pub byte_size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl From<pl_protocol::studio::StudioAttachmentDraft> for BridgeAttachmentDraft {
    fn from(value: pl_protocol::studio::StudioAttachmentDraft) -> Self {
        Self {
            draft_id: value.draft_id,
            modality: value.modality.into(),
            media_type: value.media_type,
            filename: value.filename,
            byte_size: value.byte_size,
            width: value.width,
            height: value.height,
        }
    }
}
