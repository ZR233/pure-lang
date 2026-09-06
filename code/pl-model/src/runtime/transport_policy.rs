use std::time::Duration;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

pub(crate) const RESPONSES_WEBSOCKET_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const RESPONSES_WEBSOCKET_SEND_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const RESPONSES_WEBSOCKET_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
pub(crate) const RESPONSES_WEBSOCKET_MAX_RETRIES: u32 = 1;
pub(crate) const MODEL_MAX_RETRIES: u32 = 5;
pub(crate) const RESPONSES_WEBSOCKET_PROFILE_REVISION: &str = "responses_websockets=2026-02-06;pl-ws-v2;happy-eyeballs=250ms;connect=15s;send=15s;idle=300s;ws-retries=1;total-retries=5;replay-budget=shared;fallback=http-session";

pub(crate) fn model_request_retry_delay(
    retry_number: u32,
    requested_delay_ms: Option<u64>,
    jitter_key: &str,
) -> Duration {
    let exponential_ms = 200_u64.saturating_mul(1_u64 << retry_number.saturating_sub(1).min(4));
    let delay_ms = requested_delay_ms.unwrap_or_else(|| {
        let mut hasher = DefaultHasher::new();
        jitter_key.hash(&mut hasher);
        retry_number.hash(&mut hasher);
        let jitter_percent = 90 + hasher.finish() % 21;
        exponential_ms.saturating_mul(jitter_percent) / 100
    });
    Duration::from_millis(delay_ms.min(30_000))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn model_request_retry_backoff_is_bounded() {
        let first = model_request_retry_delay(1, None, "request-a");
        assert!((Duration::from_millis(180)..=Duration::from_millis(220)).contains(&first));
        let fifth = model_request_retry_delay(5, None, "request-a");
        assert!((Duration::from_millis(2_880)..=Duration::from_millis(3_520)).contains(&fifth));
        assert_eq!(
            model_request_retry_delay(2, Some(90_000), "request-a"),
            Duration::from_secs(30)
        );
        assert_eq!(
            first,
            model_request_retry_delay(1, None, "request-a"),
            "the same inference must retain a stable retry schedule"
        );
        let distinct_delays = (0..16)
            .map(|index| model_request_retry_delay(1, None, &format!("request-{index}")))
            .collect::<std::collections::HashSet<_>>();
        assert!(distinct_delays.len() > 1);
    }
}
