use std::path::{Component, Path};

use anyhow::{Result, bail};

/// 经过校验的 executor 任务拆分提示。
///
/// scope hint 是仓库相对路径前缀，只用于计划、审查聚焦与潜在冲突提示；它不授予或
/// 限制 workspace 内的文件访问。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopeHint(String);

impl ScopeHint {
    pub(crate) fn parse(path: &str) -> Result<Self> {
        let normalized = path.trim().replace('\\', "/");
        let normalized = normalized.trim_end_matches('/');
        if normalized.is_empty()
            || normalized.starts_with('/')
            || normalized.as_bytes().get(1) == Some(&b':')
            || normalized.contains('*')
            || normalized.split('/').any(str::is_empty)
            || Path::new(normalized)
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            bail!("invalid scope hint `{path}`: use a normalized repository-relative prefix");
        }
        Ok(Self(normalized.to_string()))
    }

    pub(crate) fn into_canonical(self) -> String {
        self.0
    }
}
