//! Optional upload capability. Each dialect owns its wire contract, not the invocation loop.
use super::{FileUploadCapability, ProviderEndpoint};
use crate::completion::AttachmentInput;
use pl_protocol::Result;
use std::collections::HashMap;

pub(crate) struct FileUploadRequest<'a> {
    pub client: &'a reqwest::Client,
    pub endpoint: &'a ProviderEndpoint,
    pub model_headers: &'a HashMap<String, String>,
    pub input: &'a AttachmentInput,
    pub bytes: &'a [u8],
    pub now: i64,
}

pub(crate) struct UploadedFile {
    pub id: String,
    pub expires_at: i64,
}

impl FileUploadCapability {
    pub(crate) fn policy_key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::DeepSeek => "deepseek:user_data:86400",
        }
    }

    pub(crate) async fn upload(
        self,
        request: FileUploadRequest<'_>,
    ) -> Result<Option<UploadedFile>> {
        match self {
            Self::None => Ok(None),
            Self::DeepSeek => super::deepseek::upload_file(request).await,
        }
    }
}
