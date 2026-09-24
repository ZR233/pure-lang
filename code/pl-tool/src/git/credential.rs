use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use pl_protocol::PureError;
use secrecy::{ExposeSecret, SecretString};

use crate::tool_error;

pub const GIT_TOKEN_ENV: &str = "PL_GIT_TOKEN";

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
