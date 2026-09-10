//! Language-service input contracts and workspace-bound Thread tools.
mod thread;
use pl_lsp::query::LspQueryOperation;
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::PathBuf;
pub use thread::{LspPathBinding, LspToolKind, ThreadLspTool};
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LspCapabilitiesInput {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspQueryInput {
    /// 目标语言 ID；运行期按 catalog 路由到对应 server。
    language_id: String,
    operation: LspQueryOperation,
    /// Workspace-relative or absolute path to the source file.
    file_path: Option<PathBuf>,
    /// 1-based line number for position operations.
    #[schemars(range(min = 1))]
    line: Option<u32>,
    /// 1-based UTF-16 character offset for position operations.
    #[schemars(range(min = 1))]
    character: Option<u32>,
    /// Workspace symbol query string.
    query: Option<String>,
    /// Maximum results to return.
    #[schemars(range(min = 1))]
    max_results: Option<usize>,
}
