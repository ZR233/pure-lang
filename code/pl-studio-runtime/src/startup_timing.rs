//! Bounded startup timings contain stage names only, never paths or configuration payloads.
use std::time::Instant;
pub(crate) struct Stage {
    name: &'static str,
    started: Instant,
}
impl Stage {
    pub(crate) fn new(name: &'static str) -> Self {
        tracing::info!(startup_stage = name, "startup stage started");
        Self {
            name,
            started: Instant::now(),
        }
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        tracing::info!(
            startup_stage = self.name,
            elapsed_ms = self.started.elapsed().as_millis() as u64,
            "startup stage ended"
        );
    }
}
