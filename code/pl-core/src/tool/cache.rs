use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::turn::ToolEffect;

use super::{ToolOutput, ToolRuntimeEvent, model_visible_tool_output};

/// 工具结果在一次 turn 内的复用策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToolCachePolicy {
    #[default]
    Never,
    WithinTurn,
    UntilWorkspaceMutation,
}

/// turn-scoped 只读工具缓存。
#[derive(Debug, Clone, Default)]
pub struct TurnToolCacheHandle {
    inner: Arc<Mutex<TurnToolCache>>,
}

#[derive(Debug, Default)]
struct TurnToolCache {
    workspace_epoch: u64,
    entries: HashMap<String, ToolCacheEntry>,
    in_flight: HashMap<String, Vec<tokio::sync::oneshot::Sender<()>>>,
}

#[derive(Debug, Clone)]
struct ToolCacheEntry {
    tool_name: String,
    call_id: String,
    output: ToolOutput,
    result_hash: String,
    total_bytes: u64,
}

enum CacheAcquisition {
    Hit(ToolOutput),
    Reserved(ToolCacheReservation),
    Wait(tokio::sync::oneshot::Receiver<()>),
}

struct ToolCacheReservation {
    inner: Arc<Mutex<TurnToolCache>>,
    key: Option<String>,
}

impl TurnToolCacheHandle {
    pub(crate) async fn execute_or_reuse<F, Fut, Error>(
        &self,
        tool_name: &str,
        arguments: &Value,
        workspace_root: &Path,
        policy: ToolCachePolicy,
        call_id: String,
        execute: F,
    ) -> Result<ToolOutput, Error>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<ToolOutput, Error>>,
    {
        if policy == ToolCachePolicy::Never {
            return execute().await;
        }
        let mut execute = Some(execute);
        loop {
            match self.acquire(tool_name, arguments, workspace_root, policy) {
                CacheAcquisition::Hit(output) => return Ok(output),
                CacheAcquisition::Wait(waiter) => {
                    let _ = waiter.await;
                }
                CacheAcquisition::Reserved(reservation) => {
                    let result = execute
                        .take()
                        .expect("a cache waiter executes at most once")(
                    )
                    .await;
                    if let Ok(output) = &result {
                        reservation.store(tool_name, call_id, output);
                    }
                    return result;
                }
            }
        }
    }

    fn acquire(
        &self,
        tool_name: &str,
        arguments: &Value,
        workspace_root: &Path,
        policy: ToolCachePolicy,
    ) -> CacheAcquisition {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = cache_key(
            tool_name,
            arguments,
            workspace_root,
            policy,
            state.workspace_epoch,
        );
        if let Some(entry) = state.entries.get(&key) {
            return CacheAcquisition::Hit(compact_cache_hit(entry));
        }
        if let Some(waiters) = state.in_flight.get_mut(&key) {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            waiters.push(sender);
            return CacheAcquisition::Wait(receiver);
        }
        state.in_flight.insert(key.clone(), Vec::new());
        CacheAcquisition::Reserved(ToolCacheReservation {
            inner: Arc::clone(&self.inner),
            key: Some(key),
        })
    }

    #[cfg(test)]
    fn lookup(
        &self,
        tool_name: &str,
        arguments: &Value,
        workspace_root: &Path,
        policy: ToolCachePolicy,
    ) -> Option<ToolOutput> {
        if policy == ToolCachePolicy::Never {
            return None;
        }
        let state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = cache_key(
            tool_name,
            arguments,
            workspace_root,
            policy,
            state.workspace_epoch,
        );
        state.entries.get(&key).map(compact_cache_hit)
    }

    #[cfg(test)]
    fn insert(
        &self,
        tool_name: &str,
        arguments: &Value,
        workspace_root: &Path,
        policy: ToolCachePolicy,
        call_id: String,
        output: &ToolOutput,
    ) {
        if policy == ToolCachePolicy::Never {
            return;
        }
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = cache_key(
            tool_name,
            arguments,
            workspace_root,
            policy,
            state.workspace_epoch,
        );
        state
            .entries
            .insert(key, cache_entry(tool_name, call_id, output));
    }

    pub(crate) fn record_effect(&self, effect: Option<ToolEffect>, success: bool) {
        if !success {
            return;
        }
        if matches!(
            effect,
            Some(ToolEffect::WorkspaceWrite | ToolEffect::Process | ToolEffect::BranchControl)
        ) {
            let mut state = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.workspace_epoch = state.workspace_epoch.saturating_add(1);
        }
    }

