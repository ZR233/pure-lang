use crate::model_info::ModelInfo;
use crate::provider::{ModelProvider, SharedModelProvider};
use crate::provider_info::ProviderInfo;
use pl_protocol::Result;

/// 模型管理器 trait。
///
/// 负责加载模型信息和管理 provider 实例。
pub trait ModelsManager: Send + Sync {
    /// 获取指定 slug 的模型信息
    fn model_info(&self, slug: &str) -> ModelInfo;

    /// 列出所有可用模型
    fn list_models(&self) -> Vec<ModelInfo>;

    /// 获取默认模型 slug
    fn default_model(&self) -> &str;
}

/// 基于 bundled models + provider 的默认实现
pub struct DefaultModelsManager {
    provider: SharedModelProvider,
}

impl DefaultModelsManager {
    pub fn new(provider: SharedModelProvider) -> Self {
        Self { provider }
    }

    pub fn from_provider_info(info: ProviderInfo) -> Result<Self> {
        let provider = crate::provider::create_provider(info)?;
        Ok(Self { provider })
    }
}

impl ModelsManager for DefaultModelsManager {
    fn model_info(&self, slug: &str) -> ModelInfo {
        self.provider.model_info(slug)
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        crate::default_models::default_model_slugs()
            .iter()
            .map(|slug| self.provider.model_info(slug))
            .filter(|m| m.context_window.unwrap_or(0) > 0)
            .collect()
    }

    fn default_model(&self) -> &str {
        self.provider.default_model()
    }
}
