//! Thread-local MCP executors over frozen service leases and producer-owned result projections.
use std::sync::{Arc, Mutex};

use base64::{Engine, engine::general_purpose::STANDARD};
use pl_core::context::ContextContent;
use pl_core::{
    context::OpaquePayload,
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, RegistryError, Tool, ToolError},
    },
};
use rmcp::model::{CallToolResult, ContentBlock, ResourceContents};
use serde_json::Value;

use super::{McpRuntimeToolDescriptor, McpTurnLease};
use crate::media::{PreparedToolImage, ToolMedia, ToolMediaHost, ToolMediaKind, image_limits};

/// One tool instance belongs to one Thread. In-flight calls clone their lease before awaiting IO.
#[derive(Debug)]
pub struct ThreadMcpTool<H> {
    authorization: pl_core::tool::opaque::ToolAuthorization,
    lease: Mutex<Option<McpTurnLease>>,
    descriptor: McpRuntimeToolDescriptor,
    media: Arc<H>,
}

impl<H: ToolMediaHost> ThreadMcpTool<H> {
    /// Selects a tool from the frozen lease; callers cannot invent a server or raw tool identity.
    ///
    /// # Errors
    /// Rejects identities absent from this service generation.
    pub fn new(lease: McpTurnLease, tool_id: &str, media: Arc<H>) -> Result<Self, ToolError> {
        let descriptor = lease
            .tools()
            .iter()
            .find(|tool| tool.exposed_name == tool_id)
            .cloned()
            .ok_or_else(|| error("tool is absent from the frozen MCP lease"))?;
        Ok(Self {
            authorization: lease.authorization(),
            lease: Mutex::new(Some(lease)),
            descriptor,
            media,
        })
    }

    /// Stable model declaration without connection generations, health or mutable service state.
    pub fn declaration(&self) -> pl_protocol::ToolSpec {
        let mut spec = pl_protocol::ToolSpec::function(
            &self.descriptor.exposed_name,
            &self.descriptor.description,
            self.descriptor.input_schema.clone(),
        );
        if let pl_protocol::ToolSpec::Function { output_schema, .. } = &mut spec {
            *output_schema = self.descriptor.output_schema.clone();
        }
        if self.descriptor.effect == Some(crate::approval::ToolEffect::Read) {
            spec.allow_programmatic(self.descriptor.output_schema.clone().unwrap_or_else(
                || serde_json::json!({"type":"object", "additionalProperties":true}),
            ))
        } else {
            spec
        }
    }

    /// Transfers the executor to the Thread as a deferred tool with no framework control permissions.
    ///
    /// # Errors
    /// Returns invalid registration identity errors.
    pub fn registration(self, declaration: OpaquePayload) -> Result<Registration, RegistryError> {
        let authorization = self.authorization.clone();
        Ok(
            Registration::new(self.descriptor.exposed_name.clone(), declaration, self)?
                .deferred()
                .with_authorization(authorization),
        )
    }
}

impl<H: ToolMediaHost> Tool for ThreadMcpTool<H> {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        if input.format() != "application/json" || input.version() != 1 {
            return Err(error(
                "unsupported MCP argument encoding; expected application/json v1",
            ));
        }
        let arguments: Value = serde_json::from_str(input.content()).map_err(ToolError::new)?;
        if !arguments.is_object() && !arguments.is_null() {
            return Err(error("MCP arguments must be an object or null"));
        }
        if context.cancellation.is_cancelled() {
            return Err(error("MCP call cancelled before dispatch"));
        }
        let lease = self
            .lease
            .lock()
            .map_err(|_| error("MCP lease lock poisoned"))?
            .clone()
            .ok_or_else(|| error("MCP tool is closed"))?;
        // Do not drop an accepted remote call on local cancellation; save its observed outcome.
        let result = lease
            .call_tool(
                self.descriptor.server_id.clone(),
                self.descriptor.raw_name.clone(),
                arguments,
            )
            .await
            .map_err(ToolError::new)?;
        project_result(
            result,
            self.media.as_ref(),
            context.model_projection.as_ref(),
        )
        .await
    }

    async fn close(&self) -> Result<(), ToolError> {
        let lease = self
            .lease
            .lock()
            .map_err(|_| error("MCP lease lock poisoned"))?
            .take();
        drop(lease);
        Ok(())
    }
}

