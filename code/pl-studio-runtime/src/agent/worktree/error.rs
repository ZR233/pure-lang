use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeFailureCauseKind {
    InvalidRepoRoot,
    UnsafeBranch,
    GitLaunchFailed,
    GitTimedOut,
    GitExited,
    GitStatusUnknown,
    Io,
    Disabled,
    OperationAndCleanupFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeFailureCause {
    pub kind: WorktreeFailureCauseKind,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
}

/// Studio task agent worktree resource error.
#[derive(Debug, thiserror::Error)]
pub enum WorktreeError {
    #[error("invalid repo root: {0}")]
    InvalidRepoRoot(String),
    #[error("unsafe git branch `{0}`")]
    UnsafeBranch(String),
    #[error("failed to launch git {args}: {message}")]
    GitLaunchFailed { args: String, message: String },
    #[error("git {args} timed out")]
    GitTimedOut { args: String },
    #[error("git {args} exited with {exit_code}{}", git_stderr_suffix(.stderr))]
    GitExited {
        args: String,
        exit_code: i32,
        stderr: String,
    },
    #[error("git {args} status is unknown{}", git_stderr_suffix(.stderr))]
    GitStatusUnknown { args: String, stderr: String },
    #[error("worktree io error: {0}")]
    Io(String),
    #[error("worktree support is disabled")]
    Disabled,
    #[error("{operation}; rollback succeeded")]
    OperationFailedAfterCleanup { operation: Box<WorktreeError> },
    #[error("{operation}; rollback failed: {cleanup}")]
    OperationFailedWithCleanup {
        operation: Box<WorktreeError>,
        cleanup: Box<WorktreeError>,
    },
    #[error("{context} cleanup failed{}", cleanup_failures_suffix(.failures))]
    CleanupFailed {
        context: String,
        failures: Vec<WorktreeError>,
    },
}

impl WorktreeError {
    pub fn cause(&self) -> WorktreeFailureCause {
        let (kind, args, exit_code, stderr) = match self {
            Self::InvalidRepoRoot(_) => {
                (WorktreeFailureCauseKind::InvalidRepoRoot, None, None, None)
            }
            Self::UnsafeBranch(_) => (WorktreeFailureCauseKind::UnsafeBranch, None, None, None),
            Self::GitLaunchFailed { args, .. } => (
                WorktreeFailureCauseKind::GitLaunchFailed,
                Some(bounded(args, 1024)),
                None,
                None,
            ),
            Self::GitTimedOut { args } => (
                WorktreeFailureCauseKind::GitTimedOut,
                Some(bounded(args, 1024)),
                None,
                None,
            ),
            Self::GitExited {
                args,
                exit_code,
                stderr,
            } => (
                WorktreeFailureCauseKind::GitExited,
                Some(bounded(args, 1024)),
                Some(*exit_code),
                Some(bounded(stderr, 4096)),
            ),
            Self::GitStatusUnknown { args, stderr } => (
                WorktreeFailureCauseKind::GitStatusUnknown,
                Some(bounded(args, 1024)),
                None,
                Some(bounded(stderr, 4096)),
            ),
            Self::Io(_) => (WorktreeFailureCauseKind::Io, None, None, None),
            Self::Disabled => (WorktreeFailureCauseKind::Disabled, None, None, None),
            Self::OperationFailedAfterCleanup { operation } => return operation.cause(),
            Self::OperationFailedWithCleanup { operation, .. } => {
                let mut cause = operation.cause();
                cause.kind = WorktreeFailureCauseKind::OperationAndCleanupFailed;
                cause.message = bounded(&self.to_string(), 4096);
                return cause;
            }
            Self::CleanupFailed { failures, .. } => {
                let mut cause = failures.first().map_or(
                    WorktreeFailureCause {
                        kind: WorktreeFailureCauseKind::OperationAndCleanupFailed,
                        message: String::new(),
                        args: None,
                        exit_code: None,
                        stderr: None,
                    },
                    WorktreeError::cause,
                );
                cause.kind = WorktreeFailureCauseKind::OperationAndCleanupFailed;
                cause.message = bounded(&self.to_string(), 4096);
                return cause;
            }
        };
        WorktreeFailureCause {
            kind,
            message: bounded(&self.to_string(), 4096),
            args,
            exit_code,
            stderr,
        }
    }

    pub fn cleanup_failed(&self) -> bool {
        matches!(
            self,
            Self::OperationFailedWithCleanup { .. } | Self::CleanupFailed { .. }
        )
    }

    pub fn cleanup_succeeded(&self) -> bool {
        matches!(self, Self::OperationFailedAfterCleanup { .. })
    }
}

fn bounded(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn git_stderr_suffix(stderr: &str) -> String {
    let stderr = stderr.trim();
    if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    }
}

fn cleanup_failures_suffix(failures: &[WorktreeError]) -> String {
    failures
        .iter()
        .map(|failure| format!("; {failure}"))
        .collect()
}

impl From<WorktreeError> for pl_protocol::PureError {
    fn from(error: WorktreeError) -> Self {
        Self::ToolExecutionFailed {
            tool: "worktree".to_string(),
            error: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_causes_exhaustively_classify_operational_failures() {
        let cases = [
            (
                WorktreeError::InvalidRepoRoot("missing".to_string()),
                WorktreeFailureCauseKind::InvalidRepoRoot,
            ),
            (
                WorktreeError::UnsafeBranch("main".to_string()),
                WorktreeFailureCauseKind::UnsafeBranch,
            ),
            (
                WorktreeError::GitLaunchFailed {
                    args: "worktree add".to_string(),
                    message: "git not found".to_string(),
                },
                WorktreeFailureCauseKind::GitLaunchFailed,
            ),
            (
                WorktreeError::GitTimedOut {
                    args: "worktree add".to_string(),
                },
                WorktreeFailureCauseKind::GitTimedOut,
            ),
            (
                WorktreeError::GitExited {
                    args: "worktree add".to_string(),
                    exit_code: 128,
                    stderr: "fatal".to_string(),
                },
                WorktreeFailureCauseKind::GitExited,
            ),
            (
                WorktreeError::GitStatusUnknown {
                    args: "worktree add".to_string(),
                    stderr: "lost process status".to_string(),
                },
                WorktreeFailureCauseKind::GitStatusUnknown,
            ),
            (
                WorktreeError::Io("disk full".to_string()),
                WorktreeFailureCauseKind::Io,
            ),
            (WorktreeError::Disabled, WorktreeFailureCauseKind::Disabled),
            (
                WorktreeError::OperationFailedWithCleanup {
                    operation: Box::new(WorktreeError::GitTimedOut {
                        args: "worktree add".to_string(),
                    }),
                    cleanup: Box::new(WorktreeError::Io("cleanup".to_string())),
                },
                WorktreeFailureCauseKind::OperationAndCleanupFailed,
            ),
        ];

        for (error, expected) in cases {
            let cause = error.cause();
            assert_eq!(cause.kind, expected);
            assert!(!cause.message.is_empty());
            assert!(cause.message.len() <= 4096);
            assert_eq!(
                serde_json::from_value::<WorktreeFailureCause>(
                    serde_json::to_value(&cause).unwrap()
                )
                .unwrap(),
                cause
            );
        }
    }

    #[test]
    fn command_diagnostics_are_bounded() {
        let error = WorktreeError::GitExited {
            args: "a".repeat(2_000),
            exit_code: 128,
            stderr: "s".repeat(8_000),
        };
        let cause = error.cause();

        assert_eq!(cause.args.unwrap().len(), 1024);
        assert_eq!(cause.stderr.unwrap().len(), 4096);
    }
}
