use std::collections::{BTreeMap, BTreeSet, HashMap};

use pl_protocol::{PureError, Result};
use serde::{Deserialize, Serialize};

use super::{ModelCatalogId, ProviderId, ProviderPresetId, builtin_model_catalog};
use pl_model::model::ModelInfo;
use pl_model::provider::{
    ApplyPatchToolType, ProviderConnectionMode, ProviderEndpoint, ProviderServiceCapabilities,
    ToolWirePolicy,
};

/// Provider 模型目录来源。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ProviderModelCatalogConfig {
    /// 引用 PL 内置只读目录，并允许追加不冲突的自定义模型。
    Bundled {
        catalog: ModelCatalogId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        additional_models: Vec<ModelInfo>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        connection_overrides: BTreeMap<String, ProviderConnectionMode>,
    },
    /// 完全由产品配置提供的模型目录。
    Explicit {
        models: Vec<ModelInfo>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        connection_overrides: BTreeMap<String, ProviderConnectionMode>,
    },
}

/// Provider 服务能力的配置来源。
///
/// preset 实例通常继承 registry 默认能力；显式配置可覆盖任意兼容 endpoint。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ProviderCapabilitySelection {
    #[default]
    PresetDefaults,
    Explicit(ProviderServiceCapabilities),
}

/// 可由产品配置文件组合使用的 Provider 值对象。
///
/// 具体默认模型不属于 Provider；调用方必须通过角色路由选择模型。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<ProviderPresetId>,
    pub name: String,
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer_token: Option<String>,
    /// 可选的凭证环境变量名；显式 token 始终优先。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer_token_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub tool_wire_policy: ToolWirePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_patch_tool_type: Option<ApplyPatchToolType>,
    #[serde(default)]
    pub capabilities: ProviderCapabilitySelection,
    pub catalog: ProviderModelCatalogConfig,
}

impl ProviderConfig {
    /// 构造内置 DeepSeek provider preset，不附带任何产品角色默认值。
    pub fn deepseek_preset() -> Self {
        Self::from_bundled_catalog(
            ProviderEndpoint::deepseek(None),
            ModelCatalogId::new("deepseek").expect("static model catalog id is valid"),
            Vec::new(),
        )
        .with_preset(ProviderPresetId::new("deepseek").expect("static provider preset id is valid"))
    }

    /// 从 PL 内置模型目录创建 provider 配置。
    pub fn from_bundled_catalog(
        info: ProviderEndpoint,
        catalog: ModelCatalogId,
        additional_models: Vec<ModelInfo>,
    ) -> Self {
        Self::from_parts(
            info,
            ProviderModelCatalogConfig::Bundled {
                catalog,
                additional_models,
                connection_overrides: BTreeMap::new(),
            },
        )
    }

    /// 从完全显式的模型目录创建 provider 配置。
    pub fn from_explicit_models(info: ProviderEndpoint, models: Vec<ModelInfo>) -> Self {
        Self::from_parts(
            info,
            ProviderModelCatalogConfig::Explicit {
                models,
                connection_overrides: BTreeMap::new(),
            },
        )
    }

    fn from_parts(info: ProviderEndpoint, catalog: ProviderModelCatalogConfig) -> Self {
        let service_capabilities = info.service_capabilities.clone();
        Self {
            preset: None,
            name: info.name,
            base_url: info.base_url,
            bearer_token: info.bearer_token,
            bearer_token_env: None,
            http_headers: info.http_headers,
            tool_wire_policy: info.tool_wire_policy,
            apply_patch_tool_type: info.apply_patch_tool_type,
            capabilities: ProviderCapabilitySelection::Explicit(service_capabilities),
            catalog,
        }
    }

    /// 记录创建该实例所使用的内置 preset。
    pub fn with_preset(mut self, preset: ProviderPresetId) -> Self {
        self.preset = Some(preset);
        self.capabilities = ProviderCapabilitySelection::PresetDefaults;
        self
    }

