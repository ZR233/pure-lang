use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use crate::tool::command::CommandBackend;

use super::{CommandProcessEntry, CommandProcessTransition};

pub(super) async fn wait_for_process_activity(entry: &CommandProcessEntry, yield_time: Duration) {
    if yield_time.is_zero() {
        return;
    }
    let deadline = Instant::now() + yield_time;
    loop {
        if entry.is_final().await {
            break;
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let remaining = deadline.saturating_duration_since(now);
        if tokio::time::timeout(remaining, entry.notify.notified())
            .await
            .is_err()
        {
            break;
        }
    }
}

pub(super) fn spawn_lifecycle_task<B>(
    entry: Arc<CommandProcessEntry>,
    mut child: tokio::process::Child,
    timeout: Duration,
    cancellation_token: Option<CancellationToken>,
    backend: Arc<B>,
) where
    B: CommandBackend,
{
    tokio::spawn(async move {
        let outcome = wait_for_lifecycle_outcome(&mut child, timeout, cancellation_token).await;
        let wait_result = match outcome {
            LifecycleOutcome::Exited(result) => result,
            LifecycleOutcome::TimedOut => {
                apply_transition(&entry, CommandProcessTransition::TimedOut).await;
                close_stdin(&entry).await;
                backend.terminate(&entry.process_id, child.id()).await;
                let _ = child.start_kill();
                child.wait().await
            }
            LifecycleOutcome::Interrupted => {
                apply_transition(&entry, CommandProcessTransition::Interrupted).await;
                close_stdin(&entry).await;
                backend.terminate(&entry.process_id, child.id()).await;
                let _ = child.start_kill();
                child.wait().await
            }
        };
        {
            let mut stdin = entry.stdin.lock().await;
            stdin.take();
        }
        match wait_result {
            Ok(status) => {
                apply_transition(
                    &entry,
                    CommandProcessTransition::ProcessExited {
                        exit_code: status.code(),
                    },
                )
                .await;
            }
            Err(error) => {
                {
                    let mut state = entry.state.lock().await;
                    state.record_error(format!("failed to wait for process: {error}"));
                }
                apply_transition(&entry, CommandProcessTransition::ProcessWaitFailed).await;
            }
        }
    });
}

enum LifecycleOutcome {
    Exited(std::io::Result<std::process::ExitStatus>),
    TimedOut,
    Interrupted,
}

async fn wait_for_lifecycle_outcome(
    child: &mut tokio::process::Child,
    timeout: Duration,
    cancellation_token: Option<CancellationToken>,
) -> LifecycleOutcome {
    if let Some(token) = cancellation_token {
        tokio::select! {
            result = child.wait() => LifecycleOutcome::Exited(result),
            _ = tokio::time::sleep(timeout) => LifecycleOutcome::TimedOut,
            _ = token.cancelled() => LifecycleOutcome::Interrupted,
        }
    } else {
        tokio::select! {
            result = child.wait() => LifecycleOutcome::Exited(result),
            _ = tokio::time::sleep(timeout) => LifecycleOutcome::TimedOut,
        }
    }
}

pub(super) async fn apply_transition(
    entry: &CommandProcessEntry,
    transition: CommandProcessTransition,
) {
    let mut state = entry.state.lock().await;
    state.apply_transition(transition);
    drop(state);
    entry.notify.notify_waiters();
}

async fn close_stdin(entry: &CommandProcessEntry) {
    let mut stdin = entry.stdin.lock().await;
    stdin.take();
}
