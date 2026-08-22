//! patch 应用结果的汇总与面向用户的摘要。

use std::path::PathBuf;

use crate::backend::PatchPathDisplay;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchOutcome {
    applied: Vec<AppliedChange>,
    exact: bool,
}

impl Default for PatchOutcome {
    fn default() -> Self {
        Self {
            applied: Vec::new(),
            exact: true,
        }
    }
}

impl PatchOutcome {
    pub fn file_changes(&self) -> Vec<PatchFileChange> {
        self.applied
            .iter()
            .map(|change| match change {
                AppliedChange::Add { path, .. } => PatchFileChange::Add { path: path.clone() },
                AppliedChange::Update { path, .. } => {
                    PatchFileChange::Update { path: path.clone() }
                }
                AppliedChange::Delete { path, .. } => {
                    PatchFileChange::Delete { path: path.clone() }
                }
                AppliedChange::Move { source, target, .. } => PatchFileChange::Move {
                    source: source.clone(),
                    target: target.clone(),
                },
            })
            .collect()
    }

    pub fn summary(&self, paths: &impl PatchPathDisplay) -> String {
        let mut output = String::from("Success. Updated the following files:\n");
        for change in &self.applied {
            output.push_str(&change.summary_line(paths));
        }
        output
    }

    pub fn changed_paths(&self) -> Vec<PathBuf> {
        self.applied
            .iter()
            .filter_map(|change| match change {
                AppliedChange::Add { path, .. } | AppliedChange::Update { path, .. } => {
                    Some(path.clone())
                }
                AppliedChange::Move { target, .. } => Some(target.clone()),
                AppliedChange::Delete { .. } => None,
            })
            .collect()
    }

    pub fn deleted_paths(&self) -> Vec<PathBuf> {
        self.applied
            .iter()
            .filter_map(|change| match change {
                AppliedChange::Delete { path, .. } => Some(path.clone()),
                AppliedChange::Move { source, .. } => Some(source.clone()),
                AppliedChange::Add { .. } | AppliedChange::Update { .. } => None,
            })
            .collect()
    }

    /// 记录一条已应用的变更，返回其在结果中的索引，供 move 场景回填。
    pub(crate) fn record(&mut self, change: AppliedChange) -> usize {
        self.applied.push(change);
        self.applied.len() - 1
    }

    /// 用 move 变体回填之前占位的 add 变体。
    pub(crate) fn replace(&mut self, index: usize, change: AppliedChange) {
        self.applied[index] = change;
    }

    /// 标记一次写失败，失败摘要需要提示变更可能不完整。
    pub(crate) fn mark_inexact(&mut self) {
        self.exact = false;
    }

    pub(crate) fn failure_suffix(&self, paths: &impl PatchPathDisplay) -> String {
        if self.applied.is_empty() {
            let mut output = "\nNo files were modified before failure.".to_string();
            if !self.exact {
                output.push_str("\nA write may have partially modified a file before failure.");
            }
            return output;
        }

        let mut output = String::from("\nChanges applied before failure:\n");
        for change in &self.applied {
            output.push_str(&change.failure_line(paths));
        }
        if !self.exact {
            output.push_str("Applied changes may be incomplete because a write failed.\n");
        }
        output
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchFileChange {
    Add { path: PathBuf },
    Update { path: PathBuf },
    Delete { path: PathBuf },
    Move { source: PathBuf, target: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AppliedChange {
    Add {
        path: PathBuf,
        content: String,
        overwritten_content: Option<String>,
    },
    Update {
        path: PathBuf,
        old_content: String,
        new_content: String,
    },
    Delete {
        path: PathBuf,
        content: String,
    },
    Move {
        source: PathBuf,
        target: PathBuf,
        old_content: String,
        new_content: String,
        overwritten_target_content: Option<String>,
    },
}

impl AppliedChange {
    fn summary_line(&self, paths: &impl PatchPathDisplay) -> String {
        match self {
            Self::Add { path, .. } => format!("A {}\n", paths.display_path(path)),
            Self::Update { path, .. } => format!("M {}\n", paths.display_path(path)),
            Self::Delete { path, .. } => format!("D {}\n", paths.display_path(path)),
            Self::Move { target, .. } => format!("M {}\n", paths.display_path(target)),
        }
    }

    fn failure_line(&self, paths: &impl PatchPathDisplay) -> String {
        match self {
            Self::Add {
                path,
                content,
                overwritten_content,
            } => {
                let overwritten = overwritten_content
                    .as_ref()
                    .map(|content| format!(", overwrote {} bytes", content.len()))
                    .unwrap_or_default();
                format!(
                    "A {} ({} bytes{})\n",
                    paths.display_path(path),
                    content.len(),
                    overwritten
                )
            }
            Self::Update {
                path,
                old_content,
                new_content,
            } => format!(
                "M {} ({} -> {} bytes)\n",
                paths.display_path(path),
                old_content.len(),
                new_content.len()
            ),
            Self::Delete { path, content } => {
                format!("D {} ({} bytes)\n", paths.display_path(path), content.len())
            }
            Self::Move {
                source,
                target,
                old_content,
                new_content,
                overwritten_target_content,
            } => {
                let overwritten = overwritten_target_content
                    .as_ref()
                    .map(|content| format!(", overwrote {} bytes", content.len()))
                    .unwrap_or_default();
                format!(
                    "M {} -> {} ({} -> {} bytes{})\n",
                    paths.display_path(source),
                    paths.display_path(target),
                    old_content.len(),
                    new_content.len(),
                    overwritten
                )
            }
        }
    }
}
