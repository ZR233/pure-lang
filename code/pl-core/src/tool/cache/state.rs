use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::entry::{CacheReuseKind, ToolCacheEntry, cache_entry, compact_cache_hit};
use super::failure::ToolFailureEnvelopeV1;
use super::key::cache_key;
use super::read_file::{ReadFileRequest, read_file_request};
use super::{ToolCachePolicy, TurnToolCacheHandle, TurnToolCacheSnapshot};
use crate::tool::ToolResult;
use crate::turn::ToolEffect;

#[derive(Debug, Default)]
pub(super) struct TurnToolCache {
    workspace_epoch: u64,
    entries: HashMap<String, ToolCacheEntry>,
    failures: HashMap<String, ToolFailureEnvelopeV1>,
    in_flight: HashMap<String, Vec<tokio::sync::oneshot::Sender<()>>>,
}

pub(super) enum CacheAcquisition {
    Hit(ToolResult),
    Failed(ToolFailureEnvelopeV1),
    Reserved(ToolCacheReservation),
    Wait(tokio::sync::oneshot::Receiver<()>),
}

pub(super) struct ToolCacheReservation {
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

    pub(super) fn acquire(
        &self,
        tool_name: &str,
        arguments: &Value,
        workspace_root: &Path,
        policy: ToolCachePolicy,
        workspace_epoch: u64,
        executor_generation: u64,
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
            executor_generation,
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
            executor_generation,
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
    pub(super) fn lookup(
        &self,
        tool_name: &str,
        arguments: &Value,
        workspace_root: &Path,
        policy: ToolCachePolicy,
    ) -> Option<ToolResult> {
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
            0,
        );
        state
            .entries
            .get(&key)
            .map(|entry| compact_cache_hit(entry, CacheReuseKind::Exact))
    }

    #[cfg(test)]
    pub(super) fn insert(
        &self,
        tool_name: &str,
        arguments: &Value,
        workspace_root: &Path,
        policy: ToolCachePolicy,
        call_id: String,
        output: &ToolResult,
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
            0,
        );
        let read_file_request = read_file_request(
            tool_name,
            arguments,
            workspace_root,
            policy,
            state.workspace_epoch,
            0,
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

impl ToolCacheReservation {
    pub(super) fn store(mut self, tool_name: &str, call_id: String, output: &ToolResult) {
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

    pub(super) fn store_failure(mut self, failure: ToolFailureEnvelopeV1) {
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
