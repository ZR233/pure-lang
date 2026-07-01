use super::{AgentSupervisor, AgentWaitOutcome};

impl AgentSupervisor {
    pub async fn wait_for_activity(&self, timeout_ms: i64) -> AgentWaitOutcome {
        use tokio::time::{Duration, Instant};

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
