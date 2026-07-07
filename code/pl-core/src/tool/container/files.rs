use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use pl_protocol::Result;
use serde::Deserialize;
use serde_json::{Value, json};

use super::backend::{ContainerBackend, ContainerCopyFromRequest, ContainerCopyToRequest};
use super::helpers::{parse_input, tool_error};
use super::schema::TOOL_CONTAINER_COPY;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CopyInput {
    direction: CopyDirection,
    path: String,
    content_base64: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum CopyDirection {
    Upload,
    Download,
}

pub(super) async fn copy_container<B>(backend: &B, arguments: Value) -> Result<Value>
where
    B: ContainerBackend,
{
    let input: CopyInput = parse_input(arguments, TOOL_CONTAINER_COPY)?;
    match input.direction {
        CopyDirection::Upload => copy_upload(backend, input).await,
        CopyDirection::Download => copy_download(backend, input).await,
    }
}

async fn copy_upload<B>(backend: &B, input: CopyInput) -> Result<Value>
where
    B: ContainerBackend,
{
    let content_base64 = input.content_base64.ok_or_else(|| {
        tool_error(
            TOOL_CONTAINER_COPY,
            "contentBase64 is required when direction is upload",
        )
    })?;
    let bytes = BASE64
        .decode(content_base64.trim().as_bytes())
        .map_err(|error| tool_error(TOOL_CONTAINER_COPY, format!("invalid base64: {error}")))?;
    backend
        .copy_to(ContainerCopyToRequest {
            path: input.path.clone(),
            content: bytes.clone(),
        })
        .await?;
    Ok(json!({ "direction": "upload", "path": input.path, "bytes": bytes.len() }))
}

async fn copy_download<B>(backend: &B, input: CopyInput) -> Result<Value>
where
    B: ContainerBackend,
{
    let bytes = backend
        .copy_from(ContainerCopyFromRequest {
            path: input.path.clone(),
            archive: true,
        })
        .await?;
    Ok(json!({
        "direction": "download",
        "path": input.path,
        "tarBase64": BASE64.encode(bytes),
    }))
}
