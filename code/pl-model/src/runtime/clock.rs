//! Wall time is injected independently of the monotonic inference timing clock.
use pl_protocol::{PureError, Result};

/// Supplies request timestamps for tariff selection, including deterministic simulation.
pub trait InferenceClock: std::fmt::Debug + Send + Sync {
    /// Returns Unix seconds at the start of a physical request attempt.
    /// # Errors
    /// Returns an error if a valid timestamp cannot be obtained.
    fn unix_seconds(&self) -> Result<i64>;
}

#[derive(Debug)]
pub(super) struct SystemInferenceClock;

impl InferenceClock for SystemInferenceClock {
    fn unix_seconds(&self) -> Result<i64> {
        let duration = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| {
                PureError::ConfigError(format!("cannot timestamp model request: {error}"))
            })?;
        i64::try_from(duration.as_secs()).map_err(|error| {
            PureError::ConfigError(format!("model request timestamp overflows: {error}"))
        })
    }
}
