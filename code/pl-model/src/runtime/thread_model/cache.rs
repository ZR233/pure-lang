//! Provider hints stay bound to the Thread and route; diagnostics describe changing prefixes.
use crate::runtime::ModelRuntime;

pub(super) fn key(runtime: &ModelRuntime, thread_id: &str) -> Option<String> {
    if !runtime
        .endpoint()
        .effective_prompt_cache_policy(runtime.model())
        .uses_prompt_cache_key()
    {
        return None;
    }
    Some(crate::runtime::derive_prompt_cache_key(
        &crate::runtime::binding_cache_namespace(
            runtime.provider_instance_id(),
            runtime.endpoint(),
        ),
        thread_id,
        &runtime.model().slug,
        runtime.model().binding.transport.protocol,
    ))
}
