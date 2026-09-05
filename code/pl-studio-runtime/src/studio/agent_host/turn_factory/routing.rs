//! 冻结 Profile 的模型路由解析与 Thread Mode 模型校验。

use crate::Result;

use super::errors::turn_error;

pub(super) fn resolve_frozen_profile_route(
    config: &crate::config::StudioConfig,
    profile: &pl_protocol::AgentProfileSnapshot,
) -> Result<pl_core::ResolvedModelRoute> {
    let role = pl_core::AgentRoleId::new(profile.profile_id.clone())?;
    let mut models = config.models.clone();
    models.routes.insert(
        role.clone(),
        pl_core::ModelRouteConfig {
            provider: pl_core::ProviderId::new(profile.provider_id.clone())?,
            model: profile.model.clone(),
            effort: profile
                .effort
                .as_ref()
                .map(|effort| pl_core::ReasoningEffort::new(effort.clone())),
        },
    );
    models.resolve(&role)
}

pub(super) fn validate_thread_mode_model(
    mode: Option<&pl_core::RegisteredThreadMode>,
    model: &pl_core::ModelInfo,
) -> Result<()> {
    if let Some(mode) = mode
        && mode.workflow().is_some()
        && !model.capabilities.supports_function_calling()
    {
        return Err(turn_error(format!(
            "selected Thread Mode `{}` requires workflow function tools, but model `{}` does not support function calling; choose a function-calling model or a Mode without a workflow",
            mode.descriptor().id,
            model.slug
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_mode_rejects_a_model_that_cannot_expose_its_tools() {
        let manager = pl_core::ThreadModeManager::default();
        crate::studio::thread::register_builtins(&manager).expect("register built-in modes");
        let snapshot = manager.snapshot();
        let task = snapshot
            .mode(&pl_protocol::ThreadModeId::task())
            .expect("task mode");
        let simple = snapshot
            .mode(&pl_protocol::ThreadModeId::simple())
            .expect("simple mode");
        let mut model = pl_core::ModelInfo::compatible("hosted-only-model");
        model.capabilities.tools.function_calling = false;

        let error = validate_thread_mode_model(Some(&task), &model).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("requires workflow function tools")
        );
        validate_thread_mode_model(Some(&simple), &model).expect("prompt-only mode needs no tools");
        validate_thread_mode_model(None, &model).expect("child session has no root workflow");
    }
}
