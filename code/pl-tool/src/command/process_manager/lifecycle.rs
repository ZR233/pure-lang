use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use crate::command::ManagedCommand;

use super::{CommandProcessEntry, CommandProcessTransition};

pub(super) async fn wait_for_process_activity(entry: &CommandProcessEntry, yield_time: Duration) {
    if yield_time.is_zero() {
        return;
    }
    let deadline = Instant::now() + yield_time;
    loop {
        let notified = entry.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if entry.is_final().await {
            break;
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let remaining = deadline.saturating_duration_since(now);
        if tokio::time::timeout(remaining, notified).await.is_err() {
            break;
        }
    }
}

pub(super) struct CommandLifecycle {
    pub timeout: Duration,
    pub task_cancellation: Option<CancellationToken>,
    pub manager_cancellation: CancellationToken,
}

pub(super) fn spawn_lifecycle_task(
    entry: Arc<CommandProcessEntry>,
    mut child: ManagedCommand,
    lifecycle: CommandLifecycle,
) {
    tokio::spawn(async move {
        let outcome = wait_for_lifecycle_outcome(&mut child, lifecycle).await;
        let wait_result = match outcome {
            LifecycleOutcome::Exited(result) => result,
            LifecycleOutcome::TimedOut => {
                apply_transition(&entry, CommandProcessTransition::TimeOut).await;
                child.cancellation().cancel();
                child.wait().await
            }
            LifecycleOutcome::Interrupted => {
                apply_transition(&entry, CommandProcessTransition::Cancel).await;
                child.cancellation().cancel();
                child.wait().await
            }
        };
        // Poll the owned execution through cancellation before taking stdin: an in-flight
        // write may hold this mutex until the physical process closes its read end.
        {
            let mut stdin = entry.stdin.lock().await;
            stdin.take();
        }
        match wait_result {
            Ok(status) => {
                apply_transition(
                    &entry,
                    CommandProcessTransition::ProcessExited {
                        exit_code: status.exit_code,
                    },
                )
                .await;
            }
            Err(error) => {
                apply_transition(
                    &entry,
                    CommandProcessTransition::ProcessWaitFailed {
                        error: format!("failed to wait for process: {error}"),
                    },
                )
                .await;
            }
        }
    });
}

enum LifecycleOutcome {
    Exited(std::result::Result<crate::command::CommandExit, String>),
    TimedOut,
    Interrupted,
}

async fn wait_for_lifecycle_outcome(
    child: &mut ManagedCommand,
    lifecycle: CommandLifecycle,
) -> LifecycleOutcome {
    let cancelled = async {
        match lifecycle.task_cancellation {
            Some(token) => token.cancelled().await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        result = child.wait() => LifecycleOutcome::Exited(result),
        _ = tokio::time::sleep(lifecycle.timeout) => LifecycleOutcome::TimedOut,
        _ = cancelled => LifecycleOutcome::Interrupted,
        _ = lifecycle.manager_cancellation.cancelled() => LifecycleOutcome::Interrupted,
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
