use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::turn::ToolEffect;
use crate::{PureError, Result};

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

/// 单次 provider response 工具批次共享的缓存 epoch 快照。
#[derive(Debug, Clone)]
pub(crate) struct TurnToolCacheSnapshot {
    cache: TurnToolCacheHandle,
    workspace_epoch: u64,
}

#[derive(Debug, Default)]
struct TurnToolCache {
    workspace_epoch: u64,
    entries: HashMap<String, ToolCacheEntry>,
    failures: HashMap<String, ToolFailureEnvelopeV1>,
    in_flight: HashMap<String, Vec<tokio::sync::oneshot::Sender<()>>>,
}

/// 可安全复用的工具失败类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolFailureClassV1 {
    DeterministicLocalRead,
}

/// 同一 mutation epoch 内可复用的有界失败事实。
#[derive(Debug, Clone)]
pub(crate) struct ToolFailureEnvelopeV1 {
    class: ToolFailureClassV1,
    tool_name: String,
    original_call_id: String,
    error_hash: String,
    summary: String,
}

#[derive(Debug, Clone)]
struct ToolCacheEntry {
    tool_name: String,
    call_id: String,
    output: ToolOutput,
    result_hash: String,
    total_bytes: u64,
    read_file_range: Option<ReadFileRange>,
}

#[derive(Debug, Clone)]
struct ReadFileRequest {
    identity: String,
    start_line: u64,
    requested_end_line: u64,
}

#[derive(Debug, Clone)]
struct ReadFileRange {
    identity: String,
    start_line: u64,
    end_line: u64,
    reaches_eof: bool,
}

#[derive(Debug, Clone, Copy)]
enum CacheReuseKind {
    Exact,
    CoveredReadRange,
}

enum CacheAcquisition {
    Hit(ToolOutput),
    Failed(ToolFailureEnvelopeV1),
    Reserved(ToolCacheReservation),
    Wait(tokio::sync::oneshot::Receiver<()>),
}

struct ToolCacheReservation {
    inner: Arc<Mutex<TurnToolCache>>,
    key: Option<String>,
    read_file_request: Option<ReadFileRequest>,
}

impl TurnToolCacheHandle {
    pub(crate) fn snapshot(&self) -> TurnToolCacheSnapshot {
        TurnToolCacheSnapshot {
            cache: self.clone(),
            workspace_epoch: self.workspace_epoch(),
        }
    }

    fn acquire(
        &self,
        tool_name: &str,
        arguments: &Value,
        workspace_root: &Path,
        policy: ToolCachePolicy,
        workspace_epoch: u64,
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
            workspace_epoch,
        );
        if let Some(entry) = state.entries.get(&key) {
            return CacheAcquisition::Hit(compact_cache_hit(entry, CacheReuseKind::Exact));
        }
        let read_file_request = read_file_request(
            tool_name,
            arguments,
            workspace_root,
            policy,
            workspace_epoch,
        );
        if let Some(entry) = read_file_request.as_ref().and_then(|request| {
            state.entries.values().find(|entry| {
                entry
                    .read_file_range
                    .as_ref()
                    .is_some_and(|range| range.covers(request))
            })
        }) {
            return CacheAcquisition::Hit(compact_cache_hit(
                entry,
                CacheReuseKind::CoveredReadRange,
            ));
        }
        if let Some(failure) = state.failures.get(&key) {
            return CacheAcquisition::Failed(failure.clone());
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
            read_file_request,
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
        state
            .entries
            .get(&key)
            .map(|entry| compact_cache_hit(entry, CacheReuseKind::Exact))
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
        let read_file_request = read_file_request(
            tool_name,
            arguments,
            workspace_root,
            policy,
            state.workspace_epoch,
        );
        state.entries.insert(
            key,
            cache_entry(tool_name, call_id, output, read_file_request.as_ref()),
        );
    }

    pub(crate) fn record_effect(&self, effect: Option<ToolEffect>, _success: bool) {
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
        state
            .failures
            .retain(|_, failure| failure.tool_name != tool_name);
    }

    pub fn workspace_epoch(&self) -> u64 {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .workspace_epoch
    }
}

