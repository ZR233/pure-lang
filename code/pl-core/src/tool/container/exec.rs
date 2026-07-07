use std::path::PathBuf;
use std::sync::Arc;

use pl_model::ToolSchema;
use pl_protocol::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::tool::{
    BoxFuture, OutputTruncation, Tool, ToolContext, ToolInput, ToolOutput, TruncatedOutput,
};

use super::backend::{ContainerBackend, ContainerExecRequest};
use super::files::{copy_download, copy_upload};
use super::helpers::{bounded_model_tool_output_with_tokens, parse_input, tool_error};
use super::schema::{ContainerToolKind, TOOL_CONTAINER_EXEC};

const DEFAULT_MODEL_TOOL_OUTPUT_TOKENS: usize = 10_000;
const MAX_MODEL_TOOL_OUTPUT_TOKENS: usize = 100_000;
const DEFAULT_EXEC_OUTPUT_BYTES_CAP: usize = 1024 * 1024;
const MAX_EXEC_OUTPUT_BYTES_CAP: usize = 16 * 1024 * 1024;

/// 容器工具执行结果。
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerToolExecution {
    pub success: bool,
    pub output: String,
    pub model_output: String,
    pub output_artifacts: Vec<Value>,
    pub exit_code: Option<i32>,
    pub truncated: OutputTruncation,
}

/// 基于共享 backend 执行一个容器/file 工具。
pub async fn execute_container_tool<B>(
    backend: &B,
    name: &str,
    arguments: Value,
    cancellation_token: Option<CancellationToken>,
) -> Result<Option<ContainerToolExecution>>
where
    B: ContainerBackend,
{
    let Some(kind) = ContainerToolKind::from_name(name) else {
        return Ok(None);
    };
    let execution = match kind {
        ContainerToolKind::Exec => execute_shell(backend, arguments, cancellation_token).await?,
        ContainerToolKind::CopyUpload => {
            let output = copy_upload(backend, arguments).await?;
            ContainerToolExecution::json(true, output, Vec::new(), DEFAULT_MODEL_TOOL_OUTPUT_TOKENS)
        }
        ContainerToolKind::CopyDownload => {
            let output = copy_download(backend, arguments).await?;
            ContainerToolExecution::json(true, output, Vec::new(), DEFAULT_MODEL_TOOL_OUTPUT_TOKENS)
        }
    };
    Ok(Some(execution))
}

impl ContainerToolExecution {
    fn json(
        success: bool,
        output: Value,
        output_artifacts: Vec<Value>,
        max_output_tokens: usize,
    ) -> Self {
        let output = serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string());
        let model_output = bounded_model_tool_output_with_tokens(&output, max_output_tokens);
        Self {
            success,
            output,
            model_output,
            output_artifacts,
            exit_code: success.then_some(0),
            truncated: OutputTruncation::empty(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContainerTool<B> {
    kind: ContainerToolKind,
    backend: Arc<B>,
}

impl<B> ContainerTool<B> {
    pub fn new(kind: ContainerToolKind, backend: Arc<B>) -> Self {
        Self { kind, backend }
    }
}

impl<B> Tool for ContainerTool<B>
where
    B: ContainerBackend + 'static,
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

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput>> {
        Box::pin(async move {
            let execution = execute_container_tool(
                self.backend.as_ref(),
                self.kind.name(),
                input.arguments,
                context.options.cancellation_token.clone(),
            )
            .await?
            .ok_or_else(|| tool_error(self.name(), "unknown container tool"))?;
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

#[derive(Debug, Deserialize)]
struct ContainerExecInput {
    command: String,
    cwd: Option<String>,
    timeout_secs: Option<u64>,
    max_output_tokens: Option<usize>,
    output_bytes_cap: Option<usize>,
}

async fn execute_shell<B>(
    backend: &B,
    arguments: Value,
    cancellation_token: Option<CancellationToken>,
) -> Result<ContainerToolExecution>
where
    B: ContainerBackend,
{
    let input: ContainerExecInput = parse_input(arguments, TOOL_CONTAINER_EXEC)?;
    let max_output_tokens = input
        .max_output_tokens
        .unwrap_or(DEFAULT_MODEL_TOOL_OUTPUT_TOKENS)
        .clamp(1, MAX_MODEL_TOOL_OUTPUT_TOKENS);
    let output_bytes_cap = input
        .output_bytes_cap
        .unwrap_or(DEFAULT_EXEC_OUTPUT_BYTES_CAP)
        .clamp(1, MAX_EXEC_OUTPUT_BYTES_CAP);
    let output = backend
        .exec(ContainerExecRequest {
            command: input.command,
            cwd: input.cwd,
            timeout_secs: input.timeout_secs,
            output_bytes_cap: Some(output_bytes_cap),
            cancellation_token,
        })
        .await?;
    let mut execution = ContainerToolExecution::json(
        output.status == 0,
        json!({
            "status": output.status,
            "stdout": output.stdout,
            "stderr": output.stderr,
            "stdout_truncated": output.stdout_truncated,
            "stderr_truncated": output.stderr_truncated,
            "stdout_bytes": output.stdout_bytes,
            "stderr_bytes": output.stderr_bytes,
            "output_artifacts": output.output_artifacts,
        }),
        output.output_artifacts,
        max_output_tokens,
    );
    execution.exit_code = Some(output.status);
    execution.truncated = OutputTruncation {
        stdout: TruncatedOutput {
            content: output.stdout,
            was_truncated: output.stdout_truncated,
            original_length: usize::try_from(output.stdout_bytes).unwrap_or(usize::MAX),
        },
        stderr: TruncatedOutput {
            content: output.stderr,
            was_truncated: output.stderr_truncated,
            original_length: usize::try_from(output.stderr_bytes).unwrap_or(usize::MAX),
        },
    };
    Ok(execution)
}
