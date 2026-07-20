use std::collections::{BTreeSet, HashMap};

use pl_model::{
    ApplyPatchToolType, ModelInfo, ProviderConnectionMode, ProviderInfo,
    ProviderServiceCapabilities, ProviderWireProtocol, ToolWirePolicy,
};
use pl_protocol::{PureError, Result};
use serde::{Deserialize, Serialize};

use super::{ModelCatalogId, ProviderId, ProviderPresetId, builtin_model_catalog};

/// Provider 模型目录来源。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ProviderModelCatalogConfig {
    /// 引用 PL 内置只读目录，并允许追加不冲突的自定义模型。
    Bundled {
        catalog: ModelCatalogId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        additional_models: Vec<ModelInfo>,
    },
    /// 完全由产品配置提供的模型目录。
    Explicit { models: Vec<ModelInfo> },
}

/// Provider transport 的来源与连接选择。
///
/// preset 只保存引用和实例自己的连接方式，协议由 PL registry 解析；自定义
/// provider 必须显式声明 wire protocol，避免用厂商名称推断 endpoint。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ProviderTransportSelection {
    Preset {
        preset: ProviderPresetId,
        connection_mode: ProviderConnectionMode,
    },
    Custom {
        protocol: ProviderWireProtocol,
        connection_mode: ProviderConnectionMode,
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
    pub transport: ProviderTransportSelection,
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
            ProviderInfo::deepseek(None),
            ModelCatalogId::new("deepseek").expect("static model catalog id is valid"),
            Vec::new(),
        )
        .with_preset(ProviderPresetId::new("deepseek").expect("static provider preset id is valid"))
    }

    /// 从运行时 provider 信息和显式模型目录创建配置。
    pub fn from_provider_info(info: ProviderInfo, models: Vec<ModelInfo>) -> Self {
        Self::from_explicit_models(info, models)
    }

    /// 从 PL 内置模型目录创建 provider 配置。
    pub fn from_bundled_catalog(
        info: ProviderInfo,
        catalog: ModelCatalogId,
        additional_models: Vec<ModelInfo>,
    ) -> Self {
        Self::from_parts(
            info,
            ProviderModelCatalogConfig::Bundled {
                catalog,
                additional_models,
            },
        )
    }

    /// 从完全显式的模型目录创建 provider 配置。
    pub fn from_explicit_models(info: ProviderInfo, models: Vec<ModelInfo>) -> Self {
        Self::from_parts(info, ProviderModelCatalogConfig::Explicit { models })
    }

    fn from_parts(info: ProviderInfo, catalog: ProviderModelCatalogConfig) -> Self {
        let service_capabilities = info.service_capabilities.clone();
        Self {
            transport: ProviderTransportSelection::Custom {
                protocol: info.protocol,
                connection_mode: info.connection_mode,
            },
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
        self.transport = ProviderTransportSelection::Preset {
            preset,
            connection_mode: self.connection_mode(),
        };
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
                    .map(|preset| preset.service_capabilities)
                    .ok_or_else(|| {
                        PureError::ConfigError(format!("unknown provider preset: {preset_id}"))
                    }),
                None => Ok(ProviderServiceCapabilities::default()),
            },
        }
    }

    /// 返回实例绑定的内置 preset；自定义 provider 返回 `None`。
    pub fn preset_id(&self) -> Option<&ProviderPresetId> {
        match &self.transport {
            ProviderTransportSelection::Preset { preset, .. } => Some(preset),
            ProviderTransportSelection::Custom { .. } => None,
        }
    }

    /// 返回该 provider 实例显式选择的连接方式。
    pub fn connection_mode(&self) -> ProviderConnectionMode {
        match &self.transport {
            ProviderTransportSelection::Preset {
                connection_mode, ..
            }
            | ProviderTransportSelection::Custom {
                connection_mode, ..
            } => *connection_mode,
        }
    }

    /// 修改实例连接方式，同时保留 preset/custom 来源和协议选择。
    pub fn set_connection_mode(&mut self, mode: ProviderConnectionMode) {
        match &mut self.transport {
            ProviderTransportSelection::Preset {
                connection_mode, ..
            }
            | ProviderTransportSelection::Custom {
                connection_mode, ..
            } => *connection_mode = mode,
        }
    }

    pub fn with_connection_mode(mut self, mode: ProviderConnectionMode) -> Self {
        self.set_connection_mode(mode);
        self
    }

    /// 解析该实例使用的 wire protocol。
    pub fn protocol(&self) -> Result<ProviderWireProtocol> {
        match &self.transport {
            ProviderTransportSelection::Custom { protocol, .. } => Ok(*protocol),
            ProviderTransportSelection::Preset { preset, .. } => super::builtin_provider_catalog()
                .presets
                .into_iter()
                .find(|candidate| &candidate.id == preset)
                .map(|candidate| candidate.protocol)
                .ok_or_else(|| {
                    PureError::ConfigError(format!("unknown provider preset: {preset}"))
                }),
        }
    }

    /// 解析 PL 内置目录和产品附加目录，返回运行时唯一有效模型列表。
    pub fn effective_models(&self) -> Result<Vec<ModelInfo>> {
        match &self.catalog {
            ProviderModelCatalogConfig::Bundled {
                catalog,
                additional_models,
            } => {
                let bundled = builtin_model_catalog(catalog)?;
                if bundled.protocol != self.protocol()? {
                    return Err(PureError::ConfigError(format!(
                        "provider protocol {:?} does not match model catalog {catalog}",
                        self.protocol()?
                    )));
                }
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
            ProviderModelCatalogConfig::Explicit { models } => Ok(models.clone()),
        }
    }

    /// 返回可由产品编辑的模型列表，不包含 PL 内置只读模型。
    pub fn editable_models(&self) -> &[ModelInfo] {
        match &self.catalog {
            ProviderModelCatalogConfig::Bundled {
                additional_models, ..
            } => additional_models,
            ProviderModelCatalogConfig::Explicit { models } => models,
        }
    }

    /// 使用角色路由选中的模型创建 provider runtime 信息。
    pub fn to_provider_info(&self, model: &str) -> Result<ProviderInfo> {
        Ok(ProviderInfo {
            protocol: self.protocol()?,
            connection_mode: self.connection_mode(),
            name: self.name.clone(),
            base_url: self.base_url.clone(),
            default_model: model.to_string(),
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
        let protocol = self.protocol()?;
        if !super::provider_connection_modes(protocol).contains(&self.connection_mode()) {
            return Err(PureError::ConfigError(format!(
                "provider {provider_id} protocol {protocol:?} does not support {:?}",
                self.connection_mode()
            )));
        }
        let service_capabilities = self.service_capabilities()?;
        if service_capabilities.web_search.hosted_responses
            && protocol != ProviderWireProtocol::Responses
        {
            return Err(PureError::ConfigError(format!(
                "provider {provider_id} declares hosted Responses web search for protocol {protocol:?}"
            )));
        }
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
        if !preset
            .connection_policy
            .supported_modes
            .contains(&self.connection_mode())
        {
            return Err(PureError::ConfigError(format!(
                "provider {provider_id} connection mode {:?} is not supported by preset {preset_id}",
                self.connection_mode()
            )));
        }
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
