mod backend;
mod container;
mod container_path;
mod local;
mod ops;
mod patch;
mod schema;

use std::path::PathBuf;
use std::sync::Arc;

use pl_model::ToolSchema;
use pl_protocol::Result;
use serde_json::Value;

use crate::tool::{BoxFuture, Tool, ToolContext, ToolInput, ToolOutput};

pub use backend::{
    WorkspaceFileBackend, WorkspaceFileListEntry, WorkspaceFileListRequest,
    WorkspaceFileListResult, WorkspaceFileReadRequest, WorkspaceFileRemoveRequest,
    WorkspaceFileSearchMatch, WorkspaceFileSearchRequest, WorkspaceFileSearchResult,
    WorkspaceFileStat, WorkspaceFileStatRequest, WorkspaceFileWriteRequest,
};
pub use container::ContainerWorkspaceFileBackend;
pub use local::LocalWorkspaceFileBackend;
pub use ops::{WorkspaceFileToolExecution, execute_workspace_file_tool};
pub use schema::WorkspaceFileToolKind;

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

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput>> {
        Box::pin(async move {
            let execution = execute_workspace_file_tool(
                self.backend.as_ref(),
                self.kind.name(),
                input.arguments,
                context.options.cancellation_token.clone(),
            )
            .await?
            .ok_or_else(|| ops::tool_error(self.name(), "unknown workspace file tool"))?;
            Ok(ToolOutput {
                description: execution.model_output,
                truncated: execution.truncated,
                output_file: PathBuf::new(),
                exit_code: execution.exit_code,
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
    }

    fn to_schema(&self) -> ToolSchema {
        self.kind.to_schema()
    }
}
