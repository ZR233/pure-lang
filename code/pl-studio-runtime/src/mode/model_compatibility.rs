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
