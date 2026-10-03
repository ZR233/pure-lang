//! Private provider wire DTOs. Product prompts, shell settings and tool permissions are intentionally absent.
use super::ModelCatalogQueryError;
use crate::model::*;
use crate::provider::ProviderAdapterKind;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Deserialize)]
struct Envelope {
    data: Option<Vec<Record>>,
    models: Option<Vec<Record>>,
    error: Option<serde::de::IgnoredAny>,
    success: Option<bool>,
}

#[derive(Deserialize)]
struct Record {
    id: Option<String>,
    slug: Option<String>,
    name: Option<String>,
    display_name: Option<String>,
    description: Option<String>,
    context_window: Option<u64>,
    max_context_window: Option<u64>,
    auto_compact_token_limit: Option<u64>,
    max_output_tokens: Option<u64>,
    default_temperature: Option<f32>,
    input_modalities: Option<Vec<ModelModality>>,
    output_modalities: Option<Vec<ModelModality>>,
    effort: Option<Effort>,
    supported_reasoning_levels: Option<Vec<ReasoningLevel>>,
    default_reasoning_level: Option<String>,
    parameters: Option<Vec<ModelParameter>>,
    binding: Option<ModelBinding>,
    capabilities: Option<Capabilities>,
    supports_parallel_tool_calls: Option<bool>,
    supports_function_calling: Option<bool>,
    supports_temperature: Option<bool>,
    supports_streaming: Option<bool>,
    supports_reasoning: Option<bool>,
}

impl Record {
    fn identifier(&self) -> Result<&str, ModelCatalogQueryError> {
        let id = match (self.id.as_deref(), self.slug.as_deref()) {
            (Some(id), None) | (None, Some(id)) => id,
            (Some(id), Some(slug)) if id == slug => id,
            _ => return Err(ModelCatalogQueryError::Protocol),
        };
        if id.trim().is_empty() {
            return Err(ModelCatalogQueryError::Protocol);
        }
        Ok(id)
    }
}

#[derive(Deserialize)]
struct Effort {
    supported_levels: Vec<String>,
    default_level: Option<String>,
}
#[derive(Deserialize)]
struct ReasoningLevel {
    effort: String,
    // Descriptions are presentation metadata, not candidates or protocol instructions.
    #[serde(rename = "description")]
    _description: Option<String>,
}
#[derive(Deserialize)]
struct Capabilities {
    input: Option<Vec<ModelInputCapability>>,
    output: Option<Vec<ModelModality>>,
    streaming: Option<bool>,
    temperature: Option<bool>,
    reasoning: Option<bool>,
    web_search: Option<bool>,
    tools: Option<Tools>,
}
#[derive(Deserialize)]
struct Tools {
    function_calling: Option<bool>,
    parallel_tool_calls: Option<bool>,
    custom_tools: Option<bool>,
    freeform_tools: Option<bool>,
    programmatic_tool_calling: Option<bool>,
}

pub(super) fn models(
    bytes: &[u8],
    adapter: ProviderAdapterKind,
    previous: Option<&[ModelInfo]>,
    defaults: &[ModelInfo],
) -> Result<Vec<ModelInfo>, ModelCatalogQueryError> {
    let envelope: Envelope =
        serde_json::from_slice(bytes).map_err(|_| ModelCatalogQueryError::Protocol)?;
    if envelope.error.is_some() || envelope.success == Some(false) {
        return Err(ModelCatalogQueryError::Protocol);
    }
    let records = match (envelope.data, envelope.models) {
        (Some(records), None) | (None, Some(records)) => records,
        _ => return Err(ModelCatalogQueryError::Protocol),
    };
    // Reject duplicate IDs before cloning inherited metadata. A repeated ID must not
    // multiply a large cached declaration into an unbounded normalization allocation.
    {
        let mut ids = HashSet::new();
        for record in &records {
            if !ids.insert(record.identifier()?) {
                return Err(ModelCatalogQueryError::Protocol);
            }
        }
    }
    let metadata = defaults
        .iter()
        .chain(previous.into_iter().flatten())
        .map(|model| (model.slug.as_str(), model))
        .collect::<HashMap<_, _>>();
    let result = records
        .into_iter()
        .map(|record| normalize(record, adapter, &metadata))
        .collect::<Result<Vec<_>, _>>()?;
    validate_inventory(&result).map_err(|_| ModelCatalogQueryError::Protocol)?;
    Ok(result)
}

