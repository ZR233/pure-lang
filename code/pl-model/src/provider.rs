use std::fmt;
use std::sync::Arc;

use pl_protocol::AgentEventSender;
use pl_protocol::Result;

use crate::capabilities::ProviderCapabilities;
use crate::model_info::ModelInfo;
use crate::openai::OpenAiCompatibleProvider;
use crate::provider_info::ProviderInfo;
use crate::request::CompletionRequest;
use crate::request::CompletionResponse;

/// LLM Provider 运行时抽象。
///
/// 封装认证、API 调用、能力查询等 provider 特定逻辑。
/// 通过工厂函数 `create_provider()` 创建。
///
/// 实现者契约：
/// - 通过 event_tx 推送 LLM 输出增量（TextDelta/ThinkingDelta/ToolCallDelta）
/// - capabilities() 如实报告支持的功能
/// - auth_token() 返回当前有效的认证凭据
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
    fn default_model(&self) -> &str;
}

pub type SharedModelProvider = Arc<OpenAiCompatibleProvider>;

/// 根据 ProviderInfo 创建对应的 ModelProvider 实例
pub fn create_provider(info: ProviderInfo) -> Result<SharedModelProvider> {
    Ok(Arc::new(OpenAiCompatibleProvider::new(info)?))
}

/// 根据 ProviderInfo 和配置模型列表创建对应的 ModelProvider 实例。
pub fn create_provider_with_models(
    info: ProviderInfo,
    models: Vec<ModelInfo>,
) -> Result<SharedModelProvider> {
    Ok(Arc::new(OpenAiCompatibleProvider::with_models(
        info, models,
    )?))
}
