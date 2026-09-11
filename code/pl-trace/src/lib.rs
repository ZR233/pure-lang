//! Read-only execution diagnostics derived from canonical core Thread commits.
use pl_core::thread::{AttemptOutcome, ToolOutcome, journal::ThreadCommit};
use serde::{Deserialize, Serialize};

/// Observed invocation counts. These counts do not assert provider cache hits or token estimates.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDiagnostics {
    pub admitted_attempts: u64,
    pub committed_attempts: u64,
    pub failed_attempts: u64,
    pub cancelled_attempts: u64,
    pub rejected_attempts: u64,
    pub interrupted_attempts: u64,
    pub explicit_retries: u64,
    pub successful_tools: u64,
    pub failed_tools: u64,
    pub cancelled_tools: u64,
    pub interrupted_tools: u64,
    pub context_replacements: u64,
    pub usage: pl_core::model::ModelUsage,
}

/// Summarizes immutable facts; callers validate/replay the journal through core before diagnosis.
/// No tool, provider protocol or product configuration is consulted.
///
/// # Errors
/// Returns core journal validation failures instead of reporting statistics from corrupt history.
pub fn diagnose(
    commits: &[std::sync::Arc<ThreadCommit>],
) -> Result<ThreadDiagnostics, pl_core::thread::ThreadError> {
    pl_core::thread::journal::replay(commits)?;
    let mut report = ThreadDiagnostics::default();
    let mut usage = UsageAccumulator::default();
    for commit in commits {
        report.context_replacements += commit.replacements.len() as u64;
        if let Some(attempt) = &commit.attempt {
            match &attempt.outcome {
                AttemptOutcome::Running => {}
                AttemptOutcome::Committed(output) | AttemptOutcome::Rejected { output, .. } => {
                    usage.add(&output.usage)
                }
                AttemptOutcome::Failed(error) => usage.add(&error.usage),
                AttemptOutcome::Cancelled { result } => usage.add(match result {
                    Ok(output) => &output.usage,
                    Err(error) => &error.usage,
                }),
                AttemptOutcome::Interrupted => usage.add(&Default::default()),
            }
            match &attempt.outcome {
                AttemptOutcome::Running => {
                    report.admitted_attempts += 1;
                    report.explicit_retries += u64::from(attempt.retry_of.is_some());
                }
                AttemptOutcome::Committed(_) => report.committed_attempts += 1,
                AttemptOutcome::Failed(_) => report.failed_attempts += 1,
                AttemptOutcome::Cancelled { .. } => report.cancelled_attempts += 1,
                AttemptOutcome::Rejected { .. } => report.rejected_attempts += 1,
                AttemptOutcome::Interrupted => report.interrupted_attempts += 1,
            }
        }
        for delivery in commit.deliveries.iter() {
            match &delivery.outcome {
                ToolOutcome::Succeeded => report.successful_tools += 1,
                ToolOutcome::Failed(_) => report.failed_tools += 1,
                ToolOutcome::Cancelled => report.cancelled_tools += 1,
                ToolOutcome::Interrupted => report.interrupted_tools += 1,
            }
        }
    }
    report.usage = usage.total.unwrap_or_default();
    Ok(report)
}

#[derive(Default)]
struct UsageAccumulator {
    total: Option<pl_core::model::ModelUsage>,
}
impl UsageAccumulator {
    fn add(&mut self, usage: &pl_core::model::ModelUsage) {
        let Some(total) = &mut self.total else {
            self.total = Some(usage.clone());
            return;
        };
        total.input_tokens = sum_known(total.input_tokens, usage.input_tokens);
        total.cache_read_tokens = sum_known(total.cache_read_tokens, usage.cache_read_tokens);
        total.cache_write_tokens = sum_known(total.cache_write_tokens, usage.cache_write_tokens);
        total.output_tokens = sum_known(total.output_tokens, usage.output_tokens);
        total.reasoning_tokens = sum_known(total.reasoning_tokens, usage.reasoning_tokens);
    }
}
fn sum_known(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    left.zip(right)
        .and_then(|(left, right)| left.checked_add(right))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_usage_preserves_unknowns_and_does_not_add_cache_subsets_to_input() {
        let mut usage = UsageAccumulator::default();
        usage.add(&pl_core::model::ModelUsage {
            input_tokens: Some(100),
            cache_read_tokens: Some(80),
            cache_write_tokens: None,
            output_tokens: Some(20),
            reasoning_tokens: Some(5),
        });
        usage.add(&pl_core::model::ModelUsage {
            input_tokens: Some(50),
            cache_read_tokens: Some(30),
            cache_write_tokens: Some(10),
            output_tokens: Some(10),
            reasoning_tokens: None,
        });
        let total = usage.total.unwrap();
        assert_eq!(total.input_tokens, Some(150));
        assert_eq!(total.cache_read_tokens, Some(110));
        assert_eq!(total.output_tokens, Some(30));
        assert_eq!(total.cache_write_tokens, None);
        assert_eq!(total.reasoning_tokens, None);
        assert_eq!(sum_known(Some(u64::MAX), Some(1)), None);
    }
}