impl TurnToolCacheSnapshot {
    pub(crate) async fn execute_or_reuse<F, Fut>(
        &self,
        tool_name: &str,
        arguments: &Value,
        workspace_root: &Path,
        policy: ToolCachePolicy,
        call_id: String,
        execute: F,
    ) -> Result<ToolOutput>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<ToolOutput>>,
    {
        if policy == ToolCachePolicy::Never {
            return execute().await;
        }
        let mut execute = Some(execute);
        loop {
            match self.cache.acquire(
                tool_name,
                arguments,
                workspace_root,
                policy,
                self.workspace_epoch,
            ) {
                CacheAcquisition::Hit(output) => return Ok(output),
                CacheAcquisition::Failed(failure) => {
                    tracing::debug!(
                        tool = failure.tool_name,
                        failure_class = ?failure.class,
                        reused_from_call_id = failure.original_call_id,
                        "reused deterministic tool failure"
                    );
                    return Err(failure.duplicate_error());
                }
                CacheAcquisition::Wait(waiter) => {
                    let _ = waiter.await;
                }
                CacheAcquisition::Reserved(reservation) => {
                    let result = execute
                        .take()
                        .expect("a cache waiter executes at most once")(
                    )
                    .await;
                    match &result {
                        Ok(output) => reservation.store(tool_name, call_id, output),
                        Err(error) => {
                            if let Some(failure) = deterministic_failure(tool_name, call_id, error)
                            {
                                reservation.store_failure(failure);
                            }
                        }
                    }
                    return result;
                }
            }
        }
    }
}

