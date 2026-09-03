//! Provider-hosted programmatic tool coordinator.

use futures::FutureExt;
use pl_model::provider::ProviderWireProtocol;
use pl_protocol::{PureError, Result, ToolSpec};

use super::{
    AgentToolSet, BoxFuture, DynTool, ToolDefinition, ToolExecution, ToolExecutor, ToolGroupId,
    ToolInstallGroup, ToolInvocation, ToolName, ToolPolicy, ToolResult,
};
use crate::ResolvedModelRoute;

pub const TOOL_PROGRAMMATIC_TOOL_CALLING: &str = "programmatic_tool_calling";
const PROGRAMMATIC_TOOL_GROUP: &str = "programmatic_tool_calling";

/// Responses-native programmatic tool coordinator.
///
/// This value is registered and snapshotted like every other tool, but execution is
/// delegated to the selected provider and therefore never enters local dispatch.
#[derive(Debug, Clone)]
pub struct ProgrammaticToolCallingTool {
    definition: ToolDefinition,
    policy: ToolPolicy,
}

impl Default for ProgrammaticToolCallingTool {
    fn default() -> Self {
        Self {
            definition: ToolDefinition::from_trusted_spec(
                ToolName::builtin(TOOL_PROGRAMMATIC_TOOL_CALLING),
                ToolSpec::ProgrammaticToolCalling,
            ),
            policy: ToolPolicy::read_only(),
        }
    }
}

impl ToolExecutor for ProgrammaticToolCallingTool {
    fn definition(&self) -> &ToolDefinition {
        &self.definition
    }

    fn policy(&self) -> &ToolPolicy {
        &self.policy
    }

    fn execution(&self) -> ToolExecution {
        ToolExecution::ProviderHosted
    }

    fn execute(&self, _invocation: ToolInvocation) -> BoxFuture<'_, Result<ToolResult>> {
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
        tools.install(ToolInstallGroup::direct(
            group,
            vec![DynTool::new_executor(ProgrammaticToolCallingTool::default())],
        ))
    } else {
        tools.uninstall(&group);
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::{AgentRoleId, ProviderId};
    use pl_model::model::ModelInfo;
    use pl_model::provider::ProviderEndpoint;

    fn route() -> ResolvedModelRoute {
        let mut model = ModelInfo::fallback("hosted-model");
        model.transport = pl_model::model::ModelTransportProfile::responses_http();
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
