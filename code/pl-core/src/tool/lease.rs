use std::sync::Arc;

use pl_protocol::{PureError, Result};

use super::source::{RegistryRevision, RegistrySnapshot, ToolEntry};

/// 一次 Turn 冻结的工具条目集合。
///
/// lease 在 Turn 开始时从引擎本地注册表与共享注册表快照合并而来；活动 Turn
/// 持有的 lease 不受后续 publish 影响。名称冲突在合并时立即失败，避免运行期
/// 出现同名不同实现的歧义调度。
#[derive(Debug, Clone)]
pub struct TurnToolLease {
    revision: RegistryRevision,
    entries: Arc<[ToolEntry]>,
}

impl TurnToolLease {
    /// 合并引擎本地与共享注册表快照；revision 取两者较大值。
    ///
    /// # Errors
    ///
    /// 两个快照存在同名条目时返回 `ConfigError`（含冲突名）。
    pub fn merge(primary: RegistrySnapshot, shared: Option<RegistrySnapshot>) -> Result<Self> {
        let Some(shared) = shared else {
            return Ok(Self {
                revision: primary.revision,
                entries: primary.entries,
            });
        };
        let revision = primary.revision.max(shared.revision);
        let mut entries = Vec::with_capacity(primary.entries.len() + shared.entries.len());
        let (mut left_iter, mut right_iter) = (primary.entries.iter(), shared.entries.iter());
        let (mut left, mut right) = (left_iter.next(), right_iter.next());
        while let (Some(left_entry), Some(right_entry)) = (left, right) {
            match left_entry.name().cmp(right_entry.name()) {
                std::cmp::Ordering::Less => {
                    entries.push(left_entry.clone());
                    left = left_iter.next();
                }
                std::cmp::Ordering::Greater => {
                    entries.push(right_entry.clone());
                    right = right_iter.next();
                }
                std::cmp::Ordering::Equal => {
                    return Err(PureError::ConfigError(format!(
                        "tool `{}` is published by both the engine registry and the shared registry",
                        left_entry.name()
                    )));
                }
            }
        }
        entries.extend(left.cloned());
        entries.extend(left_iter.cloned());
        entries.extend(right.cloned());
        entries.extend(right_iter.cloned());
        Ok(Self {
            revision,
            entries: entries.into(),
        })
    }

    pub fn entry(&self, name: &str) -> Option<&ToolEntry> {
        self.entries.iter().find(|entry| entry.name() == name)
    }

    pub fn entries(&self) -> &[ToolEntry] {
        &self.entries
    }

    pub fn revision(&self) -> RegistryRevision {
        self.revision
    }

    pub fn names(&self) -> Vec<&str> {
        self.entries.iter().map(ToolEntry::name).collect()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::tool::{OutputTruncation, Tool, ToolInput, ToolOutput, ToolSourceId};
    use crate::turn::ToolEffect;
    use futures::FutureExt;

    #[derive(Debug)]
    struct NamedTool(&'static str);

    impl Tool for NamedTool {
        fn name(&self) -> &str {
            self.0
        }

        fn description(&self) -> &str {
            self.0
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({ "type": "object" })
        }

        fn effect(&self) -> Option<ToolEffect> {
            Some(ToolEffect::Read)
        }

        fn execute<'a>(
            &'a self,
            _input: ToolInput,
            _context: crate::tool::ToolContext,
        ) -> futures::future::BoxFuture<'a, std::result::Result<ToolOutput, pl_protocol::PureError>>
        {
            async {
                Ok(ToolOutput {
                    description: "ok".to_string(),
                    truncated: OutputTruncation::empty(),
                    output_file: std::path::PathBuf::new(),
                    exit_code: Some(0),
                    timed_out: false,
                    runtime_events: Vec::new(),
                })
            }
            .boxed()
        }
    }

    fn snapshot(names: &[&'static str], revision: u64) -> RegistrySnapshot {
        let source = ToolSourceId::builtin();
        let entries = names
            .iter()
            .map(|name| {
                ToolEntry::new(
                    NamedTool(name),
                    crate::tool::ToolSourceMetadata::new(source.clone()),
                )
            })
            .collect::<Vec<_>>();
        RegistrySnapshot {
            revision: RegistryRevision(revision),
            entries: entries.into(),
        }
    }

    #[test]
    fn merge_combines_disjoint_sources_sorted_by_name() {
        let primary = snapshot(&["exec", "read_file"], 3);
        let shared = snapshot(&["mcp__github__get_pr"], 9);

        let lease = TurnToolLease::merge(primary, Some(shared)).unwrap();

        assert_eq!(
            lease.names(),
            vec!["exec", "mcp__github__get_pr", "read_file"]
        );
        assert_eq!(lease.revision(), RegistryRevision(9));
        assert!(lease.entry("exec").is_some());
        assert!(lease.entry("missing").is_none());
    }

    #[test]
    fn merge_rejects_name_conflicts_between_registries() {
        let primary = snapshot(&["read_file"], 1);
        let shared = snapshot(&["read_file"], 2);

        let error = TurnToolLease::merge(primary, Some(shared)).unwrap_err();

        match error {
            PureError::ConfigError(message) => {
                assert!(message.contains("read_file"), "{message}");
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn merge_without_shared_snapshot_keeps_primary_entries() {
        let primary = snapshot(&["exec"], 7);

        let lease = TurnToolLease::merge(primary, None).unwrap();

        assert_eq!(lease.names(), vec!["exec"]);
        assert_eq!(lease.revision(), RegistryRevision(7));
    }

    #[test]
    fn tail_entries_from_either_side_are_preserved() {
        let primary = snapshot(&["alpha", "zeta"], 1);
        let shared = snapshot(&["beta"], 2);

        let lease = TurnToolLease::merge(primary, Some(shared)).unwrap();

        assert_eq!(lease.names(), vec!["alpha", "beta", "zeta"]);
    }
}
