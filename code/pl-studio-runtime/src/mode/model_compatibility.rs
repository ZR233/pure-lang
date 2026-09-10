//! 冻结 Profile 的模型路由解析与 Thread Mode 模型校验。

use crate::Result;

pub(crate) fn validate_thread_mode_model(
    mode: Option<&crate::mode::RegisteredThreadMode>,
    model: &pl_model::model::ModelInfo,
) -> Result<()> {
    if let Some(mode) = mode
        && mode.workflow().is_some()
        && !model.capabilities.supports_function_calling()
    {
        return Err(crate::PureError::ConfigError(format!(
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
        let manager = crate::mode::ThreadModeManager::default();
        crate::studio::thread::register_builtins(&manager).expect("register built-in modes");
        let snapshot = manager.snapshot();
        let task = snapshot
            .mode(&pl_protocol::ThreadModeId::task())
            .expect("task mode");
        let simple = snapshot
            .mode(&pl_protocol::ThreadModeId::simple())
            .expect("simple mode");
        let mut model = pl_model::model::ModelInfo::compatible("hosted-only-model");
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
