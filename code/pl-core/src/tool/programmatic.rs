//! Provider-hosted programmatic tool coordinator.

use futures::FutureExt;
use pl_model::ProviderWireProtocol;
use pl_protocol::{PureError, ToolSpec};

use crate::ResolvedModelRoute;
use crate::turn::ToolEffect;

use super::{
    AgentToolSet, BoxFuture, Tool, ToolCallContext, ToolExecution, ToolGroupId, ToolInput,
    ToolResult,
};

pub const TOOL_PROGRAMMATIC_TOOL_CALLING: &str = "programmatic_tool_calling";
const PROGRAMMATIC_TOOL_GROUP: &str = "programmatic_tool_calling";

/// Responses-native programmatic tool coordinator.
///
/// This value is registered and snapshotted like every other tool, but execution is
/// delegated to the selected provider and therefore never enters local dispatch.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProgrammaticToolCallingTool;

impl Tool for ProgrammaticToolCallingTool {
    fn name(&self) -> &str {
        TOOL_PROGRAMMATIC_TOOL_CALLING
    }

    fn description(&self) -> &str {
        "Coordinate eligible read-only tools in provider-hosted code."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({ "type": "object", "properties": {} })
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(ToolEffect::Read)
    }

    fn execution(&self) -> ToolExecution {
        ToolExecution::ProviderHosted
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::ProgrammaticToolCalling
    }

    fn execute<'a>(
        &'a self,
        _input: ToolInput,
        _context: ToolCallContext,
    ) -> BoxFuture<'a, Result<ToolResult, PureError>> {
        async {
            Err(PureError::ToolExecutionFailed {
                tool: TOOL_PROGRAMMATIC_TOOL_CALLING.to_string(),
                error: "programmatic tool calling is executed by the model provider".to_string(),
            })
        }
        .boxed()
    }
}

/// Reconciles the hosted coordinator for one resolved route.
///
/// Registration requires Responses wire plus matching model request-profile, model
/// capability and endpoint service capability. Unsupported routes remove the group.
///
/// # Errors
///
/// Returns a registration conflict if another local group already owns the hosted
/// coordinator's visible name.
pub fn reconcile_programmatic_tool_calling(
    tools: &AgentToolSet,
    route: &ResolvedModelRoute,
) -> crate::Result<()> {
    let supported = route.model.transport.protocol == ProviderWireProtocol::Responses
        && route
            .model
            .capabilities
            .supports_programmatic_tool_calling()
        && route
            .model
            .request_profile
            .responses_programmatic_tool_calling
        && route
            .endpoint
            .service_capabilities
            .responses_tools
            .programmatic_tool_calling;
    let group = ToolGroupId::new(PROGRAMMATIC_TOOL_GROUP);
    if supported {
        tools.install(
            group,
            vec![std::sync::Arc::new(ProgrammaticToolCallingTool)],
        )
    } else {
        tools.uninstall(&group);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use pl_model::{ModelInfo, ProviderEndpoint};

    use super::*;
    use crate::{AgentRoleId, ProviderId};

    fn route() -> ResolvedModelRoute {
        let mut model = ModelInfo::fallback("hosted-model");
        model.transport = pl_model::ModelTransportProfile::responses_http();
        model.capabilities.tools.programmatic_tool_calling = true;
        model.request_profile.responses_programmatic_tool_calling = true;
        let mut endpoint = ProviderEndpoint::openai(None);
        endpoint
            .service_capabilities
            .responses_tools
            .programmatic_tool_calling = true;
        ResolvedModelRoute {
            role: AgentRoleId::new("test").expect("role"),
            provider_id: ProviderId::new("test").expect("provider"),
            endpoint,
            model,
            effort: None,
        }
    }

    #[test]
    fn registration_requires_every_hosted_capability_gate() {
        let manager = super::super::ToolManager::new();
        let tools = manager.agent_tool_set("agent", super::super::GlobalToolInheritance::Isolated);
        let supported = route();
        reconcile_programmatic_tool_calling(&tools, &supported).expect("register supported PTC");
        assert!(
            tools
                .freeze()
                .specs()
                .contains(&ToolSpec::ProgrammaticToolCalling)
        );

        let mut unsupported = supported.clone();
        unsupported
            .endpoint
            .service_capabilities
            .responses_tools
            .programmatic_tool_calling = false;
        reconcile_programmatic_tool_calling(&tools, &unsupported).expect("remove unsupported PTC");
        assert!(tools.freeze().specs().is_empty());
    }
}
