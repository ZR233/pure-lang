use pl_protocol::AgentEventSender;
use pl_protocol::Result;

use crate::capabilities::{ModelCapabilities, ProviderCapabilities};
use crate::default_models::zhipu_default_model_slugs;
use crate::model_info::ModelInfo;
use crate::protocol::openai::{ChatReasoningStyle, OpenAiProtocol};
use crate::provider::openai::bundled_models;
use crate::provider::openai_runtime::OpenAiTransportProvider;
use crate::provider_info::ProviderInfo;
use crate::request::{CompletionRequest, CompletionResponse};

#[derive(Debug)]
pub struct ZhipuProvider {
    inner: OpenAiTransportProvider,
}

impl ZhipuProvider {
    pub(crate) fn new(info: ProviderInfo, configured_models: Vec<ModelInfo>) -> Result<Self> {
        Ok(Self {
            inner: OpenAiTransportProvider::new(
                info,
                bundled_models(zhipu_default_model_slugs()),
                configured_models,
                OpenAiProtocol::chat(ChatReasoningStyle::Zhipu),
                ProviderCapabilities::all(),
            )?,
        })
    }

    pub(crate) fn info(&self) -> &ProviderInfo {
        self.inner.info()
    }

    pub(crate) fn capabilities(&self) -> ProviderCapabilities {
        self.inner.capabilities()
    }

    pub(crate) fn stream_complete(
        &self,
        request: CompletionRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<CompletionResponse>> + Send {
        self.inner.stream_complete(request, event_tx)
    }

    pub(crate) fn auth_token(
        &self,
    ) -> impl std::future::Future<Output = Result<Option<String>>> + Send {
        self.inner.auth_token()
    }

    pub(crate) fn model_info(&self, model: &str) -> ModelInfo {
        self.inner.model_info(model)
    }

    pub(crate) fn list_models(&self) -> Vec<ModelInfo> {
        self.inner.list_models()
    }

    pub(crate) fn effective_model_capabilities(&self, model: &str) -> ModelCapabilities {
        self.inner.effective_model_capabilities(model)
    }

    pub(crate) fn default_model(&self) -> &str {
        self.inner.default_model()
    }
}
