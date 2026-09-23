//! git 工具:workspace 配置与 [`GitTool`] 适配器。
//!
//! 按域拆分:`schema` 承载工具类型与输入 schema,`policy` 承载安全策略,
//! `credential` 承载凭据注入,执行后端由宿主显式提供,
//! `commands` 承载各 git 子命令语义,`runner` 承载带凭据注入与脱敏的执行管道。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use pl_protocol::PureError;
use serde_json::Value;

use crate::deserialize_tool_input;

mod commands;
mod credential;
mod policy;
mod runner;
mod schema;

use crate::execution::ExecutionBackend;
pub use credential::*;

pub use policy::GitPolicy;
pub use schema::*;

/// git 工具运行配置。
#[derive(Debug, Clone, PartialEq)]
pub struct GitWorkspaceConfig {
    pub worktree: PathBuf,
    pub git_binary: PathBuf,
    pub policy: GitPolicy,
    /// 允许执行宿主直接使用其原生 Git 凭据链，而不注入本地 provider token。
    pub native_credentials: bool,
    pub default_push_branch: Option<String>,
    pub remote_url: Option<String>,
    pub workspace_info: BTreeMap<String, Value>,
}

impl GitWorkspaceConfig {
    pub fn local(worktree: impl Into<PathBuf>) -> Self {
        Self {
            worktree: worktree.into(),
            git_binary: PathBuf::from("git"),
            policy: GitPolicy::default(),
            native_credentials: false,
            default_push_branch: None,
            remote_url: None,
            workspace_info: BTreeMap::new(),
        }
    }

    pub fn with_native_credentials(mut self) -> Self {
        self.native_credentials = true;
        self
    }
}

/// 单个 git tool 适配器。
#[derive(Debug)]
pub struct GitTool<B, P> {
    kind: GitToolKind,
    config: GitWorkspaceConfig,
    backend: Arc<B>,
    credential_provider: Arc<P>,
}

impl<B, P> GitTool<B, P> {
    pub fn new(
        kind: GitToolKind,
        config: GitWorkspaceConfig,
        backend: Arc<B>,
        credential_provider: Arc<P>,
    ) -> Self {
        Self {
            kind,
            config,
            backend,
            credential_provider,
        }
    }

    fn name(&self) -> &str {
        self.kind.name()
    }
}

impl<B: ExecutionBackend + 'static, P: GitCredentialProvider + 'static> GitTool<B, P> {
    async fn invoke(&self, input: Value) -> Result<runner::GitToolOutcome, PureError> {
        match self.kind {
            GitToolKind::Status => {
                deserialize_tool_input::<GitEmptyInput>(self.name(), input)?;
                self.run_plain(vec!["status", "--short", "--branch"]).await
            }
            GitToolKind::Diff => self.run_diff(input).await,
            GitToolKind::Branch => self.run_branch(input).await,
            GitToolKind::Fetch => self.run_fetch(input).await,
            GitToolKind::Commit => self.run_commit(input).await,
            GitToolKind::Push => self.run_push(input).await,
            GitToolKind::WorkspaceInfo => {
                deserialize_tool_input::<GitEmptyInput>(self.name(), input)?;
                self.workspace_info()
            }
            GitToolKind::SyncDefaultBranch => self.run_sync_default_branch(input).await,
        }
    }

    /// Transfers a Git executor under the host's explicit repository/credential policy identity.
    ///
    /// # Errors
    /// Returns invalid tool registration errors.
    pub fn registration(
        self,
        declaration: pl_core::context::OpaquePayload,
        authorization: pl_core::tool::opaque::ToolAuthorization,
    ) -> Result<pl_core::tool::opaque::Registration, pl_core::tool::opaque::RegistryError> {
        Ok(
            pl_core::tool::opaque::Registration::new(self.kind.name().into(), declaration, self)?
                .with_authorization(authorization),
        )
    }
}

impl<B: ExecutionBackend + 'static, P: GitCredentialProvider + 'static> pl_core::tool::opaque::Tool
    for GitTool<B, P>
{
    async fn execute(
        &self,
        input: pl_core::context::OpaquePayload,
        context: pl_core::tool::opaque::CallContext,
    ) -> Result<pl_core::tool::ToolOutput, pl_core::tool::opaque::ToolError> {
        use pl_core::tool::opaque::ToolError;
        if input.format() != "application/json" || input.version() != 1 {
            return Err(ToolError::new(crate::tool_error(
                self.name(),
                "unsupported Git argument encoding",
            )));
        }
        if context.cancellation.is_cancelled() {
            return Err(ToolError::new(pl_core::thread::ThreadError::Cancelled));
        }
        let input = serde_json::from_str(input.content()).map_err(ToolError::new)?;
        match self.invoke(input).await {
            Ok(outcome) => dynamic_output(&outcome),
            Err(source) => {
                let observed = runner::observed_failure(&source)
                    .map(dynamic_output)
                    .transpose()?;
                let error = ToolError::new(source);
                Err(match observed {
                    Some(output) => error.with_output(output),
                    None => error,
                })
            }
        }
    }
}

fn dynamic_output(
    outcome: &runner::GitToolOutcome,
) -> Result<pl_core::tool::ToolOutput, pl_core::tool::opaque::ToolError> {
    use pl_core::{
        context::{ContextContent, OpaquePayload},
        tool::opaque::ToolError,
    };
    let preview = pl_output::bounded_text(&outcome.description, 12 * 1024, 0);
    let context = if preview.truncated {
        format!(
            "{}\n[Git preview omitted {} bytes; full result retained in history.]",
            preview.text, preview.bytes_omitted
        )
    } else {
        preview.text
    };
    Ok(pl_core::tool::ToolOutput::new(
        OpaquePayload::new("pl.tool.git", 1, outcome.description.clone())
            .map_err(ToolError::new)?,
        vec![ContextContent::Text {
            text: Arc::from(context),
        }],
    ))
}
