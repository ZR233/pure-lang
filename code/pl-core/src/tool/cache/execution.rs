use std::future::Future;
use std::path::Path;

use serde_json::Value;

use super::failure::deterministic_failure;
use super::state::CacheAcquisition;
use super::{ToolCachePolicy, TurnToolCacheSnapshot};
use crate::Result;
use crate::tool::ToolOutput;

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
