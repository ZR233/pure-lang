//! rust-analyzer 的 driver：rustup probe、缺失组件修复与 client-watcher 初始化。

use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use serde_json::Value;

use super::{
    CommandProbeError, LspProbeOutcome, LspRepairError, LspResolvedCommand, LspServerDriver,
    PROBE_TIMEOUT, run_command_capture,
};
use crate::types::LspMissingComponent;

const RUST_ANALYZER_COMPONENT: &str = "rust-analyzer";
const RUSTUP_TIMEOUT: Duration = Duration::from_secs(120);

/// rust-analyzer 生命周期 adapter。
pub(crate) struct RustAnalyzerDriver;

impl RustAnalyzerDriver {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Default for RustAnalyzerDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl LspServerDriver for RustAnalyzerDriver {
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
                    message: missing_rust_analyzer_message(),
                },
                Err(CommandProbeError::Failed { message, stderr }) => {
                    if is_rustup_missing_component_error(&stderr) {
                        missing_component()
                    } else {
                        LspProbeOutcome::Failed { message }
                    }
                }
            }
        }
        .boxed()
    }

    fn repair<'a>(
        &'a self,
        component: &'a LspMissingComponent,
    ) -> BoxFuture<'a, Result<(), LspRepairError>> {
        async move {
            if component.component != RUST_ANALYZER_COMPONENT {
                return Err(LspRepairError::NotSupported);
            }
            match run_command_capture("rustup", &["--version"], PROBE_TIMEOUT, "rustup check").await
            {
                Ok(_) | Err(CommandProbeError::Failed { .. }) => {}
                Err(CommandProbeError::MissingCommand) => {
                    return Err(LspRepairError::MissingTool {
                        tool: "rustup".to_string(),
                    });
                }
            }
            run_command_capture(
                "rustup",
                &["component", "add", RUST_ANALYZER_COMPONENT],
                RUSTUP_TIMEOUT,
                "rustup component add",
            )
            .await
            .map(|_| ())
            .map_err(|error| match error {
                CommandProbeError::MissingCommand => LspRepairError::MissingTool {
                    tool: "rustup".to_string(),
                },
                CommandProbeError::Failed { message, .. } => LspRepairError::Failed(message),
            })
        }
        .boxed()
    }

    fn initialization_options(&self) -> Value {
        rust_analyzer_settings()
    }

    fn configuration_response(&self, section: Option<&str>) -> Value {
        match section {
            Some("rust-analyzer") | None => rust_analyzer_settings(),
            Some("rust-analyzer.files") => serde_json::json!({ "watcher": "client" }),
            Some("rust-analyzer.files.watcher") => serde_json::json!("client"),
            Some(_) => Value::Null,
        }
    }
}

fn missing_component() -> LspProbeOutcome {
    LspProbeOutcome::MissingComponent(LspMissingComponent {
        component: RUST_ANALYZER_COMPONENT.to_string(),
        repair_hint: "rust-analyzer rustup component is missing; use the repair action to run \
                      `rustup component add rust-analyzer`"
            .to_string(),
    })
}

fn missing_rust_analyzer_message() -> String {
    "rust-analyzer command not found; use the explicit repair action when rustup owns the component"
        .to_string()
}

pub(crate) fn is_rustup_missing_component_error(stderr: &str) -> bool {
    let stderr = stderr.to_ascii_lowercase();
    stderr.contains("unknown binary")
        && (stderr.contains("rust-analyzer") || stderr.contains("rust_analyzer"))
}

fn rust_analyzer_settings() -> Value {
    serde_json::json!({
        "files": {
            "watcher": "client",
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rustup_unknown_binary_is_missing_component() {
        assert!(is_rustup_missing_component_error(
            "error: Unknown binary 'rust-analyzer.exe' in official toolchain 'stable-x86_64-pc-windows-msvc'."
        ));
        assert!(!is_rustup_missing_component_error(
            "error: Unknown binary 'cargo-miri.exe' in official toolchain"
        ));
    }

    #[tokio::test]
    async fn probe_reports_missing_command_for_unknown_program() {
        let driver = RustAnalyzerDriver::new();
        let command = LspResolvedCommand {
            program: "definitely-not-rust-analyzer-pure-test".to_string(),
            args: Vec::new(),
        };

        let outcome = driver.probe(&command).await;

        assert!(
            matches!(&outcome, LspProbeOutcome::MissingCommand { .. }),
            "{outcome:?}"
        );
    }

    #[tokio::test]
    async fn repair_rejects_foreign_component() {
        let driver = RustAnalyzerDriver::new();
        let component = LspMissingComponent {
            component: "other-component".to_string(),
            repair_hint: "unused".to_string(),
        };

        let error = driver.repair(&component).await.unwrap_err();

        assert!(matches!(&error, LspRepairError::NotSupported), "{error:?}");
    }

    #[test]
    fn configuration_response_covers_rust_analyzer_sections() {
        let driver = RustAnalyzerDriver::new();

        assert_eq!(
            driver.configuration_response(None),
            serde_json::json!({ "files": { "watcher": "client" } })
        );
        assert_eq!(
            driver.configuration_response(Some("rust-analyzer.files")),
            serde_json::json!({ "watcher": "client" })
        );
        assert_eq!(
            driver.configuration_response(Some("rust-analyzer.cargo")),
            serde_json::json!(null)
        );
    }
}
