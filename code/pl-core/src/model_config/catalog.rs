use std::collections::{BTreeMap, BTreeSet};

use pl_model::{
    ModelInfo, ModelModality, ProviderConnectionMode, ProviderEndpoint,
    ProviderServiceCapabilities, ProviderWireProtocol, deepseek_default_model_slugs,
    default_models, mimo_default_model_slugs, openai_default_model_slugs,
    provider_transport_profile_revision, zhipu_default_model_slugs,
};
use pl_protocol::{
    CredentialDescriptorDto, ModelCapabilitiesDto, ModelCatalogDescriptor, ModelDescriptor,
    ModelInputCapabilityDto, ModelInputSourceDto, ModelModalityDto, ModelPricingDto,
    ModelReasoningDescriptor, ModelTransportDescriptor, PROVIDER_CATALOG_SCHEMA_VERSION,
    ProviderCatalogSnapshot, ProviderConnectionModeDescriptor, ProviderPresetDescriptor,
    ProviderServiceCapabilitiesDescriptor, PureError, Result,
    WebSearchProviderCapabilitiesDescriptor,
};

use super::{ModelCatalogId, ProviderConfig, ProviderPresetId};

const MIMO_API_BASE_URL: &str = "https://api.xiaomimimo.com/v1";
const MIMO_TOKEN_PLAN_BASE_URL: &str = "https://token-plan-cn.xiaomimimo.com/v1";

/// 一组由 PL 维护且对产品只读的模型定义。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelCatalog {
    pub id: ModelCatalogId,
    pub models: Vec<ModelInfo>,
}

/// 可由宿主直接实例化的内置 Provider 预设。
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderPreset {
    pub id: ProviderPresetId,
    pub display_name: String,
    pub description: Option<String>,
    pub provider: ProviderConfig,
    pub credential_label: String,
    pub credential_env: Option<String>,
    pub model_catalog: ModelCatalogId,
    pub suggested_model: String,
    pub icon_key: Option<String>,
    pub service_capabilities: ProviderServiceCapabilities,
}

/// PL 内置 Provider 与模型目录注册表。
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderCatalogRegistry {
    pub presets: Vec<ProviderPreset>,
    pub model_catalogs: BTreeMap<ModelCatalogId, ModelCatalog>,
}

impl ProviderCatalogRegistry {
    /// 构造当前二进制内置的完整目录。
    pub fn builtin() -> Self {
        let model_catalogs = [
            model_catalog("openai", openai_default_model_slugs()),
            model_catalog("deepseek", deepseek_default_model_slugs()),
            model_catalog("zhipu", zhipu_default_model_slugs()),
            model_catalog("mimo", mimo_default_model_slugs()),
        ]
        .into_iter()
        .map(|catalog| (catalog.id.clone(), catalog))
        .collect();

        let presets = vec![
            preset(
                "openai",
                ProviderEndpoint::openai(None),
                "gpt-5.6-sol",
                "openai",
                "OPENAI_API_KEY",
                "OpenAI models served through the Responses API.",
                "openai",
            ),
            preset(
                "deepseek",
                ProviderEndpoint::deepseek(None),
                "deepseek-v4-flash",
                "deepseek",
                "DEEPSEEK_API_KEY",
                "DeepSeek reasoning and coding models.",
                "deepseek",
            ),
            preset(
                "zhipu",
                ProviderEndpoint::zhipu(None),
                "glm-5.2",
                "zhipu",
                "ZAI_API_KEY",
                "Zhipu BigModel API.",
                "zhipu",
            ),
            preset(
                "zhipu-coding-plan",
                ProviderEndpoint::zhipu_coding_plan(None),
                "glm-5.2",
                "zhipu",
                "ZAI_API_KEY",
                "Zhipu Coding Plan endpoint.",
                "zhipu",
            ),
            preset(
                "mimo-api",
                ProviderEndpoint::compatible("MiMo API", MIMO_API_BASE_URL),
                "mimo-v2.5-pro",
                "mimo",
                "MIMO_API_KEY",
                "Xiaomi MiMo public API.",
                "mimo",
            ),
            preset(
                "mimo-token-plan",
                ProviderEndpoint::compatible("MiMo Token Plan", MIMO_TOKEN_PLAN_BASE_URL),
                "mimo-v2.5-pro",
                "mimo",
                "MIMO_TOKEN_PLAN_API_KEY",
                "Xiaomi MiMo Token Plan endpoint.",
                "mimo",
            ),
        ];

        Self {
            presets,
            model_catalogs,
        }
    }

