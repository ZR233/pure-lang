use std::io::Cursor;
use std::sync::{Arc, Mutex};

use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};
use pl_protocol::AttachmentModality;
use pretty_assertions::assert_eq;
use rmcp::model::ContentBlock;

use super::*;

fn model(slug: &str) -> ModelInfo {
    pl_model::model::default_models()
        .into_iter()
        .find(|model| model.slug == slug)
        .expect("bundled model")
}

fn png(color: [u8; 3]) -> Vec<u8> {
    let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(8, 4, Rgb(color)));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn runtime(writes: Arc<Mutex<Vec<Vec<ToolImageAttachmentInput>>>>) -> AttachmentRuntime {
    AttachmentRuntime::new_batch(
        move |inputs| {
            let writes = writes.clone();
            async move {
                let attachments = inputs
                    .iter()
                    .enumerate()
                    .map(|(index, input)| ThreadAttachment {
                        id: format!("attachment-{index}"),
                        modality: AttachmentModality::Image,
                        media_type: input.media_type.clone(),
                        filename: Some(input.filename.clone()),
                        width: Some(input.width),
                        height: Some(input.height),
                        byte_size: input.data.len() as u64,
                    })
                    .collect::<Vec<_>>();
                writes.lock().unwrap().push(inputs);
                Ok(attachments)
            }
        },
        |_| async { Ok(Vec::new()) },
    )
}

#[test]
fn mcp_images_require_atomic_attachment_writer() {
    let runtime = AttachmentRuntime::new(
        |_input| async {
            Err(PureError::ConfigError(
                "single-image writer must not be used".to_string(),
            ))
        },
        |_ids| async { Ok(Vec::new()) },
    );

    assert!(
        McpImageOutputContext::for_model(&model("deepseek-v4-flash-vision-exp"), runtime,)
            .is_none()
    );
}

fn image_result(images: Vec<Vec<u8>>) -> CallToolResult {
    CallToolResult::success(
        std::iter::once(ContentBlock::text("generated images"))
            .chain(images.into_iter().map(|bytes| {
                ContentBlock::image(
                    base64::engine::general_purpose::STANDARD.encode(bytes),
                    "image/png",
                )
            }))
            .collect(),
    )
}

fn audit_json(output: &ToolResult) -> String {
    output
        .runtime_events
        .iter()
        .find_map(|event| match event {
            ToolDirective::AuditMetadata { metadata } => {
                Some(serde_json::to_string(metadata).unwrap())
            }
            _ => None,
        })
        .expect("audit metadata")
}

#[tokio::test]
async fn image_capable_model_persists_ordered_batch_without_base64() {
    let first = png([240, 30, 40]);
    let second = png([30, 40, 240]);
    let first_base64 = base64::engine::general_purpose::STANDARD.encode(&first);
    let writes = Arc::new(Mutex::new(Vec::new()));
    let context = McpImageOutputContext::for_model(
        &model("deepseek-v4-flash-vision-exp"),
        runtime(writes.clone()),
    )
    .expect("image output context");

    let output = call_tool_result_to_output(
        "images",
        "generate_image",
        image_result(vec![first, second]),
        Some(&context),
    )
    .await
    .unwrap();

    assert_eq!(output.model_attachments.len(), 2);
    assert_eq!(
        output.model_attachments[0].filename.as_deref(),
        Some("generate_image-image-1.png")
    );
    assert_eq!(
        output.model_attachments[1].filename.as_deref(),
        Some("generate_image-image-2.png")
    );
    assert_eq!(writes.lock().unwrap().len(), 1);
    assert!(!output.canonical_output().contains(&first_base64));
    assert!(!audit_json(&output).contains(&first_base64));
    assert!(audit_json(&output).contains("contentSha256"));
    assert!(output.canonical_output().contains("Image attachment"));
}

#[tokio::test]
async fn missing_image_capability_returns_diagnostic_without_write() {
    let output = call_tool_result_to_output(
        "images",
        "generate_image",
        image_result(vec![png([1, 2, 3])]),
        None,
    )
    .await
    .unwrap();

    assert!(output.model_attachments.is_empty());
    assert!(output.canonical_output().contains("current model"));
}

#[tokio::test]
async fn invalid_image_rejects_entire_batch_before_writer() {
    let writes = Arc::new(Mutex::new(Vec::new()));
    let context = McpImageOutputContext::for_model(
        &model("deepseek-v4-flash-vision-exp"),
        runtime(writes.clone()),
    )
    .unwrap();
    let result = CallToolResult::success(vec![
        ContentBlock::image(
            base64::engine::general_purpose::STANDARD.encode(png([1, 2, 3])),
            "image/png",
        ),
        ContentBlock::image("not-base64", "image/png"),
    ]);

    let output = call_tool_result_to_output("images", "generate_image", result, Some(&context))
        .await
        .unwrap();

    assert!(output.model_attachments.is_empty());
    assert!(output.canonical_output().contains("not strict Base64"));
    assert!(writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn spoofed_image_mime_rejects_entire_batch_before_writer() {
    let writes = Arc::new(Mutex::new(Vec::new()));
    let context = McpImageOutputContext::for_model(
        &model("deepseek-v4-flash-vision-exp"),
        runtime(writes.clone()),
    )
    .unwrap();
    let result = CallToolResult::success(vec![ContentBlock::image(
        base64::engine::general_purpose::STANDARD.encode(png([1, 2, 3])),
        "image/jpeg",
    )]);

    let output = call_tool_result_to_output("images", "generate_image", result, Some(&context))
        .await
        .unwrap();

    assert!(output.model_attachments.is_empty());
    assert!(
        output
            .canonical_output()
            .contains("does not match detected")
    );
    assert!(writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn error_result_never_decodes_or_persists_images() {
    let writes = Arc::new(Mutex::new(Vec::new()));
    let context = McpImageOutputContext::for_model(
        &model("deepseek-v4-flash-vision-exp"),
        runtime(writes.clone()),
    )
    .unwrap();
    let result = CallToolResult::error(vec![ContentBlock::image("not-base64", "image/png")]);

    let output = call_tool_result_to_output("images", "generate_image", result, Some(&context))
        .await
        .unwrap();

    assert!(!output.success);
    assert!(output.model_attachments.is_empty());
    assert!(writes.lock().unwrap().is_empty());
    assert!(!audit_json(&output).contains("not-base64"));
}
