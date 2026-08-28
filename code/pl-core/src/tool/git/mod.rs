//! git 工具:workspace 配置与 [`GitTool`] 适配器。
//!
//! 按域拆分:`schema` 承载工具类型与输入 schema,`policy` 承载安全策略,
//! `credential` 承载凭据注入,`execution` 承载通用命令执行 backend,
//! `commands` 承载各 git 子命令语义,`runner` 承载带凭据注入与脱敏的执行管道。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use futures::FutureExt;
use pl_protocol::PureError;
use serde_json::Value;

use super::cache::ToolCachePolicy;
use super::{
    BoxFuture, OutputTruncation, Tool, ToolCallContext, ToolInput, ToolResult,
    deserialize_tool_input,
};
use crate::turn::ToolEffect;

mod commands;
mod credential;
mod execution;
mod policy;
mod runner;
mod schema;

pub use credential::*;
pub use execution::*;
pub use policy::GitPolicy;
pub use schema::*;

#[cfg(test)]
mod unit_tests;

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
}

impl<B, P> Tool for GitTool<B, P>
where
    B: ExecutionBackend,
    P: GitCredentialProvider,
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

    fn effect(&self) -> Option<ToolEffect> {
        Some(self.kind.effect())
    }

    fn supports_programmatic_calls(&self) -> bool {
        self.kind.effect() == ToolEffect::Read
    }

    fn cache_policy(&self, _arguments: &serde_json::Value) -> ToolCachePolicy {
        self.kind.cache_policy()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        _context: ToolCallContext,
    ) -> BoxFuture<'a, Result<ToolResult, PureError>> {
        async move {
            let outcome = match self.kind {
                GitToolKind::Status => {
                    deserialize_tool_input::<GitEmptyInput>(self.name(), input.arguments)?;
                    self.run_plain(vec!["status", "--short", "--branch"]).await
                }
                GitToolKind::Diff => self.run_diff(input.arguments).await,
                GitToolKind::Branch => self.run_branch(input.arguments).await,
                GitToolKind::Fetch => self.run_fetch(input.arguments).await,
                GitToolKind::Commit => self.run_commit(input.arguments).await,
                GitToolKind::Push => self.run_push(input.arguments).await,
                GitToolKind::WorkspaceInfo => {
                    deserialize_tool_input::<GitEmptyInput>(self.name(), input.arguments)?;
                    self.workspace_info()
                }
                GitToolKind::SyncDefaultBranch => {
                    self.run_sync_default_branch(input.arguments).await
                }
            }?;
            Ok(ToolResult::from_runtime_text(
                outcome.description,
                OutputTruncation::empty(),
                PathBuf::new(),
                outcome.exit_code,
                false,
                Vec::new(),
            ))
        }
        .boxed()
    }
}
