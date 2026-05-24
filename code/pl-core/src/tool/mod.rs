mod bash;
mod subagent;
mod truncation;

use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use pl_model::ToolSchema;
use pl_protocol::PureError;
use serde::{Deserialize, Serialize};

pub use bash::{BashInput, BashTool};
pub use subagent::{SubagentInput, SubagentTool};
pub use truncation::{OutputTruncation, TruncatedOutput, TruncationStrategy};

/// 便捷类型别名：boxed future。
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 工具执行抽象（dyn-compatible）。
///
/// `execute` 返回 `BoxFuture` 以支持 trait object。
/// 具体实现中可用 `Box::pin(async move { ... })` 包裹异步逻辑。
pub trait Tool: fmt::Debug + Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    fn execute<'a>(&'a self, input: ToolInput) -> BoxFuture<'a, Result<ToolOutput, PureError>>;

    fn to_schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().to_string(),
            description: self.description().to_string(),
            input_schema: self.input_schema(),
        }
    }
}

/// 工具注册表。
///
/// 管理已注册的工具实例，提供按名称查找和 schema 收集能力。
#[derive(Default)]
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.tools.iter().map(|t| t.name()).collect();
        f.debug_struct("ToolRegistry")
            .field("tools", &names)
            .finish()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: impl Tool + 'static) {
        assert!(
            self.get(tool.name()).is_none(),
            "duplicate tool name: {}",
            tool.name()
        );
        self.tools.push(Box::new(tool));
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.iter().find(|t| t.name() == name).map(|t| &**t)
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools.iter().map(|t| t.to_schema()).collect()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

/// 通用工具输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInput {
    pub arguments: serde_json::Value,
    pub session_id: String,
    pub tool_id: String,
}

/// 通用工具输出。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolOutput {
    pub description: String,
    pub truncated: OutputTruncation,
    pub output_file: PathBuf,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn empty_truncation() -> OutputTruncation {
        OutputTruncation {
            stdout: TruncatedOutput {
                content: String::new(),
                was_truncated: false,
                original_length: 0,
            },
            stderr: TruncatedOutput {
                content: String::new(),
                was_truncated: false,
                original_length: 0,
            },
        }
    }

    #[derive(Debug)]
    struct EchoTool;

    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "Echo input"
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": { "text": { "type": "string" } }
            })
        }

        fn execute<'a>(
            &'a self,
            _input: ToolInput,
        ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
            Box::pin(async {
                Ok(ToolOutput {
                    description: "ok".to_string(),
                    truncated: empty_truncation(),
                    output_file: PathBuf::new(),
                    exit_code: None,
                    timed_out: false,
                })
            })
        }
    }

    #[test]
    fn registry_register_and_get() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);

        assert_eq!(reg.len(), 1);
        assert!(!reg.is_empty());
        assert!(reg.get("echo").is_some());
        assert!(reg.get("missing").is_none());
    }

    #[test]
    fn registry_schemas() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);

        let schemas = reg.schemas();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].name, "echo");
    }

    #[test]
    fn registry_is_empty_initially() {
        let reg = ToolRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn registry_debug_shows_names() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);

        let debug = format!("{reg:?}");
        assert!(debug.contains("echo"));
    }
}
