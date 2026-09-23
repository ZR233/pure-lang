//! Text and attachment projection from immutable input facts, independent from current tool renderers.
use pl_core::{
    context::{ContextContent, OpaquePayload},
    thread::input::InputRecord,
};
use pl_protocol::{MessagePresentation, ThreadAttachment};
use serde::Deserialize;

#[derive(Debug, PartialEq)]
pub(super) struct InputContent {
    pub text: String,
    pub attachments: Vec<ThreadAttachment>,
    pub presentation: MessagePresentation,
    pub original: OpaquePayload,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum ContentError {
    #[error("invalid saved product input")]
    Decode(#[from] serde_json::Error),
    #[error("unsupported saved product input {format} version {version}")]
    Unsupported { format: String, version: u32 },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Prompt {
    text: String,
    presentation: MessagePresentation,
    attachments: Vec<crate::studio::AttachmentRecord>,
    // Admission metadata is validated but does not replace the saved display content.
    #[serde(rename = "request")]
    _request: Option<pl_protocol::studio::StudioPromptInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Continuation {
    interaction_id: String,
    presentation: MessagePresentation,
}

/// Produces display content from the input's saved product metadata. Unknown formats are errors;
/// callers retain the original payload for a generic historical record view.
pub(super) fn input_content(input: &InputRecord) -> Result<InputContent, ContentError> {
    let payload = &input.input.payload;
    if payload.version() != 1 {
        return Err(ContentError::Unsupported {
            format: payload.format().into(),
            version: payload.version(),
        });
    }
    match payload.format() {
        "pl.studio.prompt" => {
            let prompt: Prompt = serde_json::from_str(payload.content())?;
            Ok(InputContent {
                text: prompt.text,
                presentation: prompt.presentation,
                attachments: prompt
                    .attachments
                    .iter()
                    .map(crate::studio::store::attachment::thread_attachment)
                    .collect(),
                original: payload.clone(),
            })
        }
        "pl.studio.interaction-continuation" => {
            let continuation: Continuation = serde_json::from_str(payload.content())?;
            if continuation.interaction_id.is_empty() {
                return Err(ContentError::Unsupported {
                    format: payload.format().into(),
                    version: payload.version(),
                });
            }
            Ok(InputContent {
                text: text_content(&input.input.context),
                presentation: continuation.presentation,
                attachments: Vec::new(),
                original: payload.clone(),
            })
        }
        _ => Err(ContentError::Unsupported {
            format: payload.format().into(),
            version: payload.version(),
        }),
    }
}

/// Attachment identities the current state still references through its accepted inputs.
///
/// Publishing a checkpoint that names a blob must not outrun that blob's durability, so this is the
/// fence's input: it is derived from the same saved payloads the projection reads, not from the
/// order in which drafts happened to be promoted. Inputs of an unknown format reference nothing.
pub(in crate::studio) fn referenced_attachment_ids(
    state: &pl_core::thread::ThreadSnapshot,
) -> Vec<String> {
    let mut ids = std::collections::BTreeSet::new();
    for record in state.inputs.iter() {
        let Ok(content) = input_content(record) else {
            continue;
        };
        for attachment in content.attachments {
            ids.insert(attachment.id);
        }
    }
    ids.into_iter().collect()
}

/// Concatenates actual text chunks without trimming, inserting separators or parsing opaque content.
pub(super) fn text_content(content: &[ContextContent]) -> String {
    content
        .iter()
        .filter_map(|part| match part {
            ContextContent::Text { text } => Some(text.as_ref()),
            ContextContent::Resource { .. } | ContextContent::Opaque { .. } => None,
        })
        .collect()
}

/// Host 提交身份的稳定摘要，用于跨重启校验重复提交的正文。
///
/// 摘要只覆盖幂等受理真正比对的字段（原始 request 与 presentation），因此可以从保存的载荷
/// 重新计算；它不覆盖框架路由回执、附件解析结果、序列号或展示正文。
pub(in crate::studio) fn prompt_request_digest(
    request: &pl_protocol::studio::StudioPromptInput,
    presentation: MessagePresentation,
) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"pl.studio.prompt-request\0");
    hasher.update(serde_json::to_vec(request).unwrap_or_default());
    hasher.update([0]);
    hasher.update(match presentation {
        MessagePresentation::Visible => b"visible".as_slice(),
        MessagePresentation::Hidden => b"hidden".as_slice(),
    });
    format!("sha256:{:x}", hasher.finalize())
}

/// 从已保存的 `pl.studio.prompt` 载荷重建 host 提交身份摘要。
///
/// 返回 `None` 表示这份载荷无法证明正文身份（格式/版本不符、缺少原始 request 或结构未知）。
/// 调用方必须把无法校验的持久化幂等命中当作身份冲突，不能退回只按 ID 命中的 no-op。
pub(in crate::studio) fn saved_prompt_request_digest(payload: &OpaquePayload) -> Option<String> {
    if payload.format() != "pl.studio.prompt" || payload.version() != 1 {
        return None;
    }
    let prompt: Prompt = serde_json::from_str(payload.content()).ok()?;
    Some(prompt_request_digest(
        prompt._request.as_ref()?,
        prompt.presentation,
    ))
}
