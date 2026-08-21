use pl_protocol::Result;

use crate::completion::CompletionRequest;
#[cfg(test)]
use crate::completion::CompletionResponse;
use crate::model::info::ModelInfo;

mod request;
#[cfg(test)]
mod response;
pub(crate) mod sse;

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
        sse::OpenAiStreamDecoder::new(matches!(self.endpoint, OpenAiEndpoint::Responses))
    }

    pub(crate) fn visible_output_protocol(&self) -> VisibleOutputProtocol {
        match self.endpoint {
            OpenAiEndpoint::Responses => VisibleOutputProtocol::NativePhases,
            OpenAiEndpoint::ChatCompletions => VisibleOutputProtocol::TaggedText,
        }
    }
}

#[cfg(test)]
mod unit_tests;