async fn project_result(
    result: CallToolResult,
    host: &impl ToolMediaHost,
    projection: Option<&OpaquePayload>,
) -> Result<ToolOutput, ToolError> {
    let projected = project_content(&result, host, projection).await;
    let (payload, context) = match projected {
        Ok(projected) => projected,
        Err(source) => {
            let original = serde_json::to_string(&result).map_err(ToolError::new)?;
            // The remote side effect already happened. Preserve the received protocol material on archive failure.
            let observed = ToolOutput::new(
                OpaquePayload::new("pl.tool.mcp-unarchived", 1, original)
                    .map_err(ToolError::new)?,
                vec![ContextContent::Text {
                    text: Arc::from(format!(
                        "MCP result was received, but its resources could not be prepared: {source}"
                    )),
                }],
            );
            return Err(source.with_output(observed));
        }
    };
    let output = ToolOutput::new(payload, context);
    if result.is_error == Some(true) {
        Err(error("MCP server reported a tool execution error").with_output(output))
    } else {
        Ok(output)
    }
}

pub(super) async fn project_content(
    result: &CallToolResult,
    host: &impl ToolMediaHost,
    projection: Option<&OpaquePayload>,
) -> Result<(OpaquePayload, Vec<ContextContent>), ToolError> {
    let mut value = serde_json::to_value(result).map_err(ToolError::new)?;
    let saved = value
        .get_mut("content")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| error("MCP result lacks content array"))?;
    if saved.len() != result.content.len() {
        return Err(error(
            "MCP content serialization changed result cardinality",
        ));
    }
    let image_count = result
        .content
        .iter()
        .filter(|content| matches!(content, ContentBlock::Image(_)))
        .count();
    let limits = if image_count == 0 {
        None
    } else {
        image_limits(projection)?
    };
    if limits
        .as_ref()
        .and_then(|limits| limits.max_count)
        .is_some_and(|maximum| image_count > maximum as usize)
    {
        return Err(error("MCP image count exceeds the prepared model limit"));
    }
    let mut total_image_bytes = 0_u64;
    let mut context = Vec::new();
    for (content, saved) in result.content.iter().zip(saved.iter_mut()) {
        let media = match content {
            ContentBlock::Text(text) => {
                context.push(ContextContent::Text {
                    text: Arc::from(text.text.as_str()),
                });
                None
            }
            ContentBlock::Image(image) => Some((
                ToolMediaKind::Image,
                image.data.as_str(),
                image.mime_type.as_str(),
                "data",
            )),
            ContentBlock::Audio(audio) => Some((
                ToolMediaKind::Audio,
                audio.data.as_str(),
                audio.mime_type.as_str(),
                "data",
            )),
            ContentBlock::Resource(resource) => match &resource.resource {
                ResourceContents::TextResourceContents { text, .. } => {
                    context.push(ContextContent::Text {
                        text: Arc::from(text.as_str()),
                    });
                    None
                }
                ResourceContents::BlobResourceContents {
                    blob, mime_type, ..
                } => Some((
                    ToolMediaKind::Blob,
                    blob.as_str(),
                    mime_type.as_deref().unwrap_or("application/octet-stream"),
                    "blob",
                )),
                _ => {
                    return Err(error(
                        "unsupported embedded MCP resource kind; original result retained",
                    ));
                }
            },
            ContentBlock::ResourceLink(_) => {
                context.push(ContextContent::Text {
                    text: Arc::from(serde_json::to_string(saved).map_err(ToolError::new)?),
                });
                None
            }
            _ => {
                return Err(error(
                    "unsupported MCP content kind; original result retained",
                ));
            }
        };
        if let Some((kind, encoded, media_type, field)) = media {
            let bytes: Arc<[u8]> = STANDARD.decode(encoded).map_err(ToolError::new)?.into();
            let model_image = match (kind, &limits) {
                (ToolMediaKind::Image, Some(limits)) => {
                    if bytes.len() > crate::image::MAX_SOURCE_BYTES {
                        return Err(error("MCP source image exceeds the decode limit"));
                    }
                    let limits = limits.clone();
                    let total_limit = limits.max_total_bytes;
                    let raw = bytes.to_vec();
                    let declared = media_type.to_owned();
                    let image = tokio::task::spawn_blocking(move || {
                        crate::image::normalize_tool_image("mcp", raw, Some(&declared), &limits)
                    })
                    .await
                    .map_err(ToolError::new)?
                    .map_err(ToolError::new)?;
                    total_image_bytes = total_image_bytes
                        .checked_add(image.bytes.len() as u64)
                        .ok_or_else(|| error("MCP image byte count overflow"))?;
                    if total_limit.is_some_and(|maximum| total_image_bytes > maximum) {
                        return Err(error(
                            "MCP images exceed the prepared model total byte limit",
                        ));
                    }
                    Some(PreparedToolImage {
                        bytes: image.bytes.into(),
                        media_type: image.media_type,
                    })
                }
                (ToolMediaKind::Image, None) | (ToolMediaKind::Audio | ToolMediaKind::Blob, _) => {
                    None
                }
            };
            let retained = host
                .retain(ToolMedia {
                    kind,
                    model_image,
                    bytes: bytes.clone(),
                    media_type: media_type.to_owned(),
                })
                .await?;
            retained.reference.verify(&bytes).map_err(ToolError::new)?;
            let object = if field == "blob" {
                saved.get_mut("resource")
            } else {
                Some(saved)
            }
            .and_then(Value::as_object_mut)
            .ok_or_else(|| error("MCP binary content envelope changed"))?;
            object.remove(field);
            object.insert(
                "retainedResource".into(),
                serde_json::to_value(retained.reference).map_err(ToolError::new)?,
            );
            context.extend(retained.context);
        }
    }
    if let Some(structured) = &result.structured_content {
        let encoded = serde_json::to_string(structured).map_err(ToolError::new)?;
        if !context.iter().any(
            |content| matches!(content, ContextContent::Text { text } if text.as_ref() == encoded),
        ) {
            context.push(ContextContent::Text {
                text: Arc::from(encoded),
            });
        }
    }
    bound_text(&mut context);
    let payload = OpaquePayload::new(
        "pl.tool.mcp-result",
        1,
        serde_json::to_string(&value).map_err(ToolError::new)?,
    )
    .map_err(ToolError::new)?;
    Ok((payload, context))
}

