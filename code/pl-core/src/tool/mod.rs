mod bash;
mod file;
mod multi_agent;
mod recoverable;
mod skill;
mod subagent;
mod truncation;

use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use pl_model::ToolSchema;
use pl_protocol::{AgentEventSender, PureError};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::AgentControl;
use crate::turn::TurnOptions;

pub use bash::{BashInput, BashTool};
pub use file::{
    ApplyPatchTool, CopyPathTool, CreateDirectoryTool, DeletePathTool, ListFilesTool, MovePathTool,
    ReadFileTool, SearchFilesTool, StatPathTool, WriteFileTool,
};
pub use multi_agent::{
    CloseAgentTool, FollowupTaskTool, ListAgentsTool, SendMessageTool, SpawnAgentTool,
    WaitAgentTool,
};
pub(crate) use recoverable::RECOVERABLE_SUBAGENT_429_MARKER;
pub use skill::{SkillManageTool, SkillViewTool, SkillsListTool};
pub use subagent::{SubagentInput, SubagentTool};
pub use truncation::{OutputTruncation, TruncatedOutput, TruncationStrategy};

/// 便捷类型别名：boxed future。
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 工具执行抽象（dyn-compatible）。
///
/// `execute` 返回 `BoxFuture` 以支持 trait object。
/// `ToolContext` 提供事件转发、审批策略和当前 subagent 运行边界。
/// 具体实现中可用 `Box::pin(async move { ... })` 包裹异步逻辑。
pub trait Tool: fmt::Debug + Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    fn supports_parallel_tool_calls(&self) -> bool {
        false
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>>;

    fn to_schema(&self) -> ToolSchema {
        ToolSchema::function(self.name(), self.description(), self.input_schema())
    }
}

/// 单次工具执行上下文。
///
/// 由核心 turn 循环注入，工具通过它访问事件流、审批策略、
/// workspace 信息，以及当前 subagent 的父子关系。
#[derive(Clone)]
pub struct ToolContext {
    pub event_tx: AgentEventSender,
    pub options: TurnOptions,
    pub mode: crate::turn::CompileMode,
    pub workspace_root: PathBuf,
    pub workspace_instructions: Option<String>,
    pub active_subagent: Option<SubagentContext>,
    pub agent_control: AgentControl,
}

/// 当前工具调用所在的 subagent 运行边界。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentContext {
    pub id: String,
    pub parent_id: Option<String>,
    pub agent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub depth: u32,
}

impl fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolContext")
            .field("workspace_root", &self.workspace_root)
            .field("permission_mode", &self.options.permission_mode)
            .field("active_subagent", &self.active_subagent)
            .finish_non_exhaustive()
    }
}

impl ToolContext {
    pub(crate) fn allows_workspace_escape(&self) -> bool {
        self.options.permission_mode.allows_workspace_escape()
    }

    pub(crate) async fn workspace_write_lock(&self) -> WorkspaceWriteGuard {
        workspace_write_locks().lock_for(&self.workspace_root).await
    }
}

type WorkspaceWriteGuard = OwnedMutexGuard<()>;

#[derive(Default)]
struct WorkspaceWriteLocks {
    locks: std::sync::Mutex<std::collections::HashMap<PathBuf, Arc<Mutex<()>>>>,
}

impl WorkspaceWriteLocks {
    async fn lock_for(&self, workspace_root: &std::path::Path) -> WorkspaceWriteGuard {
        let key =
            std::fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf());
        let lock = {
            let mut locks = self.locks.lock().expect("workspace write locks poisoned");
            locks
                .entry(key)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        lock.lock_owned().await
    }
}

fn workspace_write_locks() -> &'static WorkspaceWriteLocks {
    static LOCKS: OnceLock<WorkspaceWriteLocks> = OnceLock::new();
    LOCKS.get_or_init(WorkspaceWriteLocks::default)
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

    pub fn names(&self) -> Vec<&str> {
        self.tools.iter().map(|t| t.name()).collect()
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
            _context: ToolContext,
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
        assert_eq!(schemas[0].name(), "echo");
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

    #[tokio::test]
    async fn workspace_write_lock_is_shared_for_same_workspace() {
        let root = std::env::temp_dir().join(format!(
            "pure-lang-write-lock-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let context = ToolContext {
            event_tx,
            options: TurnOptions::default(),
            mode: crate::turn::CompileMode::Auto,
            workspace_root: root.clone(),
            workspace_instructions: None,
            active_subagent: None,
            agent_control: AgentControl::default(),
        };
        let first_guard = context.workspace_write_lock().await;
        let second_context = context.clone();
        let second = tokio::spawn(async move { second_context.workspace_write_lock().await });
        tokio::task::yield_now().await;

        assert!(!second.is_finished());
        drop(first_guard);
        let second_guard = second.await.unwrap();
        drop(second_guard);
        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
