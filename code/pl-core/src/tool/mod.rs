mod bash;
mod truncation;

use std::fmt;
use std::future::Future;
use std::path::PathBuf;

use pl_model::ToolSchema;
use pl_protocol::PureError;
use serde::{Deserialize, Serialize};

pub use bash::{BashInput, BashTool};
pub use truncation::{OutputTruncation, TruncatedOutput, TruncationStrategy};

/// 工具执行抽象。
///
/// 每个具体工具实现此 trait。使用原生 RPITIT（不依赖 #[async_trait]），
/// 显式声明 Send bound。
///
/// 实现者应：
/// - 从 `ToolInput` 解析结构化输入
/// - 产出结构化 `ToolOutput`
/// - 通过 `PureError`（尤其是 `ToolExecutionFailed`）报告错误
pub trait Tool: fmt::Debug + Send + Sync {
    /// 工具名称（如 "bash"）。
    fn name(&self) -> &str;

    /// 工具功能描述，供 LLM 理解何时使用。
    fn description(&self) -> &str;

    /// 输入参数的 JSON Schema。
    fn input_schema(&self) -> serde_json::Value;

    /// 执行工具。
    fn execute(
        &self,
        input: ToolInput,
    ) -> impl Future<Output = Result<ToolOutput, PureError>> + Send;

    /// 生成 LLM API 所需的 ToolSchema。
    fn to_schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().to_string(),
            description: self.description().to_string(),
            input_schema: self.input_schema(),
        }
    }
}

/// 通用工具输入。
///
/// 携带 LLM tool call 的原始 arguments，以及用于定位输出文件的
/// session 和 tool call 标识。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInput {
    /// LLM 返回的 tool call arguments（JSON）。
    pub arguments: serde_json::Value,
    /// 当前会话标识，用于组织输出文件。
    pub session_id: String,
    /// Tool call 唯一 ID（来自 LLM ToolCall.id）。
    pub tool_id: String,
}

/// 通用工具输出。
///
/// 包含人类可读的描述、截断后的输出用于内联展示、
/// 完整输出文件路径和退出元数据。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolOutput {
    /// 简短描述（如 "Command exited with code 0"）。
    pub description: String,
    /// 截断后的 stdout/stderr。
    pub truncated: OutputTruncation,
    /// 完整输出文件路径。
    pub output_file: PathBuf,
    /// 退出码（如适用）。
    pub exit_code: Option<i32>,
    /// 是否超时。
    pub timed_out: bool,
}
