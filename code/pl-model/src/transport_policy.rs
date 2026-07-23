use std::time::Duration;

pub(crate) const RESPONSES_WEBSOCKET_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const RESPONSES_WEBSOCKET_SEND_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const RESPONSES_WEBSOCKET_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
pub(crate) const RESPONSES_WEBSOCKET_MAX_RETRIES: u32 = 5;
pub(crate) const RESPONSES_WEBSOCKET_PROFILE_REVISION: &str = "responses_websockets=2026-02-06;pl-ws-v2;happy-eyeballs=250ms;connect=15s;send=15s;idle=300s;retries=5";

pub(crate) fn responses_websocket_retry_delay(
    retry_number: u32,
    requested_delay_ms: Option<u64>,
) -> Duration {
    let exponential_ms = 200_u64.saturating_mul(1_u64 << retry_number.saturating_sub(1).min(4));
    Duration::from_millis(requested_delay_ms.unwrap_or(exponential_ms).min(30_000))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn websocket_retry_backoff_is_bounded() {
        assert_eq!(
            responses_websocket_retry_delay(1, None),
            Duration::from_millis(200)
        );
        assert_eq!(
            responses_websocket_retry_delay(5, None),
            Duration::from_millis(3_200)
        );
        assert_eq!(
            responses_websocket_retry_delay(2, Some(90_000)),
            Duration::from_secs(30)
        );
    }
}
