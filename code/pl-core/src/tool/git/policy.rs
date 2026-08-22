use std::path::{Component, Path};

use pl_protocol::PureError;

use crate::tool::tool_error;

/// git workspace 安全策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitPolicy {
    pub(super) allowed_remote: String,
    pub(super) default_branch: String,
}

impl Default for GitPolicy {
    fn default() -> Self {
        Self {
            allowed_remote: "origin".to_string(),
            default_branch: "main".to_string(),
        }
    }
}

impl GitPolicy {
    pub fn new(default_branch: impl Into<String>) -> Self {
        Self {
            default_branch: default_branch.into(),
            ..Self::default()
        }
    }

    pub fn validate_remote(&self, remote: &str) -> Result<(), PureError> {
        if remote == self.allowed_remote {
            Ok(())
        } else {
            Err(tool_error(
                "git",
                format!(
                    "unsupported git remote `{remote}`; only `{}` is allowed",
                    self.allowed_remote
                ),
            ))
        }
    }

    pub fn validate_branch(&self, branch: &str) -> Result<(), PureError> {
        if branch.trim().is_empty()
            || branch.starts_with('/')
            || branch.ends_with('/')
            || branch.starts_with('.')
            || branch.contains("..")
            || branch.contains("//")
            || branch.contains("@{")
            || branch.contains('\\')
            || branch.ends_with(".lock")
            || branch.chars().any(char::is_control)
        {
            return Err(tool_error("git", format!("unsafe git branch `{branch}`")));
        }
        Ok(())
    }

    pub fn validate_path(&self, path: &str) -> Result<(), PureError> {
        let normalized = path.trim();
        if normalized.is_empty()
            || normalized != path
            || normalized.starts_with('/')
            || normalized.contains('\\')
            || has_windows_drive_prefix(normalized)
            || normalized.chars().any(char::is_control)
            || Path::new(normalized).is_absolute()
            || Path::new(normalized)
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(tool_error("git", format!("unsafe git path `{path}`")));
        }
        Ok(())
    }

    pub fn validate_fetch_refspec(&self, refspec: Option<&str>) -> Result<(), PureError> {
        let Some(refspec) = refspec else {
            return Ok(());
        };
        if refspec == self.default_branch
            || refspec == format!("refs/heads/{}", self.default_branch)
            || is_pull_request_head_ref(refspec)
        {
            Ok(())
        } else {
            Err(tool_error(
                "git",
                format!("unsupported git fetch refspec `{refspec}`"),
            ))
        }
    }
}

fn is_pull_request_head_ref(refspec: &str) -> bool {
    let (source, destination) = match refspec.split_once(':') {
        Some((source, destination)) => (source, Some(destination)),
        None => (refspec, None),
    };
    let source = source.strip_prefix("refs/").unwrap_or(source);
    let Some(number) = pull_request_head_number(source) else {
        return false;
    };
    match destination {
        Some(destination) => is_pull_request_head_destination(destination, number),
        None => true,
    }
}

fn pull_request_head_number(refspec: &str) -> Option<&str> {
    let refspec = refspec.strip_prefix("refs/").unwrap_or(refspec);
    let rest = refspec.strip_prefix("pull/")?;
    let number = rest.strip_suffix("/head")?;
    (!number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit())).then_some(number)
}

fn is_pull_request_head_destination(destination: &str, number: &str) -> bool {
    destination == format!("pr/{number}")
        || destination == format!("refs/pull/{number}/head")
        || destination == format!("refs/remotes/origin/pr/{number}")
}

fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_policy_rejects_non_origin_remote() {
        let policy = GitPolicy::default();

        assert!(policy.validate_remote("origin").is_ok());
        assert!(policy.validate_remote("upstream").is_err());
        assert!(
            policy
                .validate_remote("https://example.com/repo.git")
                .is_err()
        );
    }

    #[test]
    fn git_policy_rejects_unsafe_paths() {
        let policy = GitPolicy::default();

        assert!(policy.validate_path("src/lib.rs").is_ok());
        assert!(policy.validate_path("../secret").is_err());
        assert!(policy.validate_path("/etc/passwd").is_err());
        assert!(policy.validate_path("C:/Windows").is_err());
        assert!(policy.validate_path("bad\\path").is_err());
        assert!(policy.validate_path("bad\u{7f}path").is_err());
    }

    #[test]
    fn git_policy_rejects_unsafe_branch_names() {
        let policy = GitPolicy::default();

        assert!(policy.validate_branch("feature/safe-name").is_ok());
        assert!(policy.validate_branch("").is_err());
        assert!(policy.validate_branch("../escape").is_err());
        assert!(policy.validate_branch("/absolute").is_err());
        assert!(policy.validate_branch("bad\\branch").is_err());
        assert!(policy.validate_branch("bad\nbranch").is_err());
    }

    #[test]
    fn git_policy_allows_default_and_pr_fetch_refspecs_only() {
        let policy = GitPolicy::default();

        assert!(policy.validate_fetch_refspec(None).is_ok());
        assert!(policy.validate_fetch_refspec(Some("main")).is_ok());
        assert!(
            policy
                .validate_fetch_refspec(Some("refs/heads/main"))
                .is_ok()
        );
        assert!(policy.validate_fetch_refspec(Some("pull/42/head")).is_ok());
        assert!(
            policy
                .validate_fetch_refspec(Some("pull/42/head:pr/42"))
                .is_ok()
        );
        assert!(
            policy
                .validate_fetch_refspec(Some("refs/pull/42/head:refs/remotes/origin/pr/42"))
                .is_ok()
        );
        assert!(
            policy
                .validate_fetch_refspec(Some("pull/42/head:refs/pull/43/head"))
                .is_err()
        );
        assert!(
            policy
                .validate_fetch_refspec(Some("+refs/heads/main:refs/heads/main"))
                .is_err()
        );
        assert!(
            policy
                .validate_fetch_refspec(Some("refs/tags/v1.0.0"))
                .is_err()
        );
    }
}
