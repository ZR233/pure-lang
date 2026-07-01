use super::{AgentRecord, AgentStatus, AgentStatusUpdate};

pub(super) fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub(super) fn apply_status_update(record: &mut AgentRecord, update: AgentStatusUpdate) {
    record.status = update.status;
    if clears_status_details(update.status) {
        clear_status_details(record);
    }
    if update.summary.is_some() {
        record.summary = update.summary;
    }
    if update.error.is_some() {
        record.error = update.error;
    }
    if update.reason.is_some() {
        record.reason = update.reason;
    }
    if update.budget_limit_kind.is_some() {
        record.budget_limit_kind = update.budget_limit_kind;
    }
    if update.budget_usage.is_some() {
        record.budget_usage = update.budget_usage;
    }
    record.updated_at = unix_seconds();
}

pub(super) fn clear_for_reactivation(record: &mut AgentRecord) {
    clear_status_details(record);
    record.updated_at = unix_seconds();
}

fn clears_status_details(status: AgentStatus) -> bool {
    matches!(
        status,
        AgentStatus::Queued | AgentStatus::Running | AgentStatus::Completed
    )
}

fn clear_status_details(record: &mut AgentRecord) {
    record.error = None;
    record.reason = None;
    record.budget_limit_kind = None;
    record.budget_usage = None;
}
