use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static AGENT_ID_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn new_agent_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let sequence = AGENT_ID_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("agent-{timestamp}-{sequence}")
}
