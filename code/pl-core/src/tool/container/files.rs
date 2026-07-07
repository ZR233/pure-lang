use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use pl_protocol::Result;
use serde::Deserialize;
use serde_json::{Value, json};

use super::backend::{ContainerBackend, ContainerCopyFromRequest, ContainerCopyToRequest};
use super::helpers::{parse_input, tool_error};
use super::schema::{TOOL_CONTAINER_CP_DOWNLOAD, TOOL_CONTAINER_CP_UPLOAD};

#[derive(Debug, Deserialize)]
struct CopyUploadInput {
    path: String,
    content_base64: String,
}

pub(super) async fn copy_upload<B>(backend: &B, arguments: Value) -> Result<Value>
where
    B: ContainerBackend,
{
    let input: CopyUploadInput = parse_input(arguments, TOOL_CONTAINER_CP_UPLOAD)?;
    let bytes = BASE64
        .decode(input.content_base64.trim().as_bytes())
        .map_err(|error| {
            tool_error(TOOL_CONTAINER_CP_UPLOAD, format!("invalid base64: {error}"))
        })?;
    backend
        .copy_to(ContainerCopyToRequest {
            path: input.path.clone(),
            content: bytes.clone(),
        })
        .await?;
    Ok(json!({ "path": input.path, "bytes": bytes.len() }))
}

#[derive(Debug, Deserialize)]
struct CopyDownloadInput {
    path: String,
}

pub(super) async fn copy_download<B>(backend: &B, arguments: Value) -> Result<Value>
where
    B: ContainerBackend,
{
    let input: CopyDownloadInput = parse_input(arguments, TOOL_CONTAINER_CP_DOWNLOAD)?;
    let bytes = backend
        .copy_from(ContainerCopyFromRequest {
            path: input.path.clone(),
            archive: true,
        })
        .await?;
    Ok(json!({
        "path": input.path,
        "tar_base64": BASE64.encode(bytes),
    }))
}
