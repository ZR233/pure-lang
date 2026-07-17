use std::path::{Component, Path};

use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnedPathKind {
    Exact,
    Directory,
}

/// 经过校验的 executor 文件所有权范围。
///
/// `canonical` 保留持久化与展示所需的规范原文，`comparison` 只用于遵循平台文件系统
/// 大小写语义的边界比较。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedPath {
    canonical: String,
    comparison: String,
    kind: OwnedPathKind,
}

impl OwnedPath {
    pub(crate) fn parse(path: &str) -> Result<Self> {
        let normalized = path.trim().replace('\\', "/");
        let (path, kind) = normalized
            .strip_suffix("/**")
            .map_or((normalized.as_str(), OwnedPathKind::Exact), |path| {
                (path, OwnedPathKind::Directory)
            });
        if path.is_empty()
            || path.starts_with('/')
            || path.ends_with('/')
            || path.as_bytes().get(1) == Some(&b':')
            || path.contains('*')
            || path.split('/').any(str::is_empty)
            || Path::new(path)
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            bail!("invalid owned path `{normalized}`: use a relative normalized path");
        }
        let canonical = match kind {
            OwnedPathKind::Exact => path.to_string(),
            OwnedPathKind::Directory => format!("{path}/**"),
        };
        Ok(Self {
            comparison: comparison_key(path),
            canonical,
            kind,
        })
    }

    pub(crate) fn overlaps(&self, other: &Self) -> bool {
        match (self.kind, other.kind) {
            (OwnedPathKind::Exact, OwnedPathKind::Exact) => self.comparison == other.comparison,
            (OwnedPathKind::Exact, OwnedPathKind::Directory) => {
                is_descendant(&self.comparison, &other.comparison)
            }
            (OwnedPathKind::Directory, OwnedPathKind::Exact) => {
                is_descendant(&other.comparison, &self.comparison)
            }
            (OwnedPathKind::Directory, OwnedPathKind::Directory) => {
                self.comparison == other.comparison
                    || is_descendant(&self.comparison, &other.comparison)
                    || is_descendant(&other.comparison, &self.comparison)
            }
        }
    }

    pub(crate) fn matches(&self, changed_file: &str) -> bool {
        let changed_file = comparison_key(&changed_file.replace('\\', "/"));
        match self.kind {
            OwnedPathKind::Exact => changed_file == self.comparison,
            OwnedPathKind::Directory => is_descendant(&changed_file, &self.comparison),
        }
    }

    pub(crate) fn into_canonical(self) -> String {
        self.canonical
    }
}

fn comparison_key(path: &str) -> String {
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path.to_string()
    }
}

fn is_descendant(path: &str, directory: &str) -> bool {
    path.strip_prefix(directory)
        .is_some_and(|suffix| suffix.starts_with('/'))
}
