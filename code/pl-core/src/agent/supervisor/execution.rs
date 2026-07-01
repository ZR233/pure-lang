use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pl_protocol::PureError;

#[derive(Debug, Default)]
pub(super) struct AgentExecutionLimiter {
    active: AtomicUsize,
    max_agents: AtomicUsize,
}

#[derive(Debug)]
pub(super) struct AgentExecutionGuard {
    limiter: Arc<AgentExecutionLimiter>,
}

impl Drop for AgentExecutionGuard {
    fn drop(&mut self) {
        self.limiter.active.fetch_sub(1, Ordering::AcqRel);
    }
}

impl AgentExecutionLimiter {
    pub(super) fn configure(&self, max_agents: usize) {
        self.max_agents.store(max_agents, Ordering::Release);
    }

    pub(super) fn guard(self: &Arc<Self>) -> Result<AgentExecutionGuard, PureError> {
        let max_agents = self.max_agents.load(Ordering::Acquire);
        let max_agents = if max_agents == 0 {
            usize::MAX
        } else {
            max_agents
        };
        let mut current = self.active.load(Ordering::Acquire);
        loop {
            if current >= max_agents {
                return Err(PureError::AgentLimitReached { max_agents });
            }
            match self.active.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(AgentExecutionGuard {
                        limiter: Arc::clone(self),
                    });
                }
                Err(updated) => current = updated,
            }
        }
    }
}
