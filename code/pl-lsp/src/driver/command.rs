//! 用户在配置中声明的自定义 server 使用的通用命令 driver。
//!
//! 探测执行 `<command> --version`；没有可修复的组件语义，repair 一律拒绝。

use futures::FutureExt;
use futures::future::BoxFuture;

use super::{
    CommandProbeError, LspProbeOutcome, LspRepairError, LspResolvedCommand, LspServerDriver,
    PROBE_TIMEOUT, run_command_capture,
};
use crate::types::LspMissingComponent;

/// 无特殊初始化需求的通用命令 driver。
pub(crate) struct CommandDriver;

impl CommandDriver {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Default for CommandDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl LspServerDriver for CommandDriver {
    fn probe<'a>(&'a self, command: &'a LspResolvedCommand) -> BoxFuture<'a, LspProbeOutcome> {
        async move {
            match run_command_capture(
                &command.program,
                &["--version"],
                PROBE_TIMEOUT,
                "version check",
            )
            .await
            {
                Ok(output) => LspProbeOutcome::Ready {
                    version: String::from_utf8_lossy(&output).trim().to_string(),
                },
                Err(CommandProbeError::MissingCommand) => LspProbeOutcome::MissingCommand {
                    message: format!(
                        "`{}` command not found on PATH; verify the [lsp.servers] configuration",
                        command.program
                    ),
                },
                Err(CommandProbeError::Failed { message, .. }) => {
                    LspProbeOutcome::Failed { message }
                }
            }
        }
        .boxed()
    }

    fn repair<'a>(
        &'a self,
        _component: &'a LspMissingComponent,
    ) -> BoxFuture<'a, Result<(), LspRepairError>> {
        std::future::ready(Err(LspRepairError::NotSupported)).boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn repair_is_not_supported_for_custom_servers() {
        let driver = CommandDriver::new();
        let component = LspMissingComponent {
            component: "unused".to_string(),
            repair_hint: "unused".to_string(),
        };

        let error = driver.repair(&component).await.unwrap_err();

        assert!(matches!(&error, LspRepairError::NotSupported), "{error:?}");
    }

    #[tokio::test]
    async fn probe_reports_missing_command_for_unknown_program() {
        let driver = CommandDriver::new();
        let command = LspResolvedCommand {
            program: "definitely-not-a-language-server-pure-test".to_string(),
            args: Vec::new(),
        };

        let outcome = driver.probe(&command).await;

        assert!(
            matches!(&outcome, LspProbeOutcome::MissingCommand { .. }),
            "{outcome:?}"
        );
    }
}
