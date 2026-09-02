//! [`GitTool`] 各子命令的输入解析、policy 校验与命令编排。

use pl_protocol::PureError;
use serde_json::{Value, json};

use super::GitTool;
use super::credential::{GitCredentialOperation, GitCredentialProvider};
use super::execution::ExecutionBackend;
use super::runner::GitToolOutcome;
use super::schema::{
    GitBranchAction, GitBranchInput, GitCommitInput, GitDiffInput, GitFetchInput, GitPushInput,
    GitSyncDefaultBranchInput,
};
use crate::tool::{deserialize_tool_input, tool_error};

impl<B, P> GitTool<B, P>
where
    B: ExecutionBackend + 'static,
    P: GitCredentialProvider + 'static,
{
    pub(super) async fn run_diff(&self, arguments: Value) -> Result<GitToolOutcome, PureError> {
        let input: GitDiffInput = deserialize_tool_input(self.name(), arguments)?;
        let path = non_empty(input.path);
        if let Some(path) = path.as_deref() {
            self.config.policy.validate_path(path)?;
        }
        match (input.staged, path.as_deref()) {
            (true, Some(path)) => self.run_plain(vec!["diff", "--staged", "--", path]).await,
            (true, None) => self.run_plain(vec!["diff", "--staged"]).await,
            (false, Some(path)) => self.run_plain(vec!["diff", "--", path]).await,
            (false, None) => self.run_plain(vec!["diff"]).await,
        }
    }

    pub(super) async fn run_branch(&self, arguments: Value) -> Result<GitToolOutcome, PureError> {
        let input: GitBranchInput = deserialize_tool_input(self.name(), arguments)?;
        match input.action.unwrap_or(GitBranchAction::List) {
            GitBranchAction::List => self.run_plain(vec!["branch", "--list", "--all"]).await,
            GitBranchAction::Switch => {
                let name = required_text(self.name(), input.name, "name")?;
                self.config.policy.validate_branch(&name)?;
                self.run_plain(vec!["switch", &name]).await
            }
            GitBranchAction::Create => {
                let name = required_text(self.name(), input.name, "name")?;
                self.config.policy.validate_branch(&name)?;
                if let Some(start_point) = non_empty(input.start_point) {
                    self.config.policy.validate_branch(&start_point)?;
                    self.run_plain(vec!["switch", "-c", &name, &start_point])
                        .await
                } else {
                    self.run_plain(vec!["switch", "-c", &name]).await
                }
            }
        }
    }

    pub(super) async fn run_fetch(&self, arguments: Value) -> Result<GitToolOutcome, PureError> {
        let input: GitFetchInput = deserialize_tool_input(self.name(), arguments)?;
        let remote = non_empty(input.remote()).unwrap_or_else(|| "origin".to_string());
        self.config.policy.validate_remote(&remote)?;
        let refspec = non_empty(input.refspec);
        self.config
            .policy
            .validate_fetch_refspec(refspec.as_deref())?;
        let mut args = vec!["fetch"];
        if input.prune {
            args.push("--prune");
        }
        args.push(&remote);
        if let Some(refspec) = refspec.as_deref() {
            args.push(refspec);
        }
        self.run_with_credential(args, GitCredentialOperation::Fetch, remote.clone())
            .await
    }

    pub(super) async fn run_commit(&self, arguments: Value) -> Result<GitToolOutcome, PureError> {
        let input: GitCommitInput = deserialize_tool_input(self.name(), arguments)?;
        let message = required_text(self.name(), Some(input.message), "message")?;
        if input.all {
            self.run_plain(vec!["commit", "--no-verify", "-am", &message])
                .await
        } else {
            self.run_plain(vec!["commit", "--no-verify", "-m", &message])
                .await
        }
    }

    pub(super) async fn run_push(&self, arguments: Value) -> Result<GitToolOutcome, PureError> {
        let input: GitPushInput = deserialize_tool_input(self.name(), arguments)?;
        let remote = non_empty(input.remote()).unwrap_or_else(|| "origin".to_string());
        self.config.policy.validate_remote(&remote)?;
        let branch = non_empty(input.branch)
            .or_else(|| self.config.default_push_branch.clone())
            .ok_or_else(|| tool_error(self.name(), "missing string field `branch`"))?;
        self.config.policy.validate_branch(&branch)?;
        let destination = format!("HEAD:refs/heads/{branch}");
        let mut args = vec!["push", "--no-verify"];
        if input.set_upstream {
            args.push("-u");
        }
        args.push(&remote);
        args.push(&destination);
        self.run_with_credential(args, GitCredentialOperation::Push, remote.clone())
            .await
    }

    pub(super) fn workspace_info(&self) -> Result<GitToolOutcome, PureError> {
        let mut payload = serde_json::Map::new();
        payload.insert("worktree".to_string(), json!(self.config.worktree));
        payload.insert("clone".to_string(), json!(self.config.worktree));
        for (key, value) in &self.config.workspace_info {
            payload.insert(key.clone(), value.clone());
        }
        GitToolOutcome::json(self.name(), Value::Object(payload), Some(0))
    }

    pub(super) async fn run_sync_default_branch(
        &self,
        arguments: Value,
    ) -> Result<GitToolOutcome, PureError> {
        let input: GitSyncDefaultBranchInput = deserialize_tool_input(self.name(), arguments)?;
        if input.force && input.preserve_changes {
            return Err(tool_error(
                self.name(),
                "force and preserveChanges cannot both be true",
            ));
        }

        let status = self.run_plain(vec!["status", "--porcelain"]).await?;
        let dirty = !status.stdout.trim().is_empty();
        if dirty && !input.force && !input.preserve_changes {
            return Err(tool_error(
                self.name(),
                "git workspace has uncommitted changes; pass force=true to discard them or preserveChanges=true to stash them before sync",
            ));
        }
        if dirty && input.preserve_changes {
            self.run_plain(vec![
                "stash",
                "push",
                "-u",
                "-m",
                "pl-core sync default branch",
            ])
            .await?;
        }
        if let Some(remote_url) = self.config.remote_url.as_deref() {
            self.run_plain(vec!["remote", "set-url", "origin", remote_url])
                .await?;
        }
        self.run_with_credential(
            vec!["fetch", "--prune", "origin"],
            GitCredentialOperation::Fetch,
            "origin".to_string(),
        )
        .await?;
        let branch = self
            .config
            .default_push_branch
            .as_deref()
            .unwrap_or(&self.config.policy.default_branch);
        self.config.policy.validate_branch(branch)?;
        let origin_branch = format!("origin/{}", self.config.policy.default_branch);
        self.run_plain(vec!["checkout", "-B", branch, &origin_branch])
            .await?;
        self.run_plain(vec!["reset", "--hard", &origin_branch])
            .await?;
        if input.force {
            self.run_plain(vec!["clean", "-fdx"]).await?;
        }
        if dirty && input.preserve_changes {
            self.run_plain(vec!["stash", "pop"]).await?;
        }

        GitToolOutcome::json(
            self.name(),
            json!({
            "clone": self.config.worktree,
            "worktree": self.config.worktree,
            "preservedChanges": dirty && input.preserve_changes,
            "forced": input.force,
            }),
            Some(0),
        )
    }
}

fn required_text(tool: &str, value: Option<String>, field: &str) -> Result<String, PureError> {
    non_empty(value).ok_or_else(|| tool_error(tool, format!("missing string field `{field}`")))
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}