    pub(crate) fn invalidate_tool(&self, tool_name: &str) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .entries
            .retain(|_, entry| entry.tool_name != tool_name);
    }

    pub fn workspace_epoch(&self) -> u64 {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .workspace_epoch
    }
}

impl ToolCacheReservation {
    fn store(mut self, tool_name: &str, call_id: String, output: &ToolOutput) {
        let key = self.key.take().expect("active cache reservation");
        let entry = cache_entry(tool_name, call_id, output);
        let waiters = {
            let mut state = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let waiters = state.in_flight.remove(&key).unwrap_or_default();
            state.entries.insert(key, entry);
            waiters
        };
        notify_waiters(waiters);
    }
}

impl Drop for ToolCacheReservation {
    fn drop(&mut self) {
        let Some(key) = self.key.take() else {
            return;
        };
        let waiters = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .in_flight
            .remove(&key)
            .unwrap_or_default();
        notify_waiters(waiters);
    }
}

fn notify_waiters(waiters: Vec<tokio::sync::oneshot::Sender<()>>) {
    for waiter in waiters {
        let _ = waiter.send(());
    }
}

fn cache_entry(tool_name: &str, call_id: String, output: &ToolOutput) -> ToolCacheEntry {
    let total_bytes = output.description.len() as u64;
    let result_hash = output
        .runtime_events
        .iter()
        .find_map(|event| match event {
            ToolRuntimeEvent::OutputMetrics { result_hash, .. } => Some(result_hash.clone()),
            ToolRuntimeEvent::SkillActivated { .. }
            | ToolRuntimeEvent::ToolResultRevision { .. }
            | ToolRuntimeEvent::OutputArtifacts { .. }
            | ToolRuntimeEvent::CacheHit { .. }
            | ToolRuntimeEvent::EndTurn => None,
        })
        .unwrap_or_else(|| {
            crate::working_set::canonical_content_hash(output.description.as_bytes())
        });
    ToolCacheEntry {
        tool_name: tool_name.to_string(),
        call_id,
        output: output.clone(),
        result_hash,
        total_bytes,
    }
}

fn compact_cache_hit(entry: &ToolCacheEntry) -> ToolOutput {
    let summary = model_visible_tool_output(&entry.output.description);
    let summary = summary.chars().take(512).collect::<String>();
    let description = serde_json::json!({
        "cacheHit": true,
        "reusedFromCallId": entry.call_id,
        "resultHash": entry.result_hash,
        "totalBytes": entry.total_bytes,
        "summary": summary,
    })
    .to_string();
    let mut output = entry.output.clone();
    output.description = description;
    let (artifact_bytes, result_hash) = output
        .runtime_events
        .iter()
        .find_map(|event| match event {
            ToolRuntimeEvent::OutputMetrics {
                artifact_bytes,
                result_hash,
                ..
            } => Some((*artifact_bytes, result_hash.clone())),
            ToolRuntimeEvent::SkillActivated { .. }
            | ToolRuntimeEvent::ToolResultRevision { .. }
            | ToolRuntimeEvent::OutputArtifacts { .. }
            | ToolRuntimeEvent::CacheHit { .. }
            | ToolRuntimeEvent::EndTurn => None,
        })
        .unwrap_or((0, entry.result_hash.clone()));
    output
        .runtime_events
        .retain(|event| !matches!(event, ToolRuntimeEvent::OutputMetrics { .. }));
    output.runtime_events.push(ToolRuntimeEvent::CacheHit {
        reused_from_call_id: entry.call_id.clone(),
        result_hash: entry.result_hash.clone(),
        total_bytes: entry.total_bytes,
    });
    output.runtime_events.push(ToolRuntimeEvent::OutputMetrics {
        raw_bytes: entry.total_bytes,
        model_visible_bytes: output.description.len() as u64,
        artifact_bytes,
        result_hash,
    });
    output
}

