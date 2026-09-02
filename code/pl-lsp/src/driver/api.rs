use std::path::Path;

use futures::future::BoxFuture;
use serde_json::Value;

use crate::catalog::LspServerDefinition;
use crate::host::LspHostBackend;
use crate::runtime::LspMissingComponent;

/// 解析占位符后的进程启动命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspResolvedCommand {
    pub program: String,
    pub args: Vec<String>,
}

/// 环境探测的 typed 结果：就绪或带修复说明的缺失原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LspProbeOutcome {
    Ready { version: String },
    MissingCommand { message: String },
    MissingComponent(LspMissingComponent),
    Failed { message: String },
}

/// 修复缺失组件失败的原因。
#[derive(Debug, thiserror::Error)]
pub enum LspRepairError {
    #[error("server does not provide a repair action")]
    NotSupported,
    #[error("{tool} was not found on PATH")]
    MissingTool { tool: String },
    #[error("repair failed: {0}")]
    Failed(String),
}

/// 单个 LSP server 的生命周期 adapter。
///
/// catalog 需要运行期开放扩展（内置、用户配置与宿主自定义 driver 共存），
/// 因此以 `dyn` 分发；future 用 `BoxFuture`，不使用 `#[async_trait]`。
pub trait LspServerDriver: Send + Sync {
    /// 解析进程启动参数；默认渲染 catalog command 的 `{workspaceRoot}` 占位符。
    fn resolve_command(
        &self,
        definition: &LspServerDefinition,
        workspace_root: &Path,
    ) -> LspResolvedCommand {
        definition.command.render(workspace_root)
    }

    /// 探测运行环境，返回 typed 就绪或缺失原因，但不启动 server 进程。
    fn probe<'a>(
        &'a self,
        command: &'a LspResolvedCommand,
        host: Option<&'a dyn LspHostBackend>,
    ) -> BoxFuture<'a, LspProbeOutcome>;

    /// 修复探测发现的缺失组件；只在 `missingServerComponent` 状态下调用。
    fn repair<'a>(
        &'a self,
        component: &'a LspMissingComponent,
        host: Option<&'a dyn LspHostBackend>,
    ) -> BoxFuture<'a, Result<(), LspRepairError>>;

    /// 返回 LSP initialize 请求的 server 专项 `initializationOptions`。
    fn initialization_options(&self) -> Value {
        Value::Null
    }

    /// 返回 `workspace/configuration` 请求中单个 section 的值。
    fn configuration_response(&self, _section: Option<&str>) -> Value {
        Value::Null
    }
}