    /// 校验 preset、catalog、transport 与 suggested model 的引用完整性。
    pub fn validate(&self) -> Result<()> {
        let mut preset_ids = BTreeSet::new();
        for preset in &self.presets {
            if !preset_ids.insert(preset.id.as_str()) {
                return Err(PureError::ConfigError(format!(
                    "duplicate provider preset: {}",
                    preset.id
                )));
            }
            let catalog = self
                .model_catalogs
                .get(&preset.model_catalog)
                .ok_or_else(|| {
                    PureError::ConfigError(format!(
                        "provider preset {} references missing catalog: {}",
                        preset.id, preset.model_catalog
                    ))
                })?;
            for model in &catalog.models {
                model
                    .transport
                    .validate(&model.slug)
                    .map_err(PureError::ConfigError)?;
            }
            if !catalog
                .models
                .iter()
                .any(|model| model.slug == preset.suggested_model)
            {
                return Err(PureError::ConfigError(format!(
                    "provider preset {} references missing suggested model: {}",
                    preset.id, preset.suggested_model
                )));
            }
        }
        Ok(())
    }

    /// 查找一个内置模型目录。
    pub fn model_catalog(&self, id: &ModelCatalogId) -> Option<&ModelCatalog> {
        self.model_catalogs.get(id)
    }

    /// 生成供 Web 与 Flutter 共用的无 secret 快照。
    pub fn snapshot(&self) -> Result<ProviderCatalogSnapshot> {
        self.validate()?;
        let presets = self.presets.iter().map(preset_descriptor).collect();
        let model_catalogs = self
            .model_catalogs
            .iter()
            .map(|(id, catalog)| {
                (
                    id.to_string(),
                    ModelCatalogDescriptor {
                        id: id.to_string(),
                        models: catalog.models.iter().map(model_descriptor).collect(),
                    },
                )
            })
            .collect();
        let mut snapshot = ProviderCatalogSnapshot {
            schema_version: PROVIDER_CATALOG_SCHEMA_VERSION,
            revision: String::new(),
            presets,
            model_catalogs,
        };
        let mut canonical = serde_json::to_vec(&snapshot)
            .map_err(|error| PureError::ConfigError(error.to_string()))?;
        for catalog in self.model_catalogs.values() {
            for model in &catalog.models {
                for mode in &model.transport.supported_connection_modes {
                    canonical.push(0);
                    canonical.extend_from_slice(
                        provider_transport_profile_revision(model.transport.protocol, *mode)
                            .as_bytes(),
                    );
                }
            }
        }
        snapshot.revision = stable_revision(&canonical);
        Ok(snapshot)
    }
}

/// 返回当前 PL 内置 Provider 目录。
pub fn builtin_provider_catalog() -> ProviderCatalogRegistry {
    ProviderCatalogRegistry::builtin()
}

/// 返回一个内置模型目录的副本。
pub fn builtin_model_catalog(id: &ModelCatalogId) -> Result<ModelCatalog> {
    ProviderCatalogRegistry::builtin()
        .model_catalog(id)
        .cloned()
        .ok_or_else(|| PureError::ConfigError(format!("unknown model catalog: {id}")))
}

fn model_catalog(id: &str, slugs: &[&str]) -> ModelCatalog {
    let models = default_models()
        .into_iter()
        .filter(|model| slugs.contains(&model.slug.as_str()))
        .collect();
    ModelCatalog {
        id: ModelCatalogId::new(id).expect("static model catalog id is valid"),
        models,
    }
}

fn preset(
    id: &str,
    info: ProviderEndpoint,
    suggested_model: &str,
    catalog: &str,
    credential_env: &str,
    description: &str,
    icon_key: &str,
) -> ProviderPreset {
    let service_capabilities = info.service_capabilities.clone();
    let display_name = info.name.clone();
    let model_catalog = ModelCatalogId::new(catalog).expect("static model catalog id is valid");
    let preset_id = ProviderPresetId::new(id).expect("static provider preset id is valid");
    let mut provider =
        ProviderConfig::from_bundled_catalog(info, model_catalog.clone(), Vec::new())
            .with_preset(preset_id.clone());
    provider.bearer_token_env = Some(credential_env.to_string());
    ProviderPreset {
        id: preset_id.clone(),
        display_name,
        description: Some(description.to_string()),
        provider,
        credential_label: "API Key".to_string(),
        credential_env: Some(credential_env.to_string()),
        model_catalog,
        suggested_model: suggested_model.to_string(),
        icon_key: Some(icon_key.to_string()),
        service_capabilities,
    }
}