    /// 解析 preset 默认值或实例显式覆盖后的服务能力。
    pub fn service_capabilities(&self) -> Result<ProviderServiceCapabilities> {
        match &self.capabilities {
            ProviderCapabilitySelection::Explicit(capabilities) => Ok(capabilities.clone()),
            ProviderCapabilitySelection::PresetDefaults => match self.preset_id() {
                Some(preset_id) => super::builtin_provider_catalog()
                    .presets
                    .into_iter()
                    .find(|preset| &preset.id == preset_id)
                    .map(|preset| {
                        let mut capabilities = preset.service_capabilities;
                        if self.base_url.trim_end_matches('/')
                            != preset.provider.base_url.trim_end_matches('/')
                        {
                            capabilities.responses_tools = Default::default();
                            capabilities.web_search.hosted_responses = false;
                        }
                        capabilities
                    })
                    .ok_or_else(|| {
                        PureError::ConfigError(format!("unknown provider preset: {preset_id}"))
                    }),
                None => Ok(ProviderServiceCapabilities::default()),
            },
        }
    }

    /// 返回实例绑定的内置 preset；自定义 provider 返回 `None`。
    pub fn preset_id(&self) -> Option<&ProviderPresetId> {
        self.preset.as_ref()
    }

    /// 解析 PL 内置目录和产品附加目录，返回运行时唯一有效模型列表。
    pub fn effective_models(&self) -> Result<Vec<ModelInfo>> {
        apply_connection_overrides(self.declared_models()?, self.connection_overrides())
    }

    /// 返回模型目录声明，不应用 provider 实例的当前连接方式 override。
    pub fn declared_models(&self) -> Result<Vec<ModelInfo>> {
        match &self.catalog {
            ProviderModelCatalogConfig::Bundled {
                catalog,
                additional_models,
                ..
            } => {
                let bundled = builtin_model_catalog(catalog)?;
                let mut models = bundled.models;
                let mut slugs = models
                    .iter()
                    .map(|model| model.slug.clone())
                    .collect::<BTreeSet<_>>();
                for model in additional_models {
                    if !slugs.insert(model.slug.clone()) {
                        return Err(PureError::ConfigError(format!(
                            "additional model conflicts with bundled model: {}",
                            model.slug
                        )));
                    }
                    models.push(model.clone());
                }
                Ok(models)
            }
            ProviderModelCatalogConfig::Explicit { models, .. } => Ok(models.clone()),
        }
    }

    /// 返回按模型 slug 保存的当前连接方式 override。
    pub fn connection_overrides(&self) -> &BTreeMap<String, ProviderConnectionMode> {
        match &self.catalog {
            ProviderModelCatalogConfig::Bundled {
                connection_overrides,
                ..
            }
            | ProviderModelCatalogConfig::Explicit {
                connection_overrides,
                ..
            } => connection_overrides,
        }
    }

    /// 返回可由产品编辑的模型列表，不包含 PL 内置只读模型。
    pub fn editable_models(&self) -> &[ModelInfo] {
        match &self.catalog {
            ProviderModelCatalogConfig::Bundled {
                additional_models, ..
            } => additional_models,
            ProviderModelCatalogConfig::Explicit { models, .. } => models,
        }
    }

    /// 为 provider 实例中的某个模型选择连接方式。
    pub fn set_model_connection_mode(
        &mut self,
        model: &str,
        mode: ProviderConnectionMode,
    ) -> Result<()> {
        let models = self.declared_models()?;
        let selected = models
            .iter()
            .find(|candidate| candidate.slug == model)
            .ok_or_else(|| PureError::ConfigError(format!("unknown model: {model}")))?;
        if !selected
            .transport
            .supported_connection_modes
            .contains(&mode)
        {
            return Err(PureError::ConfigError(format!(
                "model {model} does not support connection mode {mode:?}"
            )));
        }
        if selected.transport.default_connection_mode == mode {
            self.connection_overrides_mut().remove(model);
        } else {
            self.connection_overrides_mut()
                .insert(model.to_string(), mode);
        }
        Ok(())
    }

    fn connection_overrides_mut(&mut self) -> &mut BTreeMap<String, ProviderConnectionMode> {
        match &mut self.catalog {
            ProviderModelCatalogConfig::Bundled {
                connection_overrides,
                ..
            }
            | ProviderModelCatalogConfig::Explicit {
                connection_overrides,
                ..
            } => connection_overrides,
        }
    }