fn normalize(
    record: Record,
    adapter: ProviderAdapterKind,
    metadata: &HashMap<&str, &ModelInfo>,
) -> Result<ModelInfo, ModelCatalogQueryError> {
    let slug = record.identifier()?.to_owned();
    let known = metadata.get(slug.as_str()).copied();
    let mut model = known.cloned().unwrap_or_else(|| minimal(&slug, adapter));
    model.pricing = ModelPricing::Unknown;
    if let Some(value) = record.display_name.or(record.name) {
        model.display_name = value;
    }
    if let Some(value) = record.description {
        model.description = Some(value);
    }
    if let Some(value) = record.context_window {
        model.context_window = Some(value);
    }
    if let Some(value) = record.max_context_window {
        model.max_context_window = Some(value);
    }
    if let Some(value) = record.max_output_tokens {
        model.max_output_tokens = Some(value);
    }
    if let Some(value) = record.auto_compact_token_limit {
        model.auto_compact_token_limit = Some(value);
    }
    if let Some(value) = record.default_temperature {
        model.default_temperature = Some(value);
    }
    let explicit_binding = record.binding.is_some();
    if let Some(value) = record.binding {
        // Discovery may declare transport/media, not override endpoint credentials or inject product instructions.
        if value.request.api_model.is_some()
            || !value.request.headers.is_empty()
            || !value.request.body.is_empty()
            || !super::valid_discovery_transport(&value.transport, adapter)
        {
            return Err(ModelCatalogQueryError::Protocol);
        }
        model.binding = value;
        // Canonicalize ordering, never restore a connection candidate the API withdrew.
        model.binding.transport.supported_connection_modes = super::discovery_transport(adapter)
            .supported_connection_modes
            .into_iter()
            .filter(|mode| {
                model
                    .binding
                    .transport
                    .supported_connection_modes
                    .contains(mode)
            })
            .collect();
    }
    let supplied_media_profile = record
        .capabilities
        .as_ref()
        .is_some_and(|c| c.input.is_some());
    let explicit_reasoning = record
        .supports_reasoning
        .or_else(|| record.capabilities.as_ref().and_then(|c| c.reasoning));
    if let Some(capabilities) = record.capabilities {
        if let Some(value) = capabilities.input {
            model.capabilities.input = value;
        }
        if let Some(value) = capabilities.output {
            model.capabilities.output = value;
        }
        if let Some(value) = capabilities.streaming {
            model.capabilities.streaming = value;
        }
        if let Some(value) = capabilities.temperature {
            model.capabilities.temperature = value;
        }
        if let Some(value) = capabilities.reasoning {
            model.capabilities.reasoning = value;
        }
        if let Some(value) = capabilities.web_search {
            model.capabilities.web_search = value;
        }
        if let Some(tools) = capabilities.tools {
            if let Some(value) = tools.function_calling {
                model.capabilities.tools.function_calling = value;
            }
            if let Some(value) = tools.parallel_tool_calls {
                model.capabilities.tools.parallel_tool_calls = value;
            }
            if let Some(value) = tools.custom_tools {
                model.capabilities.tools.custom_tools = value;
            }
            if let Some(value) = tools.freeform_tools {
                model.capabilities.tools.freeform_tools = value;
            }
            if let Some(value) = tools.programmatic_tool_calling {
                model.capabilities.tools.programmatic_tool_calling = value;
            }
        }
    }
    if let Some(value) = record.supports_parallel_tool_calls {
        model.capabilities.tools.parallel_tool_calls = value;
    }
    if let Some(value) = record.supports_function_calling {
        model.capabilities.tools.function_calling = value;
    }
    if let Some(value) = record.supports_temperature {
        model.capabilities.temperature = value;
    }
    if let Some(value) = record.supports_streaming {
        model.capabilities.streaming = value;
    }
    if let Some(value) = record.supports_reasoning {
        model.capabilities.reasoning = value;
    }
    if let Some(modalities) = record.input_modalities {
        model.capabilities.input = modalities
            .into_iter()
            .map(|modality| match modality {
                ModelModality::Text => ModelInputCapability::text(),
                modality => ModelInputCapability::media(
                    modality,
                    vec![ModelInputSource::Local, ModelInputSource::RemoteUrl],
                ),
            })
            .collect();
        if !explicit_binding {
            model.binding.request.media = if model
                .capabilities
                .supports_input_modality(ModelModality::Image)
            {
                image_media_profiles(
                    MediaWireFormat::ResponsesInputImage,
                    if adapter == ProviderAdapterKind::DeepSeek {
                        MediaSendOrder::ProviderFileFirst
                    } else {
                        MediaSendOrder::RemoteUrlFirst
                    },
                )
            } else {
                Vec::new()
            };
        }
    } else if supplied_media_profile && !explicit_binding {
        // A complete capability declaration needs its corresponding explicit binding; never invent a video/file codec.
        model
            .binding
            .request
            .media
            .retain(|p| model.capabilities.supports_input_modality(p.modality));
    }
    if let Some(value) = record.output_modalities {
        model.capabilities.output = value;
    }
    let default_reasoning_level = record.default_reasoning_level;
    let (levels, default) = match (record.effort, record.supported_reasoning_levels) {
        (Some(effort), None) => (Some(effort.supported_levels), effort.default_level),
        (None, Some(levels)) => (Some(levels.into_iter().map(|l| l.effort).collect()), None),
        (None, None) => (None, None),
        _ => return Err(ModelCatalogQueryError::Protocol),
    };
    if record.parameters.is_some() && levels.is_some() {
        return Err(ModelCatalogQueryError::Protocol);
    }
    if let Some(parameters) = record.parameters {
        let effort_path = if adapter == ProviderAdapterKind::OpenAi {
            "reasoning.effort"
        } else {
            "reasoning_effort"
        };
        if parameters.iter().any(|p| {
            p.name != "effort"
                || p.wire.iter().any(|(candidate, w)| {
                    !w.remove.is_empty()
                        || w.set.len() != 1
                        || w.set[0].path != effort_path
                        || w.set[0].value != serde_json::Value::String(candidate.clone())
                })
        }) {
            return Err(ModelCatalogQueryError::Protocol);
        }
        model.parameters = parameters;
    }
    if let Some(levels) = levels {
        model.parameters.retain(|p| p.name != "effort");
        if !levels.is_empty() {
            model.parameters.push(effort_parameter(levels, adapter));
        }
        if explicit_reasoning.is_none() {
            model.capabilities.reasoning = !model.supported_efforts().is_empty();
        }
    }
    for default in default.into_iter().chain(default_reasoning_level) {
        if !model.supported_efforts().contains(&default) {
            return Err(ModelCatalogQueryError::Protocol);
        }
    }
    if adapter == ProviderAdapterKind::DeepSeek {
        model.binding.request.body.remove("thinking");
        if !model.supported_efforts().is_empty() {
            model
                .binding
                .request
                .body
                .insert("thinking".into(), serde_json::json!({"type":"enabled"}));
        }
    }
    model
        .validate()
        .map_err(|_| ModelCatalogQueryError::Protocol)?;
    Ok(model)
}

