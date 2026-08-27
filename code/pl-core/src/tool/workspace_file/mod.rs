mod backend;
mod container;
mod container_path;
mod local;
mod ops;
mod patch;
mod schema;

use std::path::PathBuf;
use std::sync::Arc;

use futures::FutureExt;
use pl_model::ToolSpec;
use pl_protocol::Result;
use serde_json::Value;

use crate::tool::cache::ToolCachePolicy;
use crate::tool::{
    BoxFuture, Tool, ToolCallContext, ToolInput, ToolResult, ToolWorkspace, tool_error,
};
use crate::turn::ToolEffect;

pub use backend::*;
pub use container::ContainerWorkspaceFileBackend;
pub use local::LocalWorkspaceFileBackend;
pub use ops::{WorkspaceFileToolExecution, execute_workspace_file_tool};
pub use patch::apply_patch_to_backend;
pub use schema::*;

#[derive(Debug, Clone)]
pub struct WorkspaceFileTool<B> {
    kind: WorkspaceFileToolKind,
    backend: Arc<B>,
}

impl<B> WorkspaceFileTool<B> {
    pub fn new(kind: WorkspaceFileToolKind, backend: Arc<B>) -> Self {
        Self { kind, backend }
    }
}

impl<B> Tool for WorkspaceFileTool<B>
where
    B: WorkspaceFileBackend + 'static,
{
    fn name(&self) -> &str {
        self.kind.name()
    }

    fn description(&self) -> &str {
        self.kind.description()
    }

    fn input_schema(&self) -> Value {
        self.kind.input_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        self.kind.supports_parallel_tool_calls()
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(self.kind.effect())
    }

    fn cache_policy(&self, _arguments: &serde_json::Value) -> ToolCachePolicy {
        self.kind.cache_policy()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        _context: ToolCallContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let execution = execute_workspace_file_tool(
                self.backend.as_ref(),
                self.kind.name(),
                input.arguments,
            )
            .await?
            .ok_or_else(|| tool_error(self.name(), "unknown workspace file tool"))?;
            Ok(workspace_tool_output(execution))
        }
        .boxed()
    }

    fn spec(&self) -> ToolSpec {
        self.kind.to_spec()
    }
}

/// 使用当前 `ToolCallContext` 构造本地 workspace backend 的文件工具。
///
/// 本地文件工具需要每次执行时读取 turn 的 workspace 权限、LSP runtime 和写锁，
/// 因此不能像容器 backend 一样在注册时固定一个 backend 实例。该类型仍复用
/// `execute_workspace_file_tool`，确保本地和容器 workspace 共享同一套 schema、
/// 输入解析、patch engine 和 JSON 输出。
#[derive(Debug, Clone)]
pub struct LocalWorkspaceFileTool {
    kind: WorkspaceFileToolKind,
    workspace: ToolWorkspace,
}

impl LocalWorkspaceFileTool {
    pub fn new(kind: WorkspaceFileToolKind, workspace: ToolWorkspace) -> Self {
        Self { kind, workspace }
    }
}

impl Tool for LocalWorkspaceFileTool {
    fn name(&self) -> &str {
        self.kind.name()
    }

    fn description(&self) -> &str {
        self.kind.description()
    }

    fn input_schema(&self) -> Value {
        self.kind.input_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        self.kind.supports_parallel_tool_calls()
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(self.kind.effect())
    }

    fn cache_policy(&self, _arguments: &serde_json::Value) -> ToolCachePolicy {
        self.kind.cache_policy()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolCallContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            if matches!(self.kind, WorkspaceFileToolKind::ApplyPatch) {
                self.workspace.ensure_workspace_writable()?;
            }
            let _write_guard = if matches!(self.kind, WorkspaceFileToolKind::ApplyPatch) {
                Some(self.workspace.write_lock().await)
            } else {
                None
            };
            let backend = LocalWorkspaceFileBackend::for_call(&self.workspace, &context).await?;
            let execution =
                execute_workspace_file_tool(&backend, self.kind.name(), input.arguments)
                    .await?
                    .ok_or_else(|| tool_error(self.name(), "unknown workspace file tool"))?;
            Ok(workspace_tool_output(execution))
        }
        .boxed()
    }

    fn spec(&self) -> ToolSpec {
        self.kind.to_spec()
    }
}

fn workspace_tool_output(execution: WorkspaceFileToolExecution) -> ToolResult {
    let metrics = crate::tool::ToolDirective::OutputMetrics {
        raw_bytes: execution.output.len() as u64,
        model_visible_bytes: execution.model_output.len() as u64,
        artifact_bytes: 0,
        result_hash: crate::canonical_content_hash(execution.output.as_bytes()),
    };
    ToolResult {
        success: execution.exit_code.unwrap_or_default() == 0,
        content: crate::tool::ToolResultContent::Text(execution.output),
        model_output: execution.model_output,
        model_attachments: Vec::new(),
        truncated: execution.truncated,
        output_file: PathBuf::new(),
        exit_code: execution.exit_code,
        timed_out: false,
        runtime_events: vec![metrics],
    }
}
