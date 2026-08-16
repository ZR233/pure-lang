use std::collections::BTreeMap;
use std::fmt;

use pl_protocol::{PureError, Result};

use super::source::{RegistryRevision, ToolEntry, ToolRegistry, ToolSourceId};

/// 一次成功发布的 RAII guard。
///
/// guard 被 drop 时注销该来源当前发布（revision 递增并广播通知）。若同一来源
/// 之后又发布了新一代，旧 guard 的 drop 不影响新代——guard 只注销自己发布的
/// 那一代，保证 MCP 等 worker 重发布时不会误删后继工具集。
pub struct PublishGuard {
    registry: Option<ToolRegistry>,
    source: ToolSourceId,
    seq: u64,
}

impl fmt::Debug for PublishGuard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PublishGuard")
            .field("source", &self.source)
            .field("seq", &self.seq)
            .finish()
    }
}

impl Drop for PublishGuard {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.take() {
            registry.unpublish_generation(&self.source, self.seq);
        }
    }
}

impl ToolRegistry {
    /// 整组发布一个来源的全部工具条目。
    ///
    /// 先在锁内校验（条目名非空、来源内无重名、不与其他来源名称冲突），再原子
    /// 替换该来源条目并递增全局 revision、广播变更；校验失败返回
    /// [`PureError::ConfigError`]（含冲突名与两个来源），旧代原样保留。
    /// 若该来源现有条目与新条目语义相等（名称、canonical schema、元数据），则
    /// 不递增 revision 直接返回 guard。
    ///
    /// # Errors
    ///
    /// 条目名为空、来源内重名或与其他来源名称冲突时返回 `ConfigError`。
    pub fn publish(&self, source: ToolSourceId, entries: Vec<ToolEntry>) -> Result<PublishGuard> {
        let mut state = self.lock_state_mut();
        validate_entries(&source, &entries)?;
        validate_cross_source_names(&source, &entries, &state)?;
        if let Some(current) = state.sources.get(&source)
            && entries_semantically_equal(current, &entries)
        {
            return Ok(self.guard(source, current.seq));
        }
        state.next_seq = state.next_seq.saturating_add(1);
        let seq = state.next_seq;
        state.sources.insert(
            source.clone(),
            super::source::PublishedSource {
                seq,
                entries: entries.into(),
            },
        );
        let revision = advance_revision(&mut state);
        drop(state);
        self.broadcast_revision(revision);
        Ok(self.guard(source, seq))
    }

    /// 注销一个来源的全部当前条目。
    pub fn unpublish(&self, source: &ToolSourceId) {
        let mut state = self.lock_state_mut();
        if state.sources.remove(source).is_some() {
            let revision = advance_revision(&mut state);
            drop(state);
            self.broadcast_revision(revision);
        }
    }

    fn unpublish_generation(&self, source: &ToolSourceId, seq: u64) {
        let mut state = self.lock_state_mut();
        if state
            .sources
            .get(source)
            .is_some_and(|published| published.seq == seq)
        {
            state.sources.remove(source);
            let revision = advance_revision(&mut state);
            drop(state);
            self.broadcast_revision(revision);
        }
    }

    fn guard(&self, source: ToolSourceId, seq: u64) -> PublishGuard {
        PublishGuard {
            registry: Some(self.clone()),
            source,
            seq,
        }
    }
}

fn advance_revision(state: &mut super::source::RegistryState) -> RegistryRevision {
    state.revision = state.revision.saturating_add(1);
    RegistryRevision(state.revision)
}

fn entries_semantically_equal(
    current: &super::source::PublishedSource,
    next: &[ToolEntry],
) -> bool {
    current.entries.len() == next.len()
        && current
            .entries
            .iter()
            .zip(next.iter())
            .all(|(current, next)| current.semantic_eq(next))
}

fn validate_entries(source: &ToolSourceId, entries: &[ToolEntry]) -> Result<()> {
    let mut seen = BTreeMap::new();
    for entry in entries {
        let name = entry.name();
        if name.trim().is_empty() {
            return Err(PureError::ConfigError(format!(
                "tool source `{source}` attempted to publish an entry with an empty name"
            )));
        }
        if seen.insert(name.to_string(), ()).is_some() {
            return Err(PureError::ConfigError(format!(
                "tool source `{source}` attempted to publish duplicate tool name `{name}`"
            )));
        }
    }
    Ok(())
}

