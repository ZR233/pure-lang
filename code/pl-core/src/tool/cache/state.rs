use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::entry::{ToolCacheEntry, cache_entry, cache_hit};
use super::failure::ToolFailureEnvelopeV1;
use super::key::cache_key;
use super::{SessionToolCacheHandle, SessionToolCacheSnapshot, ToolCachePolicy};
use crate::tool::ToolResult;
use crate::turn::ToolEffect;

#[derive(Debug, Default)]
pub(super) struct SessionToolCache {
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
    inner: Arc<Mutex<SessionToolCache>>,
    key: Option<String>,
    epoch: u64,
}

impl SessionToolCacheHandle {
    pub(crate) fn snapshot(&self) -> SessionToolCacheSnapshot {
        SessionToolCacheSnapshot {
            cache: self.clone(),
        }
    }

    pub(super) fn acquire(
        &self,
        tool_name: &str,
        arguments: &Value,
        workspace_root: &Path,
        policy: ToolCachePolicy,
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
            state.workspace_epoch,
            executor_generation,
        );
        if let Some(entry) = state.entries.get(&key) {
            return CacheAcquisition::Hit(cache_hit(entry));
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
            epoch: state.workspace_epoch,
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
        state.entries.get(&key).map(cache_hit)
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
        state
            .entries
            .insert(key, cache_entry(tool_name, call_id, output));
    }

    pub(crate) fn record_effect(&self, effect: Option<ToolEffect>, _success: bool) {
        if matches!(
            effect,
            Some(
                ToolEffect::WorkspaceWrite
                    | ToolEffect::Process
                    | ToolEffect::BranchControl
                    | ToolEffect::AgentControl
            )
        ) {
            self.invalidate_all();
        }
    }

    pub(crate) fn invalidate_all(&self) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.workspace_epoch = state.workspace_epoch.saturating_add(1);
        state.entries.clear();
        state.failures.clear();
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
        let entry = serde_json::to_vec(output)
            .ok()
            .filter(|encoded| encoded.len() <= 64 * 1024)
            .map(|_| cache_entry(tool_name, call_id, output));
        let waiters = {
            let mut state = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let waiters = state.in_flight.remove(&key).unwrap_or_default();
            if self.epoch == state.workspace_epoch
                && state.entries.len() + state.failures.len() < 256
                && let Some(entry) = entry
            {
                state.entries.insert(key, entry);
            }
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
            if self.epoch == state.workspace_epoch
                && state.entries.len() + state.failures.len() < 256
            {
                state.failures.insert(key, failure);
            }
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