    /// 解析不包含模型与 transport 事实的 runtime endpoint。
    pub fn to_endpoint(&self) -> Result<ProviderEndpoint> {
        Ok(ProviderEndpoint {
            name: self.name.clone(),
            base_url: self.base_url.clone(),
            bearer_token: self.resolved_bearer_token(),
            http_headers: self.http_headers.clone(),
            tool_wire_policy: self.tool_wire_policy,
            apply_patch_tool_type: self.apply_patch_tool_type,
            service_capabilities: self.service_capabilities()?,
        })
    }

    /// 解析运行时凭证；配置中的显式 secret 优先于环境变量。
    pub fn resolved_bearer_token(&self) -> Option<String> {
        self.bearer_token
            .clone()
            .filter(|token| !token.trim().is_empty())
            .or_else(|| {
                self.bearer_token_env
                    .as_deref()
                    .filter(|name| !name.trim().is_empty())
                    .and_then(|name| std::env::var(name).ok())
                    .filter(|token| !token.trim().is_empty())
            })
    }

    pub(super) fn validate(&self, provider_id: &ProviderId) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(PureError::ConfigError(format!(
                "provider {provider_id} has empty name"
            )));
        }
        if self.base_url.trim().is_empty() {
            return Err(PureError::ConfigError(format!(
                "provider {provider_id} has empty base_url"
            )));
        }
        self.service_capabilities()?;
        if self
            .bearer_token_env
            .as_deref()
            .is_some_and(|name| name.trim().is_empty())
        {
            return Err(PureError::ConfigError(format!(
                "provider {provider_id} has empty bearer_token_env"
            )));
        }
        self.validate_preset_binding(provider_id)?;
        let models = self.effective_models()?;
        if models.is_empty() {
            return Err(PureError::ConfigError(format!(
                "provider {provider_id} must define at least one model"
            )));
        }
        let mut slugs = BTreeSet::new();
        for model in &models {
            model
                .transport
                .validate(&model.slug)
                .map_err(PureError::ConfigError)?;
            model
                .validate_media_contract()
                .map_err(PureError::ConfigError)?;
            if model.slug.trim().is_empty() {
                return Err(PureError::ConfigError(format!(
                    "provider {provider_id} contains a model with empty slug"
                )));
            }
            if !slugs.insert(model.slug.as_str()) {
                return Err(PureError::ConfigError(format!(
                    "provider {provider_id} contains duplicate model: {}",
                    model.slug
                )));
            }
        }
        Ok(())
    }

    fn validate_preset_binding(&self, provider_id: &ProviderId) -> Result<()> {
        let Some(preset_id) = self.preset_id() else {
            return Ok(());
        };
        let preset = super::builtin_provider_catalog()
            .presets
            .into_iter()
            .find(|preset| &preset.id == preset_id)
            .ok_or_else(|| {
                PureError::ConfigError(format!(
                    "provider {provider_id} references unknown preset: {preset_id}"
                ))
            })?;
        match &self.catalog {
            ProviderModelCatalogConfig::Bundled { catalog, .. }
                if catalog == &preset.model_catalog =>
            {
                Ok(())
            }
            ProviderModelCatalogConfig::Bundled { catalog, .. } => {
                Err(PureError::ConfigError(format!(
                    "provider {provider_id} catalog {catalog} does not match preset {preset_id}"
                )))
            }
            ProviderModelCatalogConfig::Explicit { .. } => Err(PureError::ConfigError(format!(
                "provider {provider_id} cannot combine preset {preset_id} with an explicit catalog"
            ))),
        }
    }
}

fn apply_connection_overrides(
    mut models: Vec<ModelInfo>,
    overrides: &BTreeMap<String, ProviderConnectionMode>,
) -> Result<Vec<ModelInfo>> {
    for (slug, mode) in overrides {
        let model = models
            .iter_mut()
            .find(|model| model.slug == *slug)
            .ok_or_else(|| {
                PureError::ConfigError(format!(
                    "connection override references unknown model: {slug}"
                ))
            })?;
        if !model.transport.supported_connection_modes.contains(mode) {
            return Err(PureError::ConfigError(format!(
                "model {slug} does not support connection mode {mode:?}"
            )));
        }
        model.transport.default_connection_mode = *mode;
    }
    Ok(models)
}