impl ToolCacheReservation {
    fn store(mut self, tool_name: &str, call_id: String, output: &ToolOutput) {
        let key = self.key.take().expect("active cache reservation");
        let entry = cache_entry(tool_name, call_id, output, self.read_file_request.as_ref());
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

    fn store_failure(mut self, failure: ToolFailureEnvelopeV1) {
        let key = self.key.take().expect("active cache reservation");
        let waiters = {
            let mut state = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let waiters = state.in_flight.remove(&key).unwrap_or_default();
            state.failures.insert(key, failure);
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

fn cache_entry(
    tool_name: &str,
    call_id: String,
    output: &ToolOutput,
    read_file_request: Option<&ReadFileRequest>,
) -> ToolCacheEntry {
    let total_bytes = output.description.len() as u64;
    let result_hash = output
        .runtime_events
        .iter()
        .find_map(|event| match event {
            ToolRuntimeEvent::OutputMetrics { result_hash, .. } => Some(result_hash.clone()),
            ToolRuntimeEvent::InteractionRequested { .. }
            | ToolRuntimeEvent::SkillActivated { .. }
            | ToolRuntimeEvent::ToolResultRevision { .. }
            | ToolRuntimeEvent::OutputArtifacts { .. }
            | ToolRuntimeEvent::AuditMetadata { .. }
            | ToolRuntimeEvent::ExecutionFailed
            | ToolRuntimeEvent::CacheHit { .. }
            | ToolRuntimeEvent::OutputBudget { .. }
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
        read_file_range: read_file_request
            .and_then(|request| ReadFileRange::from_output(request, output)),
    }
}

fn compact_cache_hit(entry: &ToolCacheEntry, reuse_kind: CacheReuseKind) -> ToolOutput {
    let summary = model_visible_tool_output(&entry.output.description);
    let summary = summary.chars().take(512).collect::<String>();
    let description = serde_json::json!({
        "cacheHit": true,
        "reusedFromCallId": entry.call_id,
        "resultHash": entry.result_hash,
        "totalBytes": entry.total_bytes,
        "reuseKind": match reuse_kind {
            CacheReuseKind::Exact => "exact",
            CacheReuseKind::CoveredReadRange => "coveredReadRange",
        },
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
            ToolRuntimeEvent::InteractionRequested { .. }
            | ToolRuntimeEvent::SkillActivated { .. }
            | ToolRuntimeEvent::ToolResultRevision { .. }
            | ToolRuntimeEvent::OutputArtifacts { .. }
            | ToolRuntimeEvent::AuditMetadata { .. }
            | ToolRuntimeEvent::ExecutionFailed
            | ToolRuntimeEvent::CacheHit { .. }
            | ToolRuntimeEvent::OutputBudget { .. }
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

impl ReadFileRange {
    fn from_output(request: &ReadFileRequest, output: &ToolOutput) -> Option<Self> {
        let value = serde_json::from_str::<Value>(&output.description).ok()?;
        let start_line = value.get("startLine")?.as_u64()?;
        let end_line = value.get("endLine")?.as_u64()?;
        if start_line != request.start_line || end_line < start_line {
            return None;
        }
        Some(Self {
            identity: request.identity.clone(),
            start_line,
            end_line,
            reaches_eof: value.get("nextStartLine").is_some_and(Value::is_null),
        })
    }

    fn covers(&self, request: &ReadFileRequest) -> bool {
        self.identity == request.identity
            && self.start_line <= request.start_line
            && request.start_line <= self.end_line
            && (self.end_line >= request.requested_end_line || self.reaches_eof)
    }
}

fn read_file_request(
    tool_name: &str,
    arguments: &Value,
    workspace_root: &Path,
    policy: ToolCachePolicy,
    workspace_epoch: u64,
) -> Option<ReadFileRequest> {
    if tool_name != "read_file" {
        return None;
    }
    let path = arguments.get("path")?.as_str()?;
    let cwd = arguments.get("cwd").and_then(Value::as_str);
    let start_line = arguments
        .get("startLine")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let max_lines = arguments
        .get("maxLines")
        .and_then(Value::as_u64)
        .unwrap_or(200);
    if start_line == 0 || max_lines == 0 {
        return None;
    }
    let repository_view = repository_view(arguments);
    let epoch = effective_epoch(policy, repository_view, workspace_epoch);
    let identity_arguments = serde_json::json!({
        "path": path,
        "cwd": cwd,
    });
    let identity = crate::working_set::canonical_content_hash(
        format!(
            "read_file\0{}\0{}\0{repository_view:?}\0{epoch}",
            workspace_root.display(),
            crate::working_set::canonical_json_string(&identity_arguments),
        )
        .as_bytes(),
    );
    Some(ReadFileRequest {
        identity,
        start_line,
        requested_end_line: start_line.saturating_add(max_lines.saturating_sub(1)),
    })
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
    let epoch = effective_epoch(policy, repository_view, workspace_epoch);
    crate::working_set::canonical_content_hash(
        format!(
            "{tool_name}\0{}\0{}\0{repository_view:?}\0{epoch}",
            workspace_root.display(),
            canonical_arguments
        )
        .as_bytes(),
    )
}

fn effective_epoch(
    policy: ToolCachePolicy,
    repository_view: RepositoryView,
    workspace_epoch: u64,
) -> u64 {
    match (policy, repository_view) {
        (ToolCachePolicy::UntilWorkspaceMutation, RepositoryView::Workspace) => workspace_epoch,
        (ToolCachePolicy::Never, _)
        | (ToolCachePolicy::WithinTurn, _)
        | (ToolCachePolicy::UntilWorkspaceMutation, RepositoryView::Project) => 0,
    }
}

fn deterministic_failure(
    tool_name: &str,
    original_call_id: String,
    error: &PureError,
) -> Option<ToolFailureEnvelopeV1> {
    if !matches!(
        tool_name,
        "read_file" | "list_files" | "search_files" | "stat_path"
    ) {
        return None;
    }
    if !matches!(
        error,
        PureError::ToolExecutionFailed { .. }
            | PureError::Io(_)
            | PureError::ConfigError(_)
            | PureError::SandboxError(_)
    ) {
        return None;
    }
    let full = error.to_string();
    let summary = full.chars().take(512).collect::<String>();
    Some(ToolFailureEnvelopeV1 {
        class: ToolFailureClassV1::DeterministicLocalRead,
        tool_name: tool_name.to_string(),
        original_call_id,
        error_hash: crate::working_set::canonical_content_hash(full.as_bytes()),
        summary,
    })
}

impl ToolFailureEnvelopeV1 {
    fn duplicate_error(&self) -> PureError {
        PureError::ToolExecutionFailed {
            tool: self.tool_name.clone(),
            error: serde_json::json!({
                "duplicateFailure": true,
                "class": "deterministicLocalRead",
                "reusedFromCallId": self.original_call_id,
                "errorHash": self.error_hash,
                "summary": self.summary,
            })
            .to_string(),
        }
    }
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

    fn read_file_output(
        start_line: u64,
        end_line: u64,
        next_start_line: Option<u64>,
    ) -> ToolOutput {
        output(
            &serde_json::json!({
                "path": "src/lib.rs",
                "startLine": start_line,
                "endLine": end_line,
                "nextStartLine": next_start_line,
                "contentHash": "sha256:test",
                "text": "file contents",
            })
            .to_string(),
        )
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
                    .snapshot()
                    .execute_or_reuse(
                        "read_file",
                        &serde_json::json!({"path": "src/lib.rs", "startLine": 1}),
                        Path::new("/workspace/repo"),
                        ToolCachePolicy::UntilWorkspaceMutation,
                        format!("call-{call}"),
                        || async move {
                            executions.fetch_add(1, Ordering::SeqCst);
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                            Ok(output("full file range"))
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

    #[tokio::test]
    async fn provider_response_snapshot_keeps_failure_key_stable_while_epoch_advances() {
        let cache = TurnToolCacheHandle::default();
        let snapshot = cache.snapshot();
        let executions = Arc::new(AtomicUsize::new(0));
        let first_started = Arc::new(tokio::sync::Notify::new());
        let release_first = Arc::new(tokio::sync::Notify::new());
        let arguments = serde_json::json!({
            "path": "missing.rs",
            "startLine": 1,
        });
        let root = Path::new("/workspace/repo");

        let first = {
            let snapshot = snapshot.clone();
            let executions = Arc::clone(&executions);
            let first_started = Arc::clone(&first_started);
            let release_first = Arc::clone(&release_first);
            let arguments = arguments.clone();
            tokio::spawn(async move {
                snapshot
                    .execute_or_reuse(
                        "read_file",
                        &arguments,
                        root,
                        ToolCachePolicy::UntilWorkspaceMutation,
                        "first".to_string(),
                        || async move {
                            executions.fetch_add(1, Ordering::SeqCst);
                            first_started.notify_one();
                            release_first.notified().await;
                            Err(PureError::ToolExecutionFailed {
                                tool: "read_file".to_string(),
                                error: "path does not exist".to_string(),
                            })
                        },
                    )
                    .await
            })
        };

        first_started.notified().await;
        cache.record_effect(Some(ToolEffect::Process), true);

        let second = {
            let snapshot = snapshot.clone();
            let executions = Arc::clone(&executions);
            let arguments = arguments.clone();
            tokio::spawn(async move {
                snapshot
                    .execute_or_reuse(
                        "read_file",
                        &arguments,
                        root,
                        ToolCachePolicy::UntilWorkspaceMutation,
                        "second".to_string(),
                        || async move {
                            executions.fetch_add(1, Ordering::SeqCst);
                            Err(PureError::ToolExecutionFailed {
                                tool: "read_file".to_string(),
                                error: "duplicate execution".to_string(),
                            })
                        },
                    )
                    .await
            })
        };

        tokio::task::yield_now().await;
        release_first.notify_one();
        let first_error = first
            .await
            .expect("first cache task joins")
            .expect_err("first read fails");
        let second_error = second
            .await
            .expect("second cache task joins")
            .expect_err("duplicate read returns compact failure");

        assert!(!first_error.to_string().contains("duplicateFailure"));
        assert!(second_error.to_string().contains("duplicateFailure"));
        assert!(second_error.to_string().contains("first"));
        assert_eq!(executions.load(Ordering::SeqCst), 1);

        let executions_after_batch = Arc::clone(&executions);
        let third_error = cache
            .snapshot()
            .execute_or_reuse(
                "read_file",
                &arguments,
                root,
                ToolCachePolicy::UntilWorkspaceMutation,
                "third".to_string(),
                || async move {
                    executions_after_batch.fetch_add(1, Ordering::SeqCst);
                    Err(PureError::ToolExecutionFailed {
                        tool: "read_file".to_string(),
                        error: "path still does not exist".to_string(),
                    })
                },
            )
            .await
            .expect_err("new provider response uses the advanced epoch");
        assert!(!third_error.to_string().contains("duplicateFailure"));
        assert_eq!(executions.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn covered_read_file_range_returns_compact_receipt_without_reexecution() {
        let cache = TurnToolCacheHandle::default();
        let executions = Arc::new(AtomicUsize::new(0));
        let root = Path::new("/workspace/repo");

        let first_executions = Arc::clone(&executions);
        cache
            .snapshot()
            .execute_or_reuse(
                "read_file",
                &serde_json::json!({
                    "path": "src/lib.rs",
                    "startLine": 1,
                    "maxLines": 430,
                }),
                root,
                ToolCachePolicy::UntilWorkspaceMutation,
                "first".to_string(),
                || async move {
                    first_executions.fetch_add(1, Ordering::SeqCst);
                    Ok(read_file_output(1, 430, Some(431)))
                },
            )
            .await
            .expect("first read succeeds");

        let covered_executions = Arc::clone(&executions);
        let covered = cache
            .snapshot()
            .execute_or_reuse(
                "read_file",
                &serde_json::json!({
                    "path": "src/lib.rs",
                    "startLine": 100,
                    "maxLines": 200,
                }),
                root,
                ToolCachePolicy::UntilWorkspaceMutation,
                "covered".to_string(),
                || async move {
                    covered_executions.fetch_add(1, Ordering::SeqCst);
                    Ok(read_file_output(100, 299, Some(300)))
                },
            )
            .await
            .expect("covered read is reused");
        assert_eq!(executions.load(Ordering::SeqCst), 1);
        assert!(covered.description.contains("\"cacheHit\":true"));
        assert!(
            covered
                .description
                .contains("\"reuseKind\":\"coveredReadRange\"")
        );

        let expanded_executions = Arc::clone(&executions);
        cache
            .snapshot()
            .execute_or_reuse(
                "read_file",
                &serde_json::json!({
                    "path": "src/lib.rs",
                    "startLine": 300,
                    "maxLines": 200,
                }),
                root,
                ToolCachePolicy::UntilWorkspaceMutation,
                "expanded".to_string(),
                || async move {
                    expanded_executions.fetch_add(1, Ordering::SeqCst);
                    Ok(read_file_output(300, 499, Some(500)))
                },
            )
            .await
            .expect("uncovered suffix executes");
        assert_eq!(executions.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn deterministic_read_failure_is_reused_until_any_workspace_mutation_attempt() {
        let cache = TurnToolCacheHandle::default();
        let executions = Arc::new(AtomicUsize::new(0));
        let arguments = serde_json::json!({"path": "missing.rs"});
        let root = Path::new("/workspace/repo");

        for call_id in ["first", "second"] {
            let executions = Arc::clone(&executions);
            let error = cache
                .snapshot()
                .execute_or_reuse(
                    "read_file",
                    &arguments,
                    root,
                    ToolCachePolicy::UntilWorkspaceMutation,
                    call_id.to_string(),
                    || async move {
                        executions.fetch_add(1, Ordering::SeqCst);
                        Err(PureError::ToolExecutionFailed {
                            tool: "read_file".to_string(),
                            error: "path does not exist".to_string(),
                        })
                    },
                )
                .await
                .unwrap_err();
            if call_id == "second" {
                assert!(error.to_string().contains("duplicateFailure"));
                assert!(error.to_string().contains("first"));
            }
        }
        assert_eq!(executions.load(Ordering::SeqCst), 1);

        cache.record_effect(Some(ToolEffect::WorkspaceWrite), false);
        let executions_after_mutation = Arc::clone(&executions);
        let _ = cache
            .snapshot()
            .execute_or_reuse(
                "read_file",
                &arguments,
                root,
                ToolCachePolicy::UntilWorkspaceMutation,
                "third".to_string(),
                || async move {
                    executions_after_mutation.fetch_add(1, Ordering::SeqCst);
                    Err(PureError::ToolExecutionFailed {
                        tool: "read_file".to_string(),
                        error: "path still does not exist".to_string(),
                    })
                },
            )
            .await;
        assert_eq!(executions.load(Ordering::SeqCst), 2);
    }
}