fn minimal(slug: &str, adapter: ProviderAdapterKind) -> ModelInfo {
    // Responses streaming is the provider protocol baseline, not a promise of tools/vision/embedding support.
    ModelInfo {
        slug: slug.into(),
        display_name: slug.into(),
        description: None,
        context_window: None,
        max_context_window: None,
        auto_compact_token_limit: None,
        default_temperature: None,
        max_output_tokens: None,
        pricing: ModelPricing::Unknown,
        parameters: Vec::new(),
        binding: ModelBinding {
            transport: super::discovery_transport(adapter),
            request: ModelRequestProfile::responses(),
        },
        capabilities: ModelCapabilities {
            streaming: true,
            temperature: false,
            reasoning: false,
            web_search: false,
            input: vec![ModelInputCapability::text()],
            output: vec![ModelModality::Text],
            tools: ToolCapabilities::default(),
            interleaved: None,
            prompt_cache: Default::default(),
        },
        truncation_policy: TruncationPolicy::default(),
        base_instructions: String::new(),
    }
}

fn effort_parameter(candidates: Vec<String>, adapter: ProviderAdapterKind) -> ModelParameter {
    let path = if adapter == ProviderAdapterKind::OpenAi {
        "reasoning.effort"
    } else {
        "reasoning_effort"
    };
    ModelParameter {
        name: "effort".into(),
        label: None,
        wire: candidates
            .iter()
            .map(|value| {
                (
                    value.clone(),
                    ParameterWire {
                        set: vec![WireAssignment {
                            path: path.into(),
                            value: serde_json::Value::String(value.clone()),
                        }],
                        remove: Vec::new(),
                    },
                )
            })
            .collect(),
        candidates,
    }
}
