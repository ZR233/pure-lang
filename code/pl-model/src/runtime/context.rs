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
        project_request(self.endpoint(), self.model(), request)
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
                    "Native Responses context requires a compatible Responses model binding".into(),
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
    request: crate::completion::CompletionRequest,
) -> Result<crate::completion::CompletionRequest> {
    validate(model.binding.transport.protocol, &request.input)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_history_rejects_incompatible_bindings_without_rewriting_content() {
        let items = [
            ModelContextItem::Compaction {
                encrypted_content: "unchanged encrypted bytes".into(),
            },
            ModelContextItem::Responses {
                item: pl_protocol::ResponsesContextItem {
                    kind: pl_protocol::ResponsesContextItemKind::Program,
                    value: serde_json::json!({"type": "program", "id": "program-1"}),
                },
            },
        ];
        for item in &items {
            assert!(
                validate(
                    ProviderWireProtocol::ChatCompletions,
                    std::slice::from_ref(item)
                )
                .is_err()
            );
            assert!(validate(ProviderWireProtocol::Responses, std::slice::from_ref(item)).is_ok());
        }
    }
    #[test]
    fn eligible_tools_are_direct_only_when_the_hosted_coordinator_is_not_selected() {
        let endpoint = crate::provider::ProviderEndpoint::deepseek(None);
        let model = crate::model::ModelInfo::compatible("direct-only");
        let direct = crate::completion::ToolSpec::function(
            "read",
            "Read",
            serde_json::json!({"type":"object"}),
        );
        let eligible = crate::completion::programmatic_tool_declaration(direct.clone());
        let request = crate::completion::CompletionRequest::builder()
            .tools(vec![eligible])
            .build();
        let encoded = project_request(&endpoint, &model, request).unwrap();
        assert_eq!(encoded.tools, vec![direct.clone()]);
        let mut restricted = crate::completion::programmatic_tool_declaration(direct);
        if let crate::completion::ToolSpec::Function {
            allowed_callers, ..
        } = &mut restricted
        {
            *allowed_callers = vec![pl_protocol::ToolCallerMode::Programmatic];
        }
        assert!(
            project_request(
                &endpoint,
                &model,
                crate::completion::CompletionRequest::builder()
                    .tools(vec![restricted])
                    .build()
            )
            .is_err(),
            "unsupported callers cannot silently acquire direct execution"
        );
    }
}