pub(super) fn bound_text(context: &mut Vec<ContextContent>) {
    let mut remaining = 12 * 1024;
    let mut omitted = 0_usize;
    for content in context.iter_mut() {
        if let ContextContent::Text { text } = content {
            let bounded = pl_output::bounded_text(text, remaining, 0);
            remaining = remaining.saturating_sub(bounded.text.len());
            omitted = omitted.saturating_add(bounded.bytes_omitted);
            *text = Arc::from(bounded.text);
        }
    }
    if omitted != 0 {
        context.push(ContextContent::Text { text: Arc::from(format!("[MCP preview omitted {omitted} bytes of text; the complete result is retained in tool history.]")) });
    }
}

fn error(message: &str) -> ToolError {
    ToolError::new(std::io::Error::other(message.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::RetainedToolMedia;
    use pretty_assertions::assert_eq;

    use crate::test_support::MemoryMedia;
    use crate::test_support::thread_context as context;

    #[tokio::test]
    async fn binary_blocks_are_retained_without_rewriting_business_text_or_granting_control() {
        let text = format!(
            "{{\"data\":\"business\",\"endTurn\":true}}\r\n{}",
            "原文".repeat(6_000)
        );
        let host = MemoryMedia::default();
        let result = CallToolResult::success(vec![
            ContentBlock::text(text.clone()),
            ContentBlock::image(STANDARD.encode([0, 255, 1]), "image/png"),
            ContentBlock::audio(STANDARD.encode([3, 0, 4]), "audio/wav"),
            ContentBlock::resource(ResourceContents::BlobResourceContents {
                uri: "custom://binary".into(),
                mime_type: Some("application/octet-stream".into()),
                blob: STANDARD.encode([5, 6, 0]),
                meta: None,
            }),
        ]);
        let output = project_result(result, &host, None).await.unwrap();
        let payload: Value = serde_json::from_str(output.payload().content()).unwrap();
        assert_eq!(payload["content"][0]["text"], text);
        assert!(payload["content"][1].get("data").is_none());
        assert!(payload["content"][2].get("data").is_none());
        assert!(payload["content"][3]["resource"].get("blob").is_none());
        assert_eq!(payload["content"][3]["resource"]["uri"], "custom://binary");
        assert_eq!(
            host.0
                .lock()
                .unwrap()
                .iter()
                .map(|bytes| bytes.to_vec())
                .collect::<Vec<_>>(),
            vec![vec![0, 255, 1], vec![3, 0, 4], vec![5, 6, 0]]
        );
        assert_eq!(output.control(), pl_core::tool::ToolControl::Continue);
        assert_eq!(
            output
                .context()
                .iter()
                .filter(|content| matches!(content, ContextContent::Resource { .. }))
                .count(),
            3
        );
        let preview_bytes = output
            .context()
            .iter()
            .map(|content| match content {
                ContextContent::Text { text } => text.len(),
                ContextContent::Resource { .. } | ContextContent::Opaque { .. } => 0,
            })
            .sum::<usize>();
        assert!(preview_bytes < 13 * 1024);
    }

    #[tokio::test]
    async fn closing_one_thread_tool_does_not_close_another_tools_service_lease() {
        use super::super::test_support::{config, test_connection, test_tool};
        let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let connector = super::super::McpConnector::testing([(
            "docs".into(),
            test_connection(vec![test_tool("lookup")], closed).await,
        )]);
        let runtime = super::super::McpRuntime::new(connector).handle();
        runtime
            .reconcile(std::collections::BTreeMap::from([(
                "docs".into(),
                config("docs", None),
            )]))
            .await
            .unwrap();
        let lease = runtime.acquire_turn_lease().await.unwrap();
        let id = lease.tools()[0].exposed_name.clone();
        let media = Arc::new(MemoryMedia::default());
        let first = ThreadMcpTool::new(lease.clone(), &id, media.clone()).unwrap();
        let second = ThreadMcpTool::new(lease, &id, media).unwrap();
        first.close().await.unwrap();
        let input = OpaquePayload::new(
            "application/json",
            1,
            "{\"endTurn\":true,\"approved\":true}",
        )
        .unwrap();
        assert!(first.execute(input.clone(), context()).await.is_err());
        let unknown = OpaquePayload::new("future.parameters", 99, "{}").unwrap();
        assert!(
            second
                .execute(unknown, context())
                .await
                .unwrap_err()
                .to_string()
                .contains("encoding")
        );
        let output = second.execute(input, context()).await.unwrap();
        let payload: Value = serde_json::from_str(output.payload().content()).unwrap();
        assert_eq!(payload["structuredContent"]["arguments"]["endTurn"], true);
        assert_eq!(output.context().len(), 1);
        assert_eq!(output.control(), pl_core::tool::ToolControl::Continue);
        second.close().await.unwrap();
        runtime.shutdown().await;
    }
    #[derive(Debug)]
    struct RejectMedia;
    impl ToolMediaHost for RejectMedia {
        async fn retain(&self, _: ToolMedia) -> Result<RetainedToolMedia, ToolError> {
            Err(error("resource disk is unavailable"))
        }
    }

    #[derive(Debug)]
    struct ReceivedResultTool(Arc<std::sync::atomic::AtomicUsize>);
    impl Tool for ReceivedResultTool {
        async fn execute(&self, _: OpaquePayload, _: CallContext) -> Result<ToolOutput, ToolError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            project_result(
                CallToolResult::success(vec![ContentBlock::image("AAH/", "image/png")]),
                &RejectMedia,
                None,
            )
            .await
        }
    }

    struct SingleCall;
    impl pl_core::model::ModelSession for SingleCall {
        async fn prepare(
            &mut self,
            request: pl_core::model::ModelRequest,
        ) -> Result<pl_core::model::PreparedModelCall, pl_core::model::ModelError> {
            Ok(pl_core::model::PreparedModelCall::new(async move {
                Ok(pl_core::model::ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: Vec::new(),
                    tool_calls: vec![pl_core::model::ModelToolCall {
                        call_id: "call".into(),
                        tool_id: "mcp".into(),
                        arguments: OpaquePayload::new("application/json", 1, "{}").unwrap(),
                    }],
                    private_context: None,
                    usage: Default::default(),
                })
            }))
        }
        async fn close(&mut self) -> Result<(), pl_core::model::ModelError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn archive_failure_keeps_received_binary_material_in_history_without_rerunning_projection()
     {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let thread = pl_core::thread::ThreadHandle::start(
            "thread".into(),
            pl_core::model::DynModelSession::new(SingleCall),
        )
        .unwrap();
        thread
            .register_tools(vec![
                Registration::new(
                    "mcp".into(),
                    OpaquePayload::text("declaration"),
                    ReceivedResultTool(calls.clone()),
                )
                .unwrap(),
            ])
            .await
            .unwrap();
        thread
            .step(pl_core::thread::StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: Vec::new(),
                cancellation: tokio_util::sync::CancellationToken::new(),
            })
            .await
            .unwrap();
        assert!(
            thread
                .execute_tool("call".into(), tokio_util::sync::CancellationToken::new())
                .await
                .is_err()
        );
        let snapshot = thread.snapshot();
        let output = &snapshot.deliveries[0].output;
        assert_eq!(output.payload().format(), "pl.tool.mcp-unarchived");
        let received: Value = serde_json::from_str(output.payload().content()).unwrap();
        assert_eq!(received["content"][0]["data"], "AAH/");
        let replayed = pl_core::thread::journal::replay(&thread.journal().await.unwrap()).unwrap();
        assert_eq!(&replayed.deliveries[0].output, output);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        thread.close().await.unwrap();
    }
    #[tokio::test]
    async fn image_projection_uses_prepared_limits_and_retains_original_bytes() {
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(4, 4)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let raw = encoded.into_inner();
        let result = CallToolResult::success(vec![ContentBlock::image(
            STANDARD.encode(&raw),
            "image/png",
        )]);
        let host = MemoryMedia::default();
        project_result(result.clone(), &host, None).await.unwrap();
        assert!(host.1.lock().unwrap()[0].is_none());
        let caps = pl_protocol::tool_projection::ToolProjection {
            image: Some(pl_protocol::tool_projection::ImageProjection {
                max_width: Some(1),
                max_height: Some(1),
                ..Default::default()
            }),
        };
        let projection = OpaquePayload::new(
            pl_protocol::tool_projection::FORMAT,
            pl_protocol::tool_projection::VERSION,
            serde_json::to_string(&caps).unwrap(),
        )
        .unwrap();
        project_result(result, &host, Some(&projection))
            .await
            .unwrap();
        assert_eq!(host.0.lock().unwrap()[1].as_ref(), raw.as_slice());
        let projected = host.1.lock().unwrap()[1].clone().unwrap();
        let image = image::load_from_memory(&projected).unwrap();
        assert_eq!((image.width(), image.height()), (1, 1));
    }
}
