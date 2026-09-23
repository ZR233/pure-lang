use std::time::Duration;

use serde_json::Value;

use crate::runtime::{LspResult, LspRuntimeError};

pub(crate) fn is_content_modified_error(error: &LspRuntimeError) -> bool {
    matches!(error, LspRuntimeError::Server { code: -32801, .. })
}

pub(crate) async fn with_content_modified_retries<F, Fut>(mut operation: F) -> LspResult<Value>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = LspResult<Value>>,
{
    let mut delay = Duration::from_millis(500);
    for attempt in 0..=3 {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(error) if is_content_modified_error(&error) && attempt < 3 => {
                tokio::time::sleep(delay).await;
                delay *= 2;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("retry loop always returns")
}
