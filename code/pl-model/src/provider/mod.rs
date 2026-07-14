use std::fmt;
use std::sync::Arc;

use pl_protocol::{PureError, Result};
use pl_trace::AgentEventSender;

use crate::capabilities::{ModelCapabilities, ProviderCapabilities};
use crate::model_info::ModelInfo;
use crate::provider_info::ProviderInfo;
use crate::request::CompletionRequest;
use crate::request::CompletionResponse;
use crate::request::{ModelCompactionRequest, ModelCompactionResponse};
use crate::stream::CompletionEventStream;

mod openai_runtime;

pub use openai_runtime::OpenAiProvider;

/// LLM Provider 运行时抽象。
///
/// 封装认证、API 调用、能力查询和模型目录等 provider 特定逻辑。
/// 实现者应只暴露 `pl-model` 的统一请求/响应类型，不把 provider 私有 wire 结构泄漏给 `pl-core`。
///
/// 当前 OpenAI、DeepSeek、Zhipu 三家供应商共享同一个 OpenAI 兼容 transport，
/// 差异由 `ProviderKind` 在 `OpenAiProvider::new` 中一次性决定（endpoint、
/// bundled 模型、能力位），不再为每家供应商单独定义 struct 或穷尽枚举分发。
/// 未来若引入协议真正不同的供应商（如 Anthropic），再新增独立 provider struct。
pub trait ModelProvider: fmt::Debug + Send + Sync {
    fn info(&self) -> &ProviderInfo;
    fn capabilities(&self) -> ProviderCapabilities;

    fn stream_events(
        &self,
        request: CompletionRequest,
    ) -> impl std::future::Future<Output = Result<CompletionEventStream>> + Send;

    fn stream_complete(
        &self,
        request: CompletionRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<CompletionResponse>> + Send;

    fn compact_context(
        &self,
        request: ModelCompactionRequest,
    ) -> impl std::future::Future<Output = Result<ModelCompactionResponse>> + Send {
        async move {
            let _ = request;
            Err(PureError::ConfigError(
                "provider does not support remote context compaction".to_string(),
            ))
        }
    }

    fn auth_token(&self) -> impl std::future::Future<Output = Result<Option<String>>> + Send;

    fn model_info(&self, model: &str) -> ModelInfo;
    fn list_models(&self) -> Vec<ModelInfo>;
    fn effective_model_capabilities(&self, model: &str) -> ModelCapabilities;
    fn default_model(&self) -> &str;
}

pub type SharedModelProvider = Arc<OpenAiProvider>;

/// 根据 ProviderInfo 创建对应的 ModelProvider 实例。
pub fn create_provider(info: ProviderInfo) -> Result<SharedModelProvider> {
    create_provider_with_models(info, Vec::new())
}

/// 根据 ProviderInfo 和配置模型列表创建对应的 ModelProvider 实例。
pub fn create_provider_with_models(
    info: ProviderInfo,
    models: Vec<ModelInfo>,
) -> Result<SharedModelProvider> {
    Ok(Arc::new(OpenAiProvider::new(info, models)?))
}

impl ModelProvider for OpenAiProvider {
    fn info(&self) -> &ProviderInfo {
        OpenAiProvider::info(self)
    }

    fn capabilities(&self) -> ProviderCapabilities {
        OpenAiProvider::capabilities(self)
    }

    fn stream_events(
        &self,
        request: CompletionRequest,
    ) -> impl std::future::Future<Output = Result<CompletionEventStream>> + Send {
        OpenAiProvider::stream_events(self, request)
    }

    fn stream_complete(
        &self,
        request: CompletionRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<CompletionResponse>> + Send {
        OpenAiProvider::stream_complete(self, request, event_tx)
    }

    fn compact_context(
        &self,
        request: ModelCompactionRequest,
    ) -> impl std::future::Future<Output = Result<ModelCompactionResponse>> + Send {
        OpenAiProvider::compact_context(self, request)
    }

    fn auth_token(&self) -> impl std::future::Future<Output = Result<Option<String>>> + Send {
        OpenAiProvider::auth_token(self)
    }

    fn model_info(&self, model: &str) -> ModelInfo {
        OpenAiProvider::model_info(self, model)
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        OpenAiProvider::list_models(self)
    }

    fn effective_model_capabilities(&self, model: &str) -> ModelCapabilities {
        OpenAiProvider::effective_model_capabilities(self, model)
    }

    fn default_model(&self) -> &str {
        OpenAiProvider::default_model(self)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn factory_creates_deepseek_provider() {
        let provider = create_provider(ProviderInfo::deepseek(None)).unwrap();

        assert_eq!(provider.default_model(), "deepseek-v4-flash");
    }

    #[test]
    fn list_models_is_provider_scoped() {
        let provider = create_provider(ProviderInfo::deepseek(None)).unwrap();
        let models = provider.list_models();

        assert!(models.iter().any(|model| model.slug == "deepseek-v4-flash"));
        assert!(!models.iter().any(|model| model.slug == "gpt-5.5"));
    }
}
