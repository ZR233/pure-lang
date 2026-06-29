use std::sync::Arc;

use lsp_types::{ProgressParams, ProgressToken, WorkDoneProgressCreateParams};
use serde_json::Value;
use tokio::process::ChildStdin;
use tokio::sync::{Mutex, broadcast};

use crate::client_config::workspace_configuration_response;
use crate::client_wire::write_message;
use crate::status::LspClientStatus;
use crate::types::LspResult;

pub(crate) async fn respond_to_server_request(
    stdin: &Arc<Mutex<Option<ChildStdin>>>,
    id: i64,
    method: &str,
    params: Option<&Value>,
    server_id: &str,
    status: &Arc<Mutex<LspClientStatus>>,
    updates: &broadcast::Sender<()>,
) -> LspResult<()> {
    let result = match method {
        "workspace/configuration" => workspace_configuration_response(params, server_id),
        "window/workDoneProgress/create" => {
            if let Some(params) = params
                && let Ok(params) =
                    serde_json::from_value::<WorkDoneProgressCreateParams>(params.clone())
                && register_progress_token_status(status, params.token).await
            {
                let _ = updates.send(());
            }
            Value::Null
        }
        "client/registerCapability" | "client/unregisterCapability" => Value::Null,
        _ => Value::Null,
    };
    write_message(
        stdin,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }),
    )
    .await
}

pub(crate) async fn apply_progress_status(
    status: &Arc<Mutex<LspClientStatus>>,
    params: ProgressParams,
) -> bool {
    status.lock().await.apply_progress(params)
}

pub(crate) async fn clear_progress_status(status: &Arc<Mutex<LspClientStatus>>) -> bool {
    status.lock().await.clear_progress()
}

pub(crate) async fn record_last_error_status(
    status: &Arc<Mutex<LspClientStatus>>,
    message: String,
) -> bool {
    status.lock().await.record_error(message)
}

async fn register_progress_token_status(
    status: &Arc<Mutex<LspClientStatus>>,
    token: ProgressToken,
) -> bool {
    status.lock().await.register_progress_token(token)
}
