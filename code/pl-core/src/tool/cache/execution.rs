use std::future::Future;
use std::path::Path;

use serde_json::Value;

use super::failure::deterministic_failure;
use super::state::CacheAcquisition;
use super::{ToolCachePolicy, TurnToolCacheSnapshot};
use crate::Result;
use crate::tool::ToolResult;

pub(crate) struct ToolCacheExecutionRequest<'a> {
    pub(crate) tool_name: &'a str,
    pub(crate) arguments: &'a Value,
    pub(crate) workspace_root: &'a Path,
    pub(crate) policy: ToolCachePolicy,
    pub(crate) call_id: String,
    pub(crate) executor_generation: u64,
}

impl TurnToolCacheSnapshot {
    pub(crate) async fn execute_or_reuse<F, Fut>(
        &self,
        request: ToolCacheExecutionRequest<'_>,
        execute: F,
    ) -> Result<ToolResult>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<ToolResult>>,
    {
        if request.policy == ToolCachePolicy::Never {
            return execute().await;
        }
        let mut execute = Some(execute);
        loop {
            match self.cache.acquire(
                request.tool_name,
                request.arguments,
                request.workspace_root,
                request.policy,
                self.workspace_epoch,
                request.executor_generation,
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
                        Ok(output) => reservation.store(request.tool_name, request.call_id, output),
                        Err(error) => {
                            if let Some(failure) =
                                deterministic_failure(request.tool_name, request.call_id, error)
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
