use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use pl_protocol::PureError;
use secrecy::{ExposeSecret, SecretString};

use crate::tool::shell::shell_quote_word;
use crate::tool::tool_error;

pub const GIT_TOKEN_ENV: &str = "PL_GIT_TOKEN";

/// git shell 命令的凭据注入模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitShellCredential {
    Disabled,
    EnvToken,
}

/// 生成可在 shell backend 中执行的 git 命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitShellCommandRequest<'a> {
    pub safe_directory: &'a str,
    pub args: &'a [&'a str],
    pub credential: GitShellCredential,
}

pub fn git_shell_command(request: GitShellCommandRequest<'_>) -> String {
    let mut command_parts = vec![
        "git".to_string(),
        "-c".to_string(),
        shell_quote_word("core.hooksPath=/dev/null"),
        "-c".to_string(),
        shell_quote_word(&format!("safe.directory={}", request.safe_directory)),
        "-c".to_string(),
        shell_quote_word("credential.helper="),
    ];
    command_parts.extend(request.args.iter().map(|arg| shell_quote_word(arg)));
    let git_command = command_parts.join(" ");
    match request.credential {
        GitShellCredential::Disabled => git_command,
        GitShellCredential::EnvToken => git_shell_command_with_askpass(&git_command),
    }
}

/// 生成 shell 脚本片段，为后续 git 命令安装统一 askpass 凭据环境。
pub fn git_shell_credential_prelude() -> String {
    format!(
        "askpass=/tmp/pl-git-askpass-$$.sh\n\
         trap 'rm -f \"$askpass\"' EXIT\n\
         cat > \"$askpass\" <<'PL_GIT_ASKPASS'\n\
         {}PL_GIT_ASKPASS\n\
         chmod 700 \"$askpass\"\n\
         export GIT_ASKPASS=\"$askpass\"\n\
         export GIT_TERMINAL_PROMPT=0\n",
        git_askpass_script()
    )
}

/// 生成 sidecar shell 脚本中可复用的 `git_with_retry` 函数。
pub fn git_shell_retry_function() -> &'static str {
    "git_with_retry() {\n\
       attempts=0\n\
       while :; do\n\
         attempts=$((attempts + 1))\n\
         git -c credential.helper= -c http.version=HTTP/1.1 \"$@\" && return 0\n\
         status=$?\n\
         if [ \"$attempts\" -ge 3 ]; then\n\
           return \"$status\"\n\
         fi\n\
         sleep $((attempts * 2))\n\
       done\n\
     }\n"
}

/// 需要 git 凭据的操作类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitCredentialOperation {
    Fetch,
    Push,
}

/// git 凭据请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCredentialRequest {
    pub operation: GitCredentialOperation,
    pub remote: String,
}

/// git 短期凭据。
#[derive(Clone)]
pub struct GitCredential(SecretString);

impl fmt::Debug for GitCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("GitCredential").field(&"[redacted]").finish()
    }
}

impl GitCredential {
    pub fn new(value: String) -> Self {
        Self(SecretString::from(value))
    }

    pub(super) fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

/// 为需要认证的 git 操作按需提供凭据。
pub trait GitCredentialProvider: fmt::Debug + Send + Sync {
    type Error: fmt::Display + Send + 'static;

    fn credential(
        &self,
        request: GitCredentialRequest,
    ) -> impl Future<Output = std::result::Result<Option<GitCredential>, Self::Error>> + Send;
}

/// 不提供任何 git 凭据的 provider。
#[derive(Debug, Clone, Default)]
pub struct NoGitCredentialProvider;

impl GitCredentialProvider for NoGitCredentialProvider {
    type Error = String;

    async fn credential(
        &self,
        _request: GitCredentialRequest,
    ) -> std::result::Result<Option<GitCredential>, Self::Error> {
        Ok(None)
    }
}

pub(super) async fn write_askpass_script(tool: &str) -> Result<PathBuf, PureError> {
    let path = std::env::temp_dir().join(format!(
        "pl-core-git-askpass-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    tokio::fs::write(&path, git_askpass_script())
        .await
        .map_err(|error| tool_error(tool, format!("failed to write git askpass: {error}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .await
            .map_err(|error| tool_error(tool, format!("failed to chmod git askpass: {error}")))?;
    }
    Ok(path)
}

/// 返回统一的 git askpass 脚本文本。
pub fn git_askpass_script() -> &'static str {
    "#!/bin/sh\ncase \"$1\" in\n  *Username*) printf '%s\\n' x-access-token ;;\n  *Password*) printf '%s\\n' \"$PL_GIT_TOKEN\" ;;\n  *) printf '\\n' ;;\nesac\n"
}

fn git_shell_command_with_askpass(git_command: &str) -> String {
    format!(
        "askpass=$(mktemp) && cat > \"$askpass\" <<'PL_GIT_ASKPASS'\n{}PL_GIT_ASKPASS\nchmod 700 \"$askpass\" && GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=\"$askpass\" {git_command}; status=$?; rm -f \"$askpass\"; exit $status",
        git_askpass_script()
    )
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn git_shell_command_without_credential_uses_safe_git_flags() {
        let command = git_shell_command(GitShellCommandRequest {
            safe_directory: "/workspace/repo",
            args: &["fetch", "origin", "feature branch"],
            credential: GitShellCredential::Disabled,
        });

        assert_eq!(
            command,
            "git -c core.hooksPath=/dev/null -c safe.directory=/workspace/repo -c credential.helper= fetch origin 'feature branch'"
        );
    }

    #[test]
    fn git_shell_command_with_credential_installs_askpass() {
        let command = git_shell_command(GitShellCommandRequest {
            safe_directory: "/workspace/repo",
            args: &["push", "origin", "HEAD:mai-agent/test"],
            credential: GitShellCredential::EnvToken,
        });

        assert!(command.contains("GIT_ASKPASS=\"$askpass\""));
        assert!(command.contains("GIT_TERMINAL_PROMPT=0"));
        assert!(command.contains("$PL_GIT_TOKEN"));
        assert!(command.contains("x-access-token"));
        assert!(command.contains("git -c core.hooksPath=/dev/null"));
        assert!(command.contains("safe.directory=/workspace/repo"));
        assert!(command.contains("push origin HEAD:mai-agent/test"));
    }

    #[test]
    fn git_shell_credential_prelude_installs_pl_token_askpass() {
        let prelude = git_shell_credential_prelude();

        assert!(prelude.contains("GIT_ASKPASS"));
        assert!(prelude.contains("GIT_TERMINAL_PROMPT"));
        assert!(prelude.contains("$PL_GIT_TOKEN"));
        assert!(prelude.contains("x-access-token"));
        assert!(!prelude.contains("MAI_GITHUB_INSTALLATION_TOKEN"));
    }

    #[test]
    fn git_shell_retry_function_defines_generic_retry_wrapper() {
        let function = git_shell_retry_function();

        assert!(function.contains("git_with_retry()"));
        assert!(function.contains("credential.helper="));
        assert!(function.contains("http.version=HTTP/1.1"));
        assert!(function.contains("attempts"));
    }
}
