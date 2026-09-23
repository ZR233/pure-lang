use std::path::{Path, PathBuf};

use pl_protocol::PureError;

use crate::workspace::ToolPathPolicy;

#[derive(Debug, Clone)]
pub struct WorkspacePaths {
    policy: ToolPathPolicy,
}

impl WorkspacePaths {
    pub async fn new(root: PathBuf, allow_workspace_escape: bool) -> Result<Self, PureError> {
        Ok(Self {
            policy: ToolPathPolicy::new(root, allow_workspace_escape, "file")?,
        })
    }

    pub async fn resolve_existing(&self, path: &str) -> Result<PathBuf, PureError> {
        self.policy.resolve_existing(path)
    }

    pub async fn resolve_for_write(&self, path: &str) -> Result<PathBuf, PureError> {
        self.policy.resolve_for_write(path)
    }

    pub async fn resolve_existing_or_parent(&self, path: &str) -> Result<PathBuf, PureError> {
        self.policy.resolve_existing_or_parent(path)
    }

    pub(crate) fn with_host_access(&self) -> Self {
        Self {
            policy: self.policy.with_host_access(),
        }
    }

    pub(crate) fn allows_host_access(&self) -> bool {
        self.policy.allows_host_access()
    }

    pub fn root(&self) -> &Path {
        self.policy.root()
    }

    pub fn display_relative(&self, path: &Path) -> String {
        self.policy.display_relative(path)
    }
}

pub fn matches_pattern(path: &str, pattern: Option<&str>) -> bool {
    let Some(pattern) = pattern.filter(|pattern| !pattern.is_empty()) else {
        return true;
    };
    if matches_pattern_once(path, pattern) {
        return true;
    }
    if !pattern.contains("**/") {
        return false;
    }

    let mut variants = vec![pattern.to_string()];
    let mut index = 0;
    while index < variants.len() {
        if let Some(offset) = variants[index].find("**/") {
            let mut variant = variants[index].clone();
            variant.replace_range(offset..offset + 3, "");
            if !variants.contains(&variant) && matches_pattern_once(path, &variant) {
                return true;
            }
            if !variants.contains(&variant) {
                variants.push(variant);
            }
        }
        index += 1;
    }
    false
}

fn matches_pattern_once(path: &str, pattern: &str) -> bool {
    if !pattern.contains('*') {
        return path.contains(pattern);
    }

    let mut remainder = path;
    let mut first = true;
    for part in pattern.split('*').filter(|part| !part.is_empty()) {
        if first && !pattern.starts_with('*') {
            let Some(stripped) = remainder.strip_prefix(part) else {
                return false;
            };
            remainder = stripped;
        } else {
            let Some(index) = remainder.find(part) else {
                return false;
            };
            remainder = &remainder[index + part.len()..];
        }
        first = false;
    }

    pattern.ends_with('*') || remainder.is_empty()
}
