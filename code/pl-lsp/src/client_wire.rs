use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;
use tokio::sync::{Mutex, oneshot};

use crate::framing::encode_message;
use crate::types::{LspResult, LspRuntimeError};

type PendingResponseSender = oneshot::Sender<LspResult<Value>>;
pub(crate) type PendingRequests = Arc<Mutex<HashMap<i64, PendingResponseSender>>>;

pub(crate) async fn request_raw(
    stdin: &Arc<Mutex<Option<ChildStdin>>>,
    pending: &PendingRequests,
    next_id: &AtomicI64,
    method: &str,
    params: Value,
    timeout: Duration,
) -> LspResult<Value> {
    let id = next_id.fetch_add(1, Ordering::Relaxed);
    let (sender, receiver) = oneshot::channel();
    pending.lock().await.insert(id, sender);
    let write = write_message(
        stdin,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }),
    )
    .await;
    if let Err(error) = write {
        pending.lock().await.remove(&id);
        return Err(error);
    }
    match tokio::time::timeout(timeout, receiver).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(LspRuntimeError::Unavailable(format!(
            "LSP request channel closed for {method}"
        ))),
        Err(_) => {
            pending.lock().await.remove(&id);
            Err(LspRuntimeError::Timeout(method.to_string()))
        }
    }
}

pub(crate) async fn notify_raw(
    stdin: &Arc<Mutex<Option<ChildStdin>>>,
    method: &str,
    params: Value,
) -> LspResult<()> {
    write_message(
        stdin,
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }),
    )
    .await
}

pub(crate) async fn write_message(
    stdin: &Arc<Mutex<Option<ChildStdin>>>,
    message: Value,
) -> LspResult<()> {
    let bytes = encode_message(&message)?;
    let mut guard = stdin.lock().await;
    let stdin = guard
        .as_mut()
        .ok_or_else(|| LspRuntimeError::Unavailable("LSP stdin unavailable".to_string()))?;
    stdin.write_all(&bytes).await?;
    stdin.flush().await?;
    Ok(())
}

pub(crate) fn response_id(value: &Value) -> Option<i64> {
    value
        .get("id")
        .and_then(|id| id.as_i64().or_else(|| id.as_u64().map(|id| id as i64)))
}

pub(crate) fn response_result(value: Value) -> LspResult<Value> {
    if let Some(error) = value.get("error") {
        let code = error
            .get("code")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("LSP request failed")
            .to_string();
        return Err(LspRuntimeError::Server { code, message });
    }
    Ok(value.get("result").cloned().unwrap_or(Value::Null))
}

pub(crate) async fn fail_pending(pending: &PendingRequests, error: LspRuntimeError) {
    let mut pending = pending.lock().await;
    for (_, sender) in pending.drain() {
        let _ = sender.send(Err(LspRuntimeError::Unavailable(error.to_string())));
    }
}
