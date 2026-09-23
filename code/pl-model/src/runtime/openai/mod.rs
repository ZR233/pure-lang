use pl_protocol::Result;

use crate::completion::CompletionRequest;
use crate::model::info::ModelInfo;

mod identity;
mod request;
pub(crate) mod sse;
pub(crate) mod usage;

use request::build_openai_request_body;
pub(crate) use request::{OpenAiRequestBody, sent_model_from_wire_body};
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