fn cache_key(
    tool_name: &str,
    arguments: &Value,
    workspace_root: &Path,
    policy: ToolCachePolicy,
    workspace_epoch: u64,
) -> String {
    let canonical_arguments = crate::working_set::canonical_json_string(arguments);
    let repository_view = repository_view(arguments);
    let epoch = match (policy, repository_view) {
        (ToolCachePolicy::UntilWorkspaceMutation, RepositoryView::Workspace) => workspace_epoch,
        (ToolCachePolicy::Never, _)
        | (ToolCachePolicy::WithinTurn, _)
        | (ToolCachePolicy::UntilWorkspaceMutation, RepositoryView::Project) => 0,
    };
    crate::working_set::canonical_content_hash(
        format!(
            "{tool_name}\0{}\0{}\0{repository_view:?}\0{epoch}",
            workspace_root.display(),
            canonical_arguments
        )
        .as_bytes(),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepositoryView {
    Project,
    Workspace,
}

fn repository_view(arguments: &Value) -> RepositoryView {
    if contains_project_path(arguments) {
        RepositoryView::Project
    } else {
        RepositoryView::Workspace
    }
}

fn contains_project_path(value: &Value) -> bool {
    match value {
        Value::String(value) => value == "/project/repo" || value.starts_with("/project/repo/"),
        Value::Array(items) => items.iter().any(contains_project_path),
        Value::Object(map) => map.values().any(contains_project_path),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::tool::OutputTruncation;

    fn output(description: &str) -> ToolOutput {
        ToolOutput {
            description: description.to_string(),
            truncated: OutputTruncation::empty(),
            output_file: PathBuf::new(),
            exit_code: Some(0),
            timed_out: false,
            runtime_events: Vec::new(),
        }
    }

    #[test]
    fn workspace_mutation_invalidates_workspace_but_not_project_view() {
        let cache = TurnToolCacheHandle::default();
        let root = Path::new("/workspace/repo");
        let workspace_args = serde_json::json!({"path": "src/lib.rs"});
        let project_args = serde_json::json!({"path": "/project/repo/AGENTS.md"});
        for (id, args) in [("workspace", &workspace_args), ("project", &project_args)] {
            cache.insert(
                "read_file",
                args,
                root,
                ToolCachePolicy::UntilWorkspaceMutation,
                id.to_string(),
                &output(id),
            );
        }

        cache.record_effect(Some(ToolEffect::WorkspaceWrite), true);

        assert_eq!(
            cache.lookup(
                "read_file",
                &workspace_args,
                root,
                ToolCachePolicy::UntilWorkspaceMutation,
            ),
            None
        );
        assert!(
            cache
                .lookup(
                    "read_file",
                    &project_args,
                    root,
                    ToolCachePolicy::UntilWorkspaceMutation,
                )
                .is_some()
        );
    }

    #[test]
    fn product_write_can_invalidate_only_its_read_cache() {
        let cache = TurnToolCacheHandle::default();
        let root = Path::new("/workspace/repo");
        let github = serde_json::json!({"method": "GET", "path": "/repos/o/r/pulls/1"});
        let file = serde_json::json!({"path": "src/lib.rs"});
        cache.insert(
            "github_api_request",
            &github,
            root,
            ToolCachePolicy::WithinTurn,
            "github-call".to_string(),
            &output("github"),
        );
        cache.insert(
            "read_file",
            &file,
            root,
            ToolCachePolicy::WithinTurn,
            "file-call".to_string(),
            &output("file"),
        );

        cache.invalidate_tool("github_api_request");

        assert_eq!(
            cache.lookup(
                "github_api_request",
                &github,
                root,
                ToolCachePolicy::WithinTurn,
            ),
            None
        );
        assert!(
            cache
                .lookup("read_file", &file, root, ToolCachePolicy::WithinTurn)
                .is_some()
        );
    }

    #[tokio::test]
    async fn concurrent_identical_reads_execute_once() {
        let cache = TurnToolCacheHandle::default();
        let executions = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for call in 0..4 {
            let cache = cache.clone();
            let executions = Arc::clone(&executions);
            tasks.push(tokio::spawn(async move {
                cache
                    .execute_or_reuse(
                        "read_file",
                        &serde_json::json!({"path": "src/lib.rs", "startLine": 1}),
                        Path::new("/workspace/repo"),
                        ToolCachePolicy::UntilWorkspaceMutation,
                        format!("call-{call}"),
                        || async move {
                            executions.fetch_add(1, Ordering::SeqCst);
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                            Ok::<_, std::convert::Infallible>(output("full file range"))
                        },
                    )
                    .await
                    .expect("infallible read")
            }));
        }

        let mut outputs = Vec::new();
        for task in tasks {
            outputs.push(task.await.expect("cache task joins"));
        }

        assert_eq!(executions.load(Ordering::SeqCst), 1);
        assert_eq!(
            outputs
                .iter()
                .filter(|output| output.description.contains("\"cacheHit\":true"))
                .count(),
            3
        );
    }
}
