//! Product selection of the model-owned hosted coordinator.
use pl_model::{config::ResolvedModelRoute, provider::ProviderWireProtocol, runtime::HostedTool};
pub(crate) fn hosted_tool(route: &ResolvedModelRoute) -> Option<HostedTool> {
    let supported = route.model.binding.transport.protocol == ProviderWireProtocol::Responses
        && route
            .model
            .capabilities
            .supports_programmatic_tool_calling()
        && route
            .model
            .binding
            .request
            .supports_programmatic_tool_calling()
        && route
            .endpoint
            .service_capabilities
            .responses_tools
            .programmatic_tool_calling;
    supported.then_some(HostedTool::ProgrammaticToolCalling)
}
