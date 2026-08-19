pub(super) fn options(
    provider: &pl_model::ProviderEndpoint,
    model: &pl_model::ModelInfo,
    capabilities: &pl_model::ModelCapabilities,
) -> crate::tool::ToolOrchestrationOptions {
    let responses_tools = &provider.service_capabilities.responses_tools;
    let uses_responses = model.transport.protocol == pl_model::ProviderWireProtocol::Responses;
    crate::tool::ToolOrchestrationOptions {
        tool_search: uses_responses
            && responses_tools.tool_search
            && capabilities.supports_tool_search()
            && model.request_profile.responses_tool_search,
        programmatic_tool_calling: uses_responses
            && responses_tools.programmatic_tool_calling
            && capabilities.supports_programmatic_tool_calling()
            && model.request_profile.responses_programmatic_tool_calling,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn compatible_responses_proxy_uses_eager_direct_tools() {
        let model = pl_model::default_models()
            .into_iter()
            .find(|model| model.slug == "gpt-5.6-sol")
            .unwrap();
        let capabilities = model.capabilities.clone();
        let provider = pl_model::ProviderEndpoint::openai(Some(
            "https://responses-proxy.example/v1".to_string(),
        ));

        let options = super::options(&provider, &model, &capabilities);

        assert!(!options.tool_search);
        assert!(!options.programmatic_tool_calling);
    }

    #[test]
    fn official_openai_responses_endpoint_uses_hosted_tools() {
        let model = pl_model::default_models()
            .into_iter()
            .find(|model| model.slug == "gpt-5.6-sol")
            .unwrap();
        let capabilities = model.capabilities.clone();
        let provider = pl_model::ProviderEndpoint::openai(None);

        let options = super::options(&provider, &model, &capabilities);

        assert!(options.tool_search);
        assert!(options.programmatic_tool_calling);
    }
}
