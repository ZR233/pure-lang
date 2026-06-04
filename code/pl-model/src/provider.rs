use std::fmt;
use std::sync::Arc;

use pl_protocol::AgentEventSender;
use pl_protocol::Result;

use crate::capabilities::{ModelCapabilities, ProviderCapabilities};
use crate::model_info::ModelInfo;
use crate::provider_info::{ProviderInfo, ProviderKind};
use crate::request::CompletionRequest;
use crate::request::CompletionResponse;

mod deepseek;
mod openai;
mod openai_runtime;
mod zhipu;

pub use deepseek::DeepSeekProvider;
pub use openai::OpenAiProvider;
pub use zhipu::ZhipuProvider;

/// LLM Provider 运行时抽象。
///
/// 封装认证、API 调用、能力查询和模型目录等 provider 特定逻辑。
/// 实现者应只暴露 `pl-model` 的统一请求/响应类型，不把 provider 私有 wire 结构泄漏给 `pl-core`。
pub trait ModelProvider: fmt::Debug + Send + Sync {
    fn info(&self) -> &ProviderInfo;
    fn capabilities(&self) -> ProviderCapabilities;

    fn stream_complete(
        &self,
        request: CompletionRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<CompletionResponse>> + Send;

    fn auth_token(&self) -> impl std::future::Future<Output = Result<Option<String>>> + Send;

    fn model_info(&self, model: &str) -> ModelInfo;
    fn list_models(&self) -> Vec<ModelInfo>;
    fn effective_model_capabilities(&self, model: &str) -> ModelCapabilities;
    fn default_model(&self) -> &str;
}

#[derive(Debug)]
pub enum ProviderRuntime {
    OpenAi(OpenAiProvider),
    DeepSeek(DeepSeekProvider),
    Zhipu(ZhipuProvider),
}

pub type SharedModelProvider = Arc<ProviderRuntime>;

/// 根据 ProviderInfo 创建对应的 ModelProvider 实例。
pub fn create_provider(info: ProviderInfo) -> Result<SharedModelProvider> {
    create_provider_with_models(info, Vec::new())
}

/// 根据 ProviderInfo 和配置模型列表创建对应的 ModelProvider 实例。
pub fn create_provider_with_models(
    info: ProviderInfo,
    models: Vec<ModelInfo>,
) -> Result<SharedModelProvider> {
    let runtime = match info.provider_kind {
        ProviderKind::OpenAi => ProviderRuntime::OpenAi(OpenAiProvider::new(info, models)?),
        ProviderKind::DeepSeek => ProviderRuntime::DeepSeek(DeepSeekProvider::new(info, models)?),
        ProviderKind::Zhipu => ProviderRuntime::Zhipu(ZhipuProvider::new(info, models)?),
    };
    Ok(Arc::new(runtime))
}

impl ModelProvider for ProviderRuntime {
    fn info(&self) -> &ProviderInfo {
        match self {
            Self::OpenAi(provider) => provider.info(),
            Self::DeepSeek(provider) => provider.info(),
            Self::Zhipu(provider) => provider.info(),
        }
    }

    fn capabilities(&self) -> ProviderCapabilities {
        match self {
            Self::OpenAi(provider) => provider.capabilities(),
            Self::DeepSeek(provider) => provider.capabilities(),
            Self::Zhipu(provider) => provider.capabilities(),
        }
    }

    async fn stream_complete(
        &self,
        request: CompletionRequest,
        event_tx: AgentEventSender,
    ) -> Result<CompletionResponse> {
        match self {
            Self::OpenAi(provider) => provider.stream_complete(request, event_tx).await,
            Self::DeepSeek(provider) => provider.stream_complete(request, event_tx).await,
            Self::Zhipu(provider) => provider.stream_complete(request, event_tx).await,
        }
    }

    async fn auth_token(&self) -> Result<Option<String>> {
        match self {
            Self::OpenAi(provider) => provider.auth_token().await,
            Self::DeepSeek(provider) => provider.auth_token().await,
            Self::Zhipu(provider) => provider.auth_token().await,
        }
    }

    fn model_info(&self, model: &str) -> ModelInfo {
        match self {
            Self::OpenAi(provider) => provider.model_info(model),
            Self::DeepSeek(provider) => provider.model_info(model),
            Self::Zhipu(provider) => provider.model_info(model),
        }
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        match self {
            Self::OpenAi(provider) => provider.list_models(),
            Self::DeepSeek(provider) => provider.list_models(),
            Self::Zhipu(provider) => provider.list_models(),
        }
    }

    fn effective_model_capabilities(&self, model: &str) -> ModelCapabilities {
        match self {
            Self::OpenAi(provider) => provider.effective_model_capabilities(model),
            Self::DeepSeek(provider) => provider.effective_model_capabilities(model),
            Self::Zhipu(provider) => provider.effective_model_capabilities(model),
        }
    }

    fn default_model(&self) -> &str {
        match self {
            Self::OpenAi(provider) => provider.default_model(),
            Self::DeepSeek(provider) => provider.default_model(),
            Self::Zhipu(provider) => provider.default_model(),
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn factory_creates_deepseek_runtime() {
        let provider = create_provider(ProviderInfo::deepseek(None)).unwrap();

        assert!(matches!(provider.as_ref(), ProviderRuntime::DeepSeek(_)));
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
