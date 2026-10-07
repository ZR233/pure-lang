//! Provider-native history compatibility belongs to the selected model adapter.
use crate::completion::ModelContextItem;
use crate::provider::ProviderWireProtocol;
use crate::{PureError, Result};

use super::ModelRuntime;

impl ModelRuntime {
    /// Checks whether this binding can consume every supplied native context item.
    ///
    /// # Errors
    /// Rejects incompatible native material without rewriting or dropping history.
    pub fn validate_context(&self, items: &[ModelContextItem]) -> Result<()> {
        validate(self.model().binding.transport.protocol, items)
    }
    /// Projects tools and checks capabilities before the caller admits this frozen request.
    ///
    /// # Errors
    /// Rejects unsupported content, tool schemas and model parameters without starting IO.
    pub fn prepare_request(
        &self,
        request: crate::completion::CompletionRequest,
    ) -> Result<crate::completion::CompletionRequest> {
        project_request(
            self.endpoint(),
            self.model(),
            self.provider_instance_id(),
            request,
        )
    }
}

pub(super) fn validate(protocol: ProviderWireProtocol, items: &[ModelContextItem]) -> Result<()> {
    if protocol == ProviderWireProtocol::Responses {
        return Ok(());
    }
    for item in items {
        match item {
            ModelContextItem::Compaction { .. } | ModelContextItem::Responses { .. } => {
                return Err(PureError::ConfigError(
                    "session history contains native Responses context (for example an encrypted \
                     compaction checkpoint) that a Chat Completions model cannot consume; use a \
                     Responses-protocol model for this session or start a new session"
                        .into(),
                ));
            }
            ModelContextItem::Message { .. }
            | ModelContextItem::ToolResult { .. }
            | ModelContextItem::ToolMedia { .. } => {}
        }
    }
    Ok(())
}

pub(super) fn project_request(
    endpoint: &crate::provider::ProviderEndpoint,
    model: &crate::model::ModelInfo,
    provider_instance_id: &str,
    mut request: crate::completion::CompletionRequest,
) -> Result<crate::completion::CompletionRequest> {
    validate(model.binding.transport.protocol, &request.input)?;
    if model.binding.transport.protocol == ProviderWireProtocol::ChatCompletions
        && model
            .capabilities
            .interleaved
            .as_ref()
            .is_some_and(|capability| {
                capability.field == crate::model::ReasoningInterleavedField::ReasoningDetails
            })
        && request
            .input
            .iter()
            .filter_map(ModelContextItem::as_message)
            .any(|message| {
                message
                    .reasoning_content
                    .as_ref()
                    .is_some_and(|text| !text.is_empty())
            })
    {
        return Err(PureError::ConfigError(
            "typed reasoning_details cannot be reconstructed from text reasoning history".into(),
        ));
    }
    drop_unreadable_cross_provider_reasoning(
        &mut request,
        crate::runtime::binding_cache_namespace(provider_instance_id, endpoint).as_str(),
    );
    let capabilities = model
        .capabilities
        .clone()
        .with_native_custom_tools(endpoint.uses_native_custom_tools());
    let native = endpoint.uses_native_custom_tools()
        && capabilities.supports_custom_tools()
        && capabilities.supports_freeform_tools();
    let projection = if native {
        crate::completion::tool_schema::CustomToolProjection::Native
    } else {
        crate::completion::tool_schema::CustomToolProjection::ToFunction
    };
    let mut request = request.provider_compatible(projection);
    if !request
        .tools
        .iter()
        .any(crate::completion::ToolSpec::is_programmatic_tool_calling)
    {
        for tool in &mut request.tools {
            match tool {
                crate::completion::ToolSpec::Function {
                    allowed_callers,
                    output_schema,
                    ..
                }
                | crate::completion::ToolSpec::Custom {
                    allowed_callers,
                    output_schema,
                    ..
                } => {
                    if allowed_callers.contains(&pl_protocol::ToolCallerMode::Programmatic) {
                        if !allowed_callers.contains(&pl_protocol::ToolCallerMode::Direct) {
                            return Err(PureError::ConfigError(
                                "programmatic-only tool requires the selected hosted coordinator"
                                    .into(),
                            ));
                        }
                        allowed_callers.clear();
                        *output_schema = None;
                    }
                }
                crate::completion::ToolSpec::WebSearch { .. }
                | crate::completion::ToolSpec::ProgrammaticToolCalling => {}
            }
        }
    }

    request.validate_against(&model.slug, &capabilities)?;
    Ok(request)
}

/// 出站投影：目标 provider 无法解释的来源 reasoning 不回放。
///
/// 原生 reasoning 的 `encrypted_content` 由产生它的 provider 账户/部署加密；切换 provider 后
/// 目标无法解密（严格兼容网关会以 `invalid_encrypted_content` 失败）。这里按帧已记录的来源隔离
/// 身份与目标隔离身份比较：来源不同的原生回放只移除 reasoning item，保留 assistant 文本、工具
/// 身份与工具输出，且不重放工具副作用；来源身份缺失（旧帧或无证据）时维持既有逐字回放语义。
fn drop_unreadable_cross_provider_reasoning(
    request: &mut crate::completion::CompletionRequest,
    target_isolation: &str,
) {
    for span in &mut request.replay_spans {
        let Some(source) = span.source_isolation.as_deref() else {
            continue;
        };
        if source == target_isolation {
            continue;
        }
        span.output.retain(|item| {
            item.get("type").and_then(serde_json::Value::as_str) != Some("reasoning")
        });
    }
}