fn preset_descriptor(preset: &ProviderPreset) -> ProviderPresetDescriptor {
    ProviderPresetDescriptor {
        id: preset.id.to_string(),
        display_name: preset.display_name.clone(),
        description: preset.description.clone(),
        base_url: preset.provider.base_url.clone(),
        credential: CredentialDescriptorDto {
            label: preset.credential_label.clone(),
            env_var: preset.credential_env.clone(),
        },
        model_catalog_id: preset.model_catalog.to_string(),
        suggested_model: preset.suggested_model.clone(),
        icon_key: preset.icon_key.clone(),
        service_capabilities: provider_service_capabilities_descriptor(
            &preset.service_capabilities,
        ),
    }
}

/// 将运行时服务能力投影为无凭证公共 DTO。
pub fn provider_service_capabilities_descriptor(
    capabilities: &ProviderServiceCapabilities,
) -> ProviderServiceCapabilitiesDescriptor {
    ProviderServiceCapabilitiesDescriptor {
        web_search: WebSearchProviderCapabilitiesDescriptor {
            hosted_responses: capabilities.web_search.hosted_responses,
            hosted_dialect: capabilities.web_search.hosted_dialect.as_str().to_string(),
            standalone: capabilities
                .web_search
                .standalone
                .map(|dialect| dialect.as_str().to_string()),
        },
        prompt_cache_dialect: capabilities.prompt_cache.dialect.as_str().to_string(),
        responses_programmatic_tool_calling: capabilities.responses_tools.programmatic_tool_calling,
    }
}

fn connection_mode_descriptor(mode: ProviderConnectionMode) -> ProviderConnectionModeDescriptor {
    ProviderConnectionModeDescriptor {
        id: connection_mode_label(mode).to_string(),
        display_name: match mode {
            ProviderConnectionMode::WebSocket => "WebSocket".to_string(),
            ProviderConnectionMode::Http => "HTTP".to_string(),
        },
    }
}

fn connection_mode_label(mode: ProviderConnectionMode) -> &'static str {
    match mode {
        ProviderConnectionMode::WebSocket => "web_socket",
        ProviderConnectionMode::Http => "http",
    }
}

fn model_descriptor(model: &ModelInfo) -> ModelDescriptor {
    let capabilities = &model.capabilities;
    let reasoning = model
        .effort_parameter()
        .map(|parameter| ModelReasoningDescriptor {
            parameter: parameter.name.clone(),
            label: parameter
                .label
                .clone()
                .unwrap_or_else(|| "Reasoning effort".to_string()),
            default: parameter.candidates.first().cloned(),
            candidates: parameter.candidates.clone(),
        });
    let pricing = model.currency.as_ref().map(|currency| ModelPricingDto {
        currency: currency.clone(),
        input_per_mtok: model.input_price_per_mtok,
        output_per_mtok: model.output_price_per_mtok,
        cache_read_per_mtok: model.cache_read_price_per_mtok,
        cache_write_per_mtok: model.cache_write_price_per_mtok,
    });
    ModelDescriptor {
        id: model.slug.clone(),
        display_name: model.display_name.clone(),
        description: model.description.clone(),
        context_window: model.context_window,
        max_context_window: model.max_context_window,
        max_output_tokens: model.max_output_tokens,
        transport: ModelTransportDescriptor {
            protocol: protocol_label(model.transport.protocol).to_string(),
            connection_modes: model
                .transport
                .supported_connection_modes
                .iter()
                .copied()
                .map(connection_mode_descriptor)
                .collect(),
            default_connection_mode: connection_mode_label(model.transport.default_connection_mode)
                .to_string(),
        },
        capabilities: ModelCapabilitiesDto {
            input: capabilities
                .input
                .iter()
                .map(|capability| ModelInputCapabilityDto {
                    modality: modality_descriptor(capability.modality),
                    sources: capability
                        .sources
                        .iter()
                        .copied()
                        .map(input_source_descriptor)
                        .collect(),
                    max_count: capability.limits.max_count,
                    max_bytes: capability.limits.max_bytes,
                    max_total_bytes: capability.limits.max_total_bytes,
                    max_width: capability.limits.max_width,
                    max_height: capability.limits.max_height,
                    media_types: capability.limits.media_types.clone(),
                })
                .collect(),
            output: capabilities
                .output
                .iter()
                .copied()
                .map(modality_descriptor)
                .collect(),
            streaming: capabilities.streaming,
            temperature: capabilities.temperature,
            reasoning: capabilities.reasoning,
            web_search: capabilities.web_search,
            function_calling: capabilities.tools.function_calling,
            parallel_tool_calls: capabilities.tools.parallel_tool_calls,
            custom_tools: capabilities.tools.custom_tools,
            freeform_tools: capabilities.tools.freeform_tools,
        },
        reasoning,
        pricing,
    }
}