fn validate_cross_source_names(
    source: &ToolSourceId,
    entries: &[ToolEntry],
    state: &super::source::RegistryState,
) -> Result<()> {
    for entry in entries {
        let name = entry.name();
        if let Some((owner, _)) = state.sources.iter().find(|(owner, published)| {
            **owner != *source && published.entries.iter().any(|e| e.name() == name)
        }) {
            return Err(PureError::ConfigError(format!(
                "tool source `{source}` cannot publish tool `{name}`: already owned by source `{owner}`"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::tool::{OutputTruncation, Tool, ToolInput, ToolOutput};
    use crate::turn::ToolEffect;
    use futures::FutureExt;

    fn output() -> ToolOutput {
        ToolOutput {
            description: "ok".to_string(),
            truncated: OutputTruncation::empty(),
            output_file: std::path::PathBuf::new(),
            exit_code: Some(0),
            timed_out: false,
            runtime_events: Vec::new(),
        }
    }

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
            async { Ok(output()) }.boxed()
        }
    }

    fn entry(name: &'static str, source: &ToolSourceId) -> ToolEntry {
        ToolEntry::new(
            NamedTool(name),
            crate::tool::ToolSourceMetadata::new(source.clone()),
        )
    }

    #[test]
    fn publish_replaces_source_generation_atomically() {
        let registry = ToolRegistry::new();
        let source = ToolSourceId::builtin();
        let _guard = registry
            .publish(source.clone(), vec![entry("alpha", &source)])
            .unwrap();
        let first_revision = registry.revision();
        let _guard = registry
            .publish(
                source.clone(),
                vec![entry("beta", &source), entry("gamma", &source)],
            )
            .unwrap();

        let snapshot = registry.snapshot();
        assert_eq!(snapshot.names(), vec!["beta", "gamma"]);
        assert!(snapshot.revision.0 > first_revision.0);
    }

    #[test]
    fn publish_rejects_cross_source_name_conflicts_and_keeps_old_generation() {
        let registry = ToolRegistry::new();
        let builtin = ToolSourceId::builtin();
        let mcp = ToolSourceId::mcp();
        let _guard = registry
            .publish(builtin.clone(), vec![entry("shared", &builtin)])
            .unwrap();
        let revision = registry.revision();

        let error = registry
            .publish(mcp.clone(), vec![entry("shared", &mcp)])
            .unwrap_err();

        match error {
            PureError::ConfigError(message) => {
                assert!(message.contains("shared"), "{message}");
                assert!(message.contains("builtin"), "{message}");
                assert!(message.contains("mcp"), "{message}");
            }
            other => panic!("unexpected error: {other}"),
        }
        assert_eq!(registry.revision(), revision);
        assert_eq!(registry.snapshot().names(), vec!["shared"]);
    }

    #[test]
    fn publish_rejects_empty_names_and_intra_source_duplicates() {
        let registry = ToolRegistry::new();
        let source = ToolSourceId::task();

        assert!(registry.publish(source.clone(), vec![]).is_ok());
        assert!(
            registry
                .publish(
                    source.clone(),
                    vec![entry("dup", &source), entry("dup", &source)]
                )
                .is_err()
        );
    }

    #[test]
    fn identical_republish_is_a_no_op() {
        let registry = ToolRegistry::new();
        let source = ToolSourceId::lsp();
        let _guard = registry
            .publish(source.clone(), vec![entry("lsp_query", &source)])
            .unwrap();
        let revision = registry.revision();

        let _guard = registry
            .publish(source.clone(), vec![entry("lsp_query", &source)])
            .unwrap();

        assert_eq!(registry.revision(), revision);
    }

    #[test]
    fn guard_drop_unpublishes_only_its_own_generation() {
        let registry = ToolRegistry::new();
        let source = ToolSourceId::mcp();
        let old_guard = registry
            .publish(source.clone(), vec![entry("old_tool", &source)])
            .unwrap();
        let new_guard = registry
            .publish(source.clone(), vec![entry("new_tool", &source)])
            .unwrap();
        drop(old_guard);

        assert_eq!(registry.snapshot().names(), vec!["new_tool"]);
        drop(new_guard);
        assert!(registry.snapshot().names().is_empty());
    }

    #[test]
    fn revision_is_monotonic_across_operations() {
        let registry = ToolRegistry::new();
        let source = ToolSourceId::builtin();
        let mut last = registry.revision().0;
        for index in 0..3 {
            let _guard = registry
                .publish(source.clone(), vec![entry("tool", &source)])
                .unwrap();
            let revision = registry.revision().0;
            assert!(revision > last, "publish #{index} must increase revision");
            last = revision;
        }
        registry.unpublish(&source);
        assert!(registry.revision().0 > last);
    }

    #[test]
    fn subscribe_observes_published_revisions() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(async {
            let registry = ToolRegistry::new();
            let mut receiver = registry.subscribe();
            let source = ToolSourceId::builtin();
            let _guard = registry
                .publish(source.clone(), vec![entry("tool", &source)])
                .unwrap();

            assert!(receiver.changed().await.is_ok());
            assert_eq!(*receiver.borrow_and_update(), registry.revision());
        });
    }
}
