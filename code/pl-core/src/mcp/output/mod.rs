use std::path::PathBuf;

use base64::Engine;
use pl_model::{MediaRepresentation, ModelInfo, ModelInputCapability, ModelModality};
use pl_protocol::{PureError, Result, ThreadAttachment};
use rmcp::model::{CallToolResult, ContentBlock};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::tool::{
    OutputTruncation, ToolDirective, ToolResult, canonical_image_extension,
    maximum_base64_input_len, normalize_tool_image,
};
use crate::{AttachmentRuntime, ToolImageAttachmentInput};

/// 当前 Turn 接收 MCP typed image result 所需的完整模型与宿主边界。
#[derive(Debug, Clone)]
pub struct McpImageOutputContext {
    capability: ModelInputCapability,
    attachment_runtime: AttachmentRuntime,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct McpImageAuditMetadata {
    media_type: String,
    byte_size: u64,
    width: u32,
    height: u32,
    content_sha256: String,
}

impl McpImageOutputContext {
    /// 仅为明确支持 image 输入和持久快照重放的精确模型构造上下文。
    pub fn for_model(model: &ModelInfo, attachment_runtime: AttachmentRuntime) -> Option<Self> {
        if !attachment_runtime.supports_atomic_image_batch() {
            return None;
        }
        let capability = model.capabilities.input_capability(ModelModality::Image)?;
        let profile = model.request_profile.media_profile(ModelModality::Image)?;
        if capability.limits.max_count == Some(0)
            || profile.first_send.is_empty()
            || !profile.replay.contains(&MediaRepresentation::DataUrl)
        {
            return None;
        }
        Some(Self {
            capability: capability.clone(),
            attachment_runtime,
        })
    }
}

/// 把 rmcp typed result 转成统一工具输出，并在允许时原子接收图片附件。
pub(super) async fn call_tool_result_to_output(
    server_id: &str,
    raw_tool_name: &str,
    result: CallToolResult,
    image_output: Option<&McpImageOutputContext>,
) -> Result<ToolResult> {
    let is_error = result.is_error.unwrap_or(false);
    let image_count = result
        .content
        .iter()
        .filter(|content| matches!(content, ContentBlock::Image(_)))
        .count();
    let (attachments, image_audit, image_error) = if is_error || image_count == 0 {
        (Vec::new(), Vec::new(), None)
    } else if let Some(image_output) = image_output {
        match admit_images(raw_tool_name, &result.content, image_output).await {
            Ok((attachments, image_audit)) => (attachments, image_audit, None),
            Err(error) => (
                Vec::new(),
                Vec::new(),
                Some(bounded_diagnostic(&error.to_string())),
            ),
        }
    } else {
        (
            Vec::new(),
            Vec::new(),
            Some("the current model does not support durable MCP image output".to_string()),
        )
    };

    let sanitized_content = sanitize_content(
        &result.content,
        &attachments,
        &image_audit,
        is_error || image_error.is_some(),
    )?;
    let mut model_content = format_mcp_content(&sanitized_content);
    if let Some(error) = &image_error {
        if !model_content.is_empty() {
            model_content.push('\n');
        }
        model_content.push_str("MCP image output was omitted: ");
        model_content.push_str(error);
    }
    let audit = serde_json::json!({
        "type": "mcpCallResult",
        "server": server_id,
        "tool": raw_tool_name,
        "result": sanitized_result(&result, sanitized_content),
    });
    let description = if is_error {
        format!("Tool execution error: {model_content}")
    } else {
        model_content
    };
    let mut runtime_events = vec![ToolDirective::AuditMetadata { metadata: audit }];
    if is_error {
        runtime_events.push(ToolDirective::ExecutionFailed);
    }
    let mut output = ToolResult::from_runtime_text(
        description,
        OutputTruncation::empty(),
        PathBuf::new(),
        Some(if is_error { 1 } else { 0 }),
        false,
        runtime_events,
    );
    output.model_attachments = attachments;
    Ok(output)
}

async fn admit_images(
    raw_tool_name: &str,
    content: &[ContentBlock],
    image_output: &McpImageOutputContext,
) -> Result<(Vec<ThreadAttachment>, Vec<McpImageAuditMetadata>)> {
    let image_count = content
        .iter()
        .filter(|content| matches!(content, ContentBlock::Image(_)))
        .count();
    if let Some(max_count) = image_output.capability.limits.max_count
        && image_count > max_count as usize
    {
        return Err(image_error(
            raw_tool_name,
            format!(
                "returned {} images, exceeding the model limit of {max_count}",
                image_count
            ),
        ));
    }
    let mut images = Vec::with_capacity(image_count);
    for (index, image) in content
        .iter()
        .filter_map(|content| match content {
            ContentBlock::Image(image) => Some(image),
            _ => None,
        })
        .enumerate()
    {
        if image.data.len() > maximum_base64_input_len() {
            return Err(image_error(
                raw_tool_name,
                format!("image {} exceeds the encoded source limit", index + 1),
            ));
        }
        images.push((image.data.clone(), image.mime_type.clone()));
    }
    let capability = image_output.capability.clone();
    let tool_name = raw_tool_name.to_string();
    let inputs =
        tokio::task::spawn_blocking(move || normalize_images(&tool_name, images, &capability))
            .await
            .map_err(|error| image_error(raw_tool_name, format!("image task failed: {error}")))??;
    let audit = inputs
        .iter()
        .map(|input| McpImageAuditMetadata {
            media_type: input.media_type.clone(),
            byte_size: input.data.len() as u64,
            width: input.width,
            height: input.height,
            content_sha256: input.content_sha256.clone(),
        })
        .collect();
    let attachments = image_output.attachment_runtime.write_images(inputs).await?;
    Ok((attachments, audit))
}

fn normalize_images(
    raw_tool_name: &str,
    images: Vec<(String, String)>,
    capability: &ModelInputCapability,
) -> Result<Vec<ToolImageAttachmentInput>> {
    let mut normalized = Vec::with_capacity(images.len());
    let mut total_bytes = 0_u64;
    for (index, (data, declared_media_type)) in images.into_iter().enumerate() {
        if data.len() > maximum_base64_input_len() {
            return Err(image_error(
                raw_tool_name,
                format!("image {} exceeds the encoded source limit", index + 1),
            ));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|error| {
                image_error(
                    raw_tool_name,
                    format!("image {} is not strict Base64: {error}", index + 1),
                )
            })?;
        if bytes.len() > crate::tool::MAX_SOURCE_BYTES {
            return Err(image_error(
                raw_tool_name,
                format!("image {} exceeds the decoded source limit", index + 1),
            ));
        }
        let image =
            normalize_tool_image(raw_tool_name, bytes, Some(&declared_media_type), capability)?;
        total_bytes = total_bytes.saturating_add(image.bytes.len() as u64);
        if let Some(max_total_bytes) = capability.limits.max_total_bytes
            && total_bytes > max_total_bytes
        {
            return Err(image_error(
                raw_tool_name,
                format!("image batch exceeds the model limit of {max_total_bytes} bytes"),
            ));
        }
        let extension = canonical_image_extension(&image.media_type).ok_or_else(|| {
            image_error(raw_tool_name, "normalized image has no canonical extension")
        })?;
        normalized.push(ToolImageAttachmentInput {
            filename: format!(
                "{}-image-{}.{}",
                safe_tool_slug(raw_tool_name),
                index + 1,
                extension
            ),
            media_type: image.media_type,
            data: image.bytes,
            content_sha256: image.content_sha256,
            width: image.width,
            height: image.height,
        });
    }
    Ok(normalized)
}

fn sanitize_content(
    content: &[ContentBlock],
    attachments: &[ThreadAttachment],
    image_audit: &[McpImageAuditMetadata],
    images_omitted: bool,
) -> Result<Vec<Value>> {
    let mut attachment_index = 0;
    content
        .iter()
        .map(|content| {
            if let ContentBlock::Image(image) = content {
                let mut object = Map::from_iter([
                    ("type".to_string(), Value::String("image".to_string())),
                    (
                        "encodedLength".to_string(),
                        Value::from(image.data.len() as u64),
                    ),
                ]);
                if images_omitted {
                    object.insert(
                        "declaredMediaType".to_string(),
                        Value::String(image.mime_type.clone()),
                    );
                    object.insert("omitted".to_string(), Value::Bool(true));
                    object.insert(
                        "displayText".to_string(),
                        Value::String("[MCP image omitted]".to_string()),
                    );
                } else {
                    let attachment = attachments.get(attachment_index).ok_or_else(|| {
                        image_error("mcp", "MCP image attachment order is incomplete")
                    })?;
                    let audit = image_audit.get(attachment_index).ok_or_else(|| {
                        image_error("mcp", "MCP image audit metadata is incomplete")
                    })?;
                    attachment_index += 1;
                    object.insert(
                        "image".to_string(),
                        serde_json::to_value(audit).map_err(|error| {
                            image_error(
                                "mcp",
                                format!("failed to serialize image audit metadata: {error}"),
                            )
                        })?,
                    );
                    object.insert(
                        "attachment".to_string(),
                        serde_json::to_value(attachment).map_err(|error| {
                            image_error(
                                "mcp",
                                format!("failed to serialize attachment metadata: {error}"),
                            )
                        })?,
                    );
                    object.insert(
                        "displayText".to_string(),
                        Value::String(format!(
                            "[Image attachment: {}]",
                            attachment.filename.as_deref().unwrap_or("image")
                        )),
                    );
                }
                return Ok(Value::Object(object));
            }
            let mut value = serde_json::to_value(content).map_err(|error| {
                image_error("mcp", format!("failed to serialize MCP content: {error}"))
            })?;
            redact_binary_fields(&mut value);
            Ok(value)
        })
        .collect()
}

fn sanitized_result(result: &CallToolResult, content: Vec<Value>) -> Value {
    let mut value = Map::new();
    insert_serialized(&mut value, "resultType", result.result_type.as_ref());
    value.insert("content".to_string(), Value::Array(content));
    insert_serialized(
        &mut value,
        "structuredContent",
        result.structured_content.as_ref(),
    );
    insert_serialized(&mut value, "isError", result.is_error.as_ref());
    insert_serialized(&mut value, "_meta", result.meta.as_ref());
    Value::Object(value)
}

fn insert_serialized<T: Serialize>(map: &mut Map<String, Value>, key: &str, value: Option<&T>) {
    if let Some(value) = value
        && let Ok(mut value) = serde_json::to_value(value)
    {
        redact_binary_fields(&mut value);
        map.insert(key.to_string(), value);
    }
}

fn redact_binary_fields(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                redact_binary_fields(value);
            }
        }
        Value::Object(object) => {
            for field in ["data", "blob"] {
                if let Some(Value::String(encoded)) = object.remove(field) {
                    object.insert(format!("{field}Length"), Value::from(encoded.len() as u64));
                }
            }
            for value in object.values_mut() {
                redact_binary_fields(value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn format_mcp_content(content: &[Value]) -> String {
    content
        .iter()
        .map(format_mcp_content_part)
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_mcp_content_part(content: &Value) -> String {
    let Some(object) = content.as_object() else {
        return compact_json(content);
    };
    if let Some(display) = object.get("displayText").and_then(Value::as_str) {
        return display.to_string();
    }
    match object.get("type").and_then(Value::as_str) {
        Some("text") => object
            .get("text")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| compact_json(content)),
        Some("json") => object
            .get("json")
            .map(compact_json)
            .unwrap_or_else(|| compact_json(content)),
        _ => compact_json(content),
    }
}

fn safe_tool_slug(raw_tool_name: &str) -> String {
    let slug = raw_tool_name
        .chars()
        .take(48)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if slug.is_empty() {
        "mcp".to_string()
    } else {
        slug
    }
}

fn bounded_diagnostic(value: &str) -> String {
    const MAX_CHARS: usize = 512;
    if value.chars().count() <= MAX_CHARS {
        value.to_string()
    } else {
        format!(
            "{}...",
            value.chars().take(MAX_CHARS - 3).collect::<String>()
        )
    }
}

fn image_error(tool: &str, error: impl Into<String>) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.into(),
    }
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod unit_tests;
