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

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::thread::input::{InputDelivery, InputState, ThreadInput};
    use pretty_assertions::assert_eq;

    fn input(payload: OpaquePayload) -> InputRecord {
        InputRecord {
            accepted_sequence: 1,
            delivery: InputDelivery::NextTurn,
            ordinal: 1,
            revision: 1,
            state: InputState::Pending,
            input: ThreadInput {
                id: "input".into(),
                payload,
                context: vec![ContextContent::Text {
                    text: "actual text\r\n".into(),
                }],
            },
        }
    }

    #[test]
    fn saved_prompt_display_is_independent_from_model_attachment_annotations() {
        let payload = OpaquePayload::new(
            "pl.studio.prompt",
            1,
            r#"{"text":"  原文\r\n","presentation":"visible","attachments":[]}"#,
        )
        .unwrap();
        let projected = input_content(&input(payload.clone())).unwrap();
        assert_eq!(projected.text, "  原文\r\n");
        assert_eq!(projected.original, payload);
        assert_eq!(projected.presentation, MessagePresentation::Visible);
    }

    #[test]
    fn hidden_continuation_and_unknown_payload_remain_distinct() {
        let payload = OpaquePayload::new(
            "pl.studio.interaction-continuation",
            1,
            r#"{"interactionId":"question","presentation":"hidden"}"#,
        )
        .unwrap();
        let projected = input_content(&input(payload)).unwrap();
        assert_eq!(projected.text, "actual text\r\n");
        assert_eq!(projected.presentation, MessagePresentation::Hidden);
        let unknown = input(OpaquePayload::new("future.input", 8, "unknown\n").unwrap());
        assert!(input_content(&unknown).is_err());
        assert_eq!(unknown.input.payload.content(), "unknown\n");
    }
}
