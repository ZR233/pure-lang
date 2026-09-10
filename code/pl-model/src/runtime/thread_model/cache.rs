//! Stable cache hints use encoded-prefix choices, never Thread execution identities.
use pl_core::model::{ModelError, ModelFailureKind};
use sha2::{Digest, Sha256};

use crate::{completion::CompletionRequest, runtime::ModelRuntime};

pub(super) fn key(
    runtime: &ModelRuntime,
    request: &CompletionRequest,
) -> Result<Option<String>, ModelError> {
    if !runtime
        .endpoint()
        .effective_prompt_cache_policy(runtime.model())
        .uses_prompt_cache_key()
    {
        return Ok(None);
    }
    let isolation =
        crate::runtime::binding_cache_namespace(runtime.provider_instance_id(), runtime.endpoint());
    let mut prefix = serde_json::json!({
        "version": "pl-model/thread-prefix/v1",
        "isolation": isolation,
        "model": runtime.model().slug,
        "adapter": runtime.endpoint().adapter,
        "protocol": runtime.model().binding.transport.protocol,
        "toolWirePolicy": runtime.endpoint().tool_wire_policy,
        "instructions": request.instructions,
        "tools": crate::completion::stable_tool_schemas(request.tools.clone()),
        "toolChoice": request.tool_choice,
        "parallelToolCalls": request.parallel_tool_calls,
        "reasoning": request.reasoning,
    });
    crate::completion::canonicalize_json(&mut prefix);
    let encoded = serde_json::to_vec(&prefix)
        .map_err(|error| super::failure(ModelFailureKind::InvalidResponse, error))?;
    Ok(Some(format!("pl:{:x}", Sha256::digest(encoded))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn generation_limits_do_not_partition_the_same_prompt_prefix() {
        let mut model = crate::model::ModelInfo::compatible("cache-test");
        model
            .binding
            .set_transport(crate::model::ModelTransportProfile {
                protocol: crate::provider::ProviderWireProtocol::Responses,
                ..model.binding.transport.clone()
            });
        let runtime =
            ModelRuntime::new(crate::provider::ProviderEndpoint::openai(None), model).unwrap();
        let mut request = CompletionRequest::builder().build();
        request.instructions = Some("Stable instruction".into());
        let original = key(&runtime, &request).unwrap();
        assert!(original.is_some());
        request.temperature = Some(0.7);
        request.max_tokens = Some(4096);
        assert_eq!(key(&runtime, &request).unwrap(), original);
        request.instructions = Some("Changed instruction".into());
        assert_ne!(key(&runtime, &request).unwrap(), original);
    }
}
