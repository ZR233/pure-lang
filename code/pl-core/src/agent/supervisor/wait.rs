use std::future::Future;

use tokio::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use super::{AgentSupervisor, AgentWaitOutcome, AgentWaitSnapshot};

const DEFAULT_AGENT_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// agent wait loop 的共享执行选项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentWaitLoopOptions {
    timeout: Duration,
    poll_interval: Duration,
}

impl AgentWaitLoopOptions {
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            poll_interval: DEFAULT_AGENT_WAIT_POLL_INTERVAL,
        }
    }

    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }
}

/// agent wait loop 的共享返回值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWaitLoopResult<T> {
    pub value: T,
    pub timed_out: bool,
}

/// agent wait loop 的共享错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentWaitLoopError<E> {
    Cancelled,
    Read(E),
}

/// 轮询宿主提供的 agent 快照，直到完成、超时或取消。
///
/// 宿主负责读取自身状态并投影成 `AgentWaitSnapshot`；pl-core 统一维护
/// wait loop 的完成判断、轮询节奏、超时语义和取消语义。
pub async fn wait_for_agent_completion<T, E, F, Fut>(
    mut read_snapshot: F,
    options: AgentWaitLoopOptions,
    cancellation_token: &CancellationToken,
) -> Result<AgentWaitLoopResult<T>, AgentWaitLoopError<E>>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(AgentWaitSnapshot, T), E>>,
{
    let deadline = Instant::now() + options.timeout;
    loop {
        if cancellation_token.is_cancelled() {
            return Err(AgentWaitLoopError::Cancelled);
        }
        let (snapshot, value) = read_snapshot().await.map_err(AgentWaitLoopError::Read)?;
        if snapshot.is_complete() {
            return Ok(AgentWaitLoopResult {
                value,
                timed_out: false,
            });
        }
        if Instant::now() >= deadline {
            return Ok(AgentWaitLoopResult {
                value,
                timed_out: true,
            });
        }
        let next_poll = std::cmp::min(Instant::now() + options.poll_interval, deadline);
        tokio::select! {
            _ = tokio::time::sleep_until(next_poll) => {},
            _ = cancellation_token.cancelled() => return Err(AgentWaitLoopError::Cancelled),
        }
    }
}

impl AgentSupervisor {
    pub async fn wait_for_activity(&self, timeout_ms: i64) -> AgentWaitOutcome {
        let timeout_ms = timeout_ms.clamp(250, 120_000) as u64;
        let start_seq = {
            let mut state = self.state.lock().await;
            if state.activity_seq > state.observed_activity_seq {
                state.observed_activity_seq = state.activity_seq;
                return AgentWaitOutcome { timed_out: false };
            }
            state.observed_activity_seq
        };
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut state = self.state.lock().await;
                if state.activity_seq > start_seq {
                    state.observed_activity_seq = state.activity_seq;
                    return AgentWaitOutcome { timed_out: false };
                }
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return AgentWaitOutcome { timed_out: true };
            }
        }
    }
}
