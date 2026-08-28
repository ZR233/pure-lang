//! 验收专用的最终 provider wire 请求捕获。
//!
//! 只有测试/xtask harness 显式设置 `PURE_STUDIO_WIRE_CAPTURE_DIR` 时写盘；
//! 默认生产路径不记录 prompt，也从不接触认证头。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use pl_protocol::{PureError, Result};
use serde::Serialize;
use serde_json::{Map, Value};

use super::openai::OpenAiRequestBody;

const CAPTURE_DIRECTORY_ENV: &str = "PURE_STUDIO_WIRE_CAPTURE_DIR";
static CAPTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireCapture<'a> {
    schema_version: u32,
    protocol: &'a str,
    request_mode: &'a str,
    wire_body: &'a Value,
}

pub(super) async fn capture_http(body: &OpenAiRequestBody) -> Result<()> {
    let (protocol, wire_body) = match body {
        OpenAiRequestBody::Responses(body) => ("responsesHttp", Value::Object(body.clone())),
        OpenAiRequestBody::Chat(body) => ("chatCompletions", Value::Object(body.clone())),
    };
    capture(protocol, "full", &wire_body).await
}

pub(super) async fn capture_responses_websocket(
    request_mode: &'static str,
    wire_body: &Map<String, Value>,
) -> Result<()> {
    capture(
        "responsesWebSocket",
        request_mode,
        &Value::Object(wire_body.clone()),
    )
    .await
}

async fn capture(protocol: &str, request_mode: &str, wire_body: &Value) -> Result<()> {
    let Some(directory) = std::env::var_os(CAPTURE_DIRECTORY_ENV).map(PathBuf::from) else {
        return Ok(());
    };
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|error| {
            PureError::ConfigError(format!(
                "failed to create wire capture directory `{}`: {error}",
                directory.display()
            ))
        })?;
    let sequence = CAPTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    let path = directory.join(format!("{sequence:06}-{protocol}-{request_mode}.json"));
    let payload = serde_json::to_vec_pretty(&WireCapture {
        schema_version: 1,
        protocol,
        request_mode,
        wire_body,
    })
    .map_err(|error| PureError::ConfigError(format!("failed to encode wire capture: {error}")))?;
    tokio::fs::write(&path, payload).await.map_err(|error| {
        PureError::ConfigError(format!(
            "failed to write wire capture `{}`: {error}",
            path.display()
        ))
    })
}
