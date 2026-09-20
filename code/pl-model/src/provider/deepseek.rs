//! Native DeepSeek access. Completion uses the same lifecycle and accounting as routed calls.
use crate::completion::{CompletionFailure, CompletionRequest, CompletionResponse};
use crate::runtime::{InvocationRunner, ModelInvocationContext};

/// Concrete DeepSeek client, obtained from the resolved runtime's provider enum.
#[derive(Debug, Clone)]
pub struct DeepSeekClient<'a> {
    pub(crate) runner: &'a InvocationRunner,
}

/// DeepSeek thinking mode.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeepSeekThinking {
    #[default]
    Enabled,
    Disabled,
}

/// DeepSeek-specific inference controls.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct DeepSeekCompletionOptions {
    pub thinking: DeepSeekThinking,
}

/// Common completion input combined with typed native settings.
#[derive(Debug, Clone)]
pub struct DeepSeekCompletion {
    pub request: CompletionRequest,
    pub options: DeepSeekCompletionOptions,
}

impl DeepSeekClient<'_> {
    /// Executes a native request with shared cancellation, retries and final accounting.
    ///
    /// # Errors
    /// Returns a typed failure retaining any usage reported before the failure.
    pub async fn complete(
        &self,
        input: DeepSeekCompletion,
        context: ModelInvocationContext,
    ) -> Result<CompletionResponse, CompletionFailure> {
        let body = super::clients::native_body(input.options)?;
        self.runner
            .with_native_body(body)
            .complete(input.request, context)
            .await
    }
}

pub(crate) async fn upload_file(
    request: super::files::FileUploadRequest<'_>,
) -> pl_protocol::Result<Option<super::files::UploadedFile>> {
    use crate::runtime::transport;
    let part = reqwest::multipart::Part::bytes(request.bytes.to_vec())
        .file_name(
            request
                .input
                .filename
                .clone()
                .unwrap_or_else(|| "attachment".into()),
        )
        .mime_str(&request.input.media_type)
        .map_err(transport::reqwest_error_to_pure)?;
    let form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("purpose", "user_data")
        .text("expires_after[anchor]", "created_at")
        .text("expires_after[seconds]", "86400");
    let headers = transport::headers(
        request.endpoint.bearer_token.as_deref(),
        request.endpoint.http_headers.as_ref(),
        request.model_headers,
    )?;
    let response = request
        .client
        .post(format!(
            "{}/files",
            request.endpoint.base_url.trim_end_matches('/')
        ))
        .headers(headers)
        .multipart(form)
        .send()
        .await
        .map_err(transport::reqwest_error_to_pure)?;
    if matches!(response.status().as_u16(), 404 | 405 | 501) {
        return Ok(None);
    }
    let response = transport::checked(response).await?;
    #[derive(serde::Deserialize)]
    struct File {
        id: String,
        expires_at: Option<i64>,
    }
    let file: File = response
        .json()
        .await
        .map_err(transport::reqwest_error_to_pure)?;
    if file.id.is_empty() {
        return Err(pl_protocol::PureError::Protocol(
            "upload response has no file id".into(),
        ));
    }
    Ok(Some(super::files::UploadedFile {
        id: file.id,
        expires_at: file.expires_at.unwrap_or(request.now.saturating_add(86400)),
    }))
}