fn protocol_label(protocol: ProviderWireProtocol) -> &'static str {
    match protocol {
        ProviderWireProtocol::Responses => "responses",
        ProviderWireProtocol::ChatCompletions => "chat_completions",
    }
}

fn modality_descriptor(modality: ModelModality) -> ModelModalityDto {
    match modality {
        ModelModality::Text => ModelModalityDto::Text,
        ModelModality::Image => ModelModalityDto::Image,
        ModelModality::Audio => ModelModalityDto::Audio,
        ModelModality::Video => ModelModalityDto::Video,
        ModelModality::File => ModelModalityDto::File,
    }
}

fn input_source_descriptor(source: pl_model::ModelInputSource) -> ModelInputSourceDto {
    match source {
        pl_model::ModelInputSource::Local => ModelInputSourceDto::Local,
        pl_model::ModelInputSource::RemoteUrl => ModelInputSourceDto::RemoteUrl,
    }
}

fn stable_revision(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn builtin_registry_contains_gpt56_and_shared_mimo_catalog() {
        let registry = ProviderCatalogRegistry::builtin();
        registry.validate().unwrap();
        let snapshot = registry.snapshot().unwrap();

        let openai = &snapshot.model_catalogs["openai"];
        assert!(openai.models.iter().any(|model| model.id == "gpt-5.6-sol"));
        for model in &openai.models {
            assert_eq!(model.transport.protocol, "responses");
            assert_eq!(
                model.transport.connection_modes,
                vec![
                    ProviderConnectionModeDescriptor {
                        id: "web_socket".to_string(),
                        display_name: "WebSocket".to_string(),
                    },
                    ProviderConnectionModeDescriptor {
                        id: "http".to_string(),
                        display_name: "HTTP".to_string(),
                    },
                ]
            );
            assert_eq!(model.transport.default_connection_mode, "web_socket");
        }
        let mimo = &snapshot.model_catalogs["mimo"];
        assert_eq!(
            mimo.models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["mimo-v2.5-pro", "mimo-v2.5", "mimo-v2-pro", "mimo-v2-omni"]
        );
        assert_eq!(
            snapshot
                .presets
                .iter()
                .filter(|preset| preset.model_catalog_id == "mimo")
                .count(),
            2
        );
    }

    #[test]
    fn deepseek_preset_exposes_native_hosted_search_dialect() {
        let snapshot = ProviderCatalogRegistry::builtin().snapshot().unwrap();
        let deepseek = snapshot
            .presets
            .iter()
            .find(|preset| preset.id == "deepseek")
            .unwrap();

        assert_eq!(snapshot.schema_version, 9);
        assert!(deepseek.service_capabilities.web_search.hosted_responses);
        assert_eq!(
            deepseek.service_capabilities.web_search.hosted_dialect,
            "deepseek_responses"
        );
    }

    #[test]
    fn snapshot_revision_is_stable_and_secret_free() {
        let registry = ProviderCatalogRegistry::builtin();
        let first = registry.snapshot().unwrap();
        let second = registry.snapshot().unwrap();

        assert_eq!(first.revision, second.revision);
        let json = serde_json::to_string(&first).unwrap();
        assert!(!json.contains("bearer_token"));
    }

    #[test]
    fn snapshot_revision_covers_connection_mode_order() {
        let registry = ProviderCatalogRegistry::builtin();
        let original = registry.snapshot().unwrap();
        let mut reordered = registry;
        reordered
            .model_catalogs
            .get_mut(&ModelCatalogId::new("openai").unwrap())
            .unwrap()
            .models
            .first_mut()
            .unwrap()
            .transport
            .supported_connection_modes
            .reverse();

        let reordered = reordered.snapshot().unwrap();

        assert_ne!(original.revision, reordered.revision);
    }

    #[test]
    fn snapshot_revision_covers_service_capabilities() {
        let registry = ProviderCatalogRegistry::builtin();
        let original = registry.snapshot().unwrap();
        let mut changed = registry;
        changed
            .presets
            .iter_mut()
            .find(|preset| preset.id.as_str() == "openai")
            .unwrap()
            .service_capabilities = ProviderServiceCapabilities::default();

        let changed = changed.snapshot().unwrap();

        assert_ne!(original.revision, changed.revision);
    }

    #[test]
    fn registry_allows_responses_capability_for_a_mixed_protocol_catalog() {
        let mut registry = ProviderCatalogRegistry::builtin();
        registry
            .presets
            .iter_mut()
            .find(|preset| preset.id.as_str() == "deepseek")
            .unwrap()
            .service_capabilities
            .web_search
            .hosted_responses = true;

        registry.validate().unwrap();
    }
}
