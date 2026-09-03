use pl_protocol::Result;

use crate::completion::CompletionRequest;
#[cfg(test)]
use crate::completion::CompletionResponse;
use crate::model::info::ModelInfo;

mod client_config;
mod identity;
mod request;
#[cfg(test)]
mod response;
pub(crate) mod sse;
mod usage;

pub(crate) use client_config::PureOpenAiConfig;
pub(crate) use request::OpenAiRequestBody;
use request::build_openai_request_body;
#[cfg(test)]
use response::{chat_parse_response, responses_parse_response};

/// OpenAI API 协议端点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenAiEndpoint {
    Responses,
    ChatCompletions,
}

/// OpenAI 协议编解码器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpenAiProtocol {
    endpoint: OpenAiEndpoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VisibleOutputProtocol {
    NativePhases,
    TaggedText,
}

impl OpenAiProtocol {
    pub(crate) fn responses() -> Self {
        Self {
            endpoint: OpenAiEndpoint::Responses,
        }
    }

    pub(crate) fn chat() -> Self {
        Self {
            endpoint: OpenAiEndpoint::ChatCompletions,
        }
    }

    pub(crate) fn build_request(
        &self,
        request: &CompletionRequest,
        model: &ModelInfo,
        prompt_cache_key: Option<&str>,
    ) -> Result<OpenAiRequestBody> {
        build_openai_request_body(self.endpoint, request, model, prompt_cache_key)
    }

    #[cfg(test)]
    fn build_request_body(&self, request: &CompletionRequest) -> serde_json::Value {
        let fallback = ModelInfo::fallback("test-model");
        self.build_request_body_with_model(request, &fallback)
    }

    #[cfg(test)]
    fn build_request_body_with_model(
        &self,
        request: &CompletionRequest,
        model: &ModelInfo,
    ) -> serde_json::Value {
        serde_json::to_value(
            self.build_request(request, model, None)
                .expect("typed provider request should build"),
        )
        .expect("typed provider request should serialize")
    }

    #[cfg(test)]
    fn parse_response(&self, body: serde_json::Value) -> Result<CompletionResponse> {
        match self.endpoint {
            OpenAiEndpoint::Responses => responses_parse_response(body),
            OpenAiEndpoint::ChatCompletions => chat_parse_response(body),
        }
    }

    pub(crate) fn new_stream_decoder(&self) -> sse::OpenAiStreamDecoder {
        sse::OpenAiStreamDecoder::new(self.visible_output_protocol())
    }

    pub(crate) fn visible_output_protocol(&self) -> VisibleOutputProtocol {
        match self.endpoint {
            OpenAiEndpoint::Responses => VisibleOutputProtocol::NativePhases,
            OpenAiEndpoint::ChatCompletions => VisibleOutputProtocol::TaggedText,
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::collections::HashMap;

    use pl_protocol::{
        AttachmentModality, ContentPart, Message, MessageContent, MessageRole, ModelContextItem,
    };

    use crate::completion::{CompletionRequest, ReasoningConfig};
    use crate::model::info::ModelInfo;

    pub(crate) fn text_message(role: MessageRole, content: &str) -> Message {
        Message {
            presentation: Default::default(),
            role,
            content: MessageContent::text(content.to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
            metadata: HashMap::new(),
        }
    }

    pub(crate) fn image_message() -> Message {
        Message {
            presentation: Default::default(),
            role: MessageRole::User,
            content: MessageContent::new(vec![
                ContentPart::Text {
                    text: "describe".to_string(),
                },
                ContentPart::Attachment {
                    attachment_id: "attachment-1".to_string(),
                    modality: AttachmentModality::Image,
                    media_type: "image/png".to_string(),
                    filename: Some("sample.png".to_string()),
                },
            ]),
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
            metadata: HashMap::new(),
        }
    }

    pub(crate) fn image_prepared_content() -> Vec<crate::completion::PreparedContentPart> {
        vec![crate::completion::PreparedContentPart {
            attachment_id: "attachment-1".to_string(),
            modality: AttachmentModality::Image,
            media_type: "image/png".to_string(),
            filename: Some("sample.png".to_string()),
            sources: vec![crate::completion::PreparedContentSource::DataUrl {
                base64: "aGVsbG8=".to_string(),
            }],
        }]
    }

    pub(crate) fn context_items(messages: Vec<Message>) -> Vec<ModelContextItem> {
        messages.into_iter().map(ModelContextItem::from).collect()
    }

    pub(crate) fn request_with_effort(effort: &str) -> CompletionRequest {
        CompletionRequest::builder()
            .input(context_items(vec![text_message(
                MessageRole::User,
                "hello",
            )]))
            .parallel_tool_calls(true)
            .reasoning(Some(ReasoningConfig {
                effort: Some(effort.to_string()),
                summary: None,
            }))
            .build()
    }

    pub(crate) fn bundled_model(slug: &str) -> ModelInfo {
        crate::model::default_models()
            .into_iter()
            .find(|model| model.slug == slug)
            .unwrap_or_else(|| panic!("test bundled model not found: {slug}"))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::OpenAiProtocol;
    use super::test_support::{context_items, text_message};
    use crate::completion::CompletionRequest;
    use pl_protocol::MessageRole;

    #[test]
    fn responses_use_top_level_instructions_and_developer_messages() {
        let request = CompletionRequest::builder()
            .instructions("base")
            .input(context_items(vec![
                text_message(MessageRole::System, "developer"),
                text_message(MessageRole::User, "user context"),
                text_message(MessageRole::User, "real prompt"),
            ]))
            .build();

        let responses_body = OpenAiProtocol::responses().build_request_body(&request);
        let chat_body = OpenAiProtocol::chat().build_request_body(&request);

        assert_eq!(responses_body["instructions"], serde_json::json!("base"),);
        assert_eq!(
            responses_body["input"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["role"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["developer", "user", "user"],
        );
        assert_eq!(
            chat_body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["role"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["system", "system", "user", "user"],
        );
        assert_eq!(
            chat_body["messages"][0]["content"],
            serde_json::json!("base"),
        );
    }
}
