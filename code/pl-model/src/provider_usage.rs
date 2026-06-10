use std::collections::BTreeMap;
use std::time::Duration;

use pl_protocol::{PureError, Result};
use reqwest::header::{ACCEPT_LANGUAGE, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::Value;

use crate::provider_info::ProviderInfo;

const ZHIPU_TOKEN_LIMIT_TYPE: &str = "TOKENS_LIMIT";
const ZHIPU_TIME_LIMIT_TYPE: &str = "TIME_LIMIT";

#[derive(Debug, Clone, PartialEq)]
pub struct DeepSeekBalanceUsage {
    pub is_available: bool,
    pub balances: Vec<DeepSeekBalanceInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeepSeekBalanceInfo {
    pub currency: String,
    pub total_balance: String,
    pub granted_balance: String,
    pub topped_up_balance: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ZhipuCodingPlanUsage {
    pub level: Option<String>,
    pub limits: Vec<ZhipuQuotaLimit>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ZhipuQuotaLimit {
    pub window: ZhipuQuotaWindow,
    pub percentage: f64,
    pub current_value: Option<f64>,
    pub total: Option<f64>,
    pub remaining: Option<f64>,
    pub next_reset_at: Option<i64>,
    pub usage_details: Vec<ZhipuToolUsageDetail>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZhipuQuotaWindow {
    FiveHour,
    Weekly,
    McpMonthly,
    Other(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ZhipuToolUsageDetail {
    pub name: String,
    pub current_value: Option<f64>,
    pub total: Option<f64>,
    pub percentage: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct DeepSeekBalanceResponse {
    is_available: bool,
    balance_infos: Vec<DeepSeekBalanceInfoResponse>,
}

#[derive(Debug, Deserialize)]
struct DeepSeekBalanceInfoResponse {
    currency: String,
    total_balance: String,
    granted_balance: String,
    topped_up_balance: String,
}

#[derive(Debug, Deserialize)]
struct ZhipuEnvelope {
    #[serde(default)]
    code: Option<i64>,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    data: Option<ZhipuQuotaData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ZhipuQuotaData {
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    limits: Vec<ZhipuQuotaLimitResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ZhipuQuotaLimitResponse {
    #[serde(rename = "type")]
    limit_type: String,
    #[serde(default)]
    unit: Option<i64>,
    #[serde(default)]
    number: Option<i64>,
    #[serde(default)]
    percentage: Option<f64>,
    #[serde(default)]
    current_value: Option<f64>,
    #[serde(default)]
    usage: Option<f64>,
    #[serde(default)]
    next_reset_time: Option<i64>,
    #[serde(default)]
    usage_details: Option<Value>,
}

pub async fn query_deepseek_balance(info: &ProviderInfo) -> Result<DeepSeekBalanceUsage> {
    let token = required_token(info)?;
    let url = endpoint_url(&info.base_url, "/user/balance")?;
    let client = reqwest_client()?;
    let response = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(http_error)?;
    let status = response.status();
    let body = response.text().await.map_err(http_error)?;
    if !status.is_success() {
        return Err(PureError::HttpError(format!(
            "DeepSeek balance request failed: HTTP {status}"
        )));
    }
    parse_deepseek_balance(&body)
}

pub async fn query_zhipu_coding_plan_usage(info: &ProviderInfo) -> Result<ZhipuCodingPlanUsage> {
    let token = required_token(info)?;
    let url = endpoint_url(&info.base_url, "/api/monitor/usage/quota/limit")?;
    let client = reqwest_client()?;
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&token).map_err(|error| PureError::HttpError(error.to_string()))?,
    );
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    let response = client
        .get(url)
        .headers(headers)
        .send()
        .await
        .map_err(http_error)?;
    let status = response.status();
    let body = response.text().await.map_err(http_error)?;
    if !status.is_success() {
        return Err(PureError::HttpError(format!(
            "Zhipu Coding Plan quota request failed: HTTP {status}"
        )));
    }
    parse_zhipu_coding_plan_usage(&body)
}

pub fn parse_deepseek_balance(body: &str) -> Result<DeepSeekBalanceUsage> {
    let response: DeepSeekBalanceResponse = serde_json::from_str(body)?;
    Ok(DeepSeekBalanceUsage {
        is_available: response.is_available,
        balances: response
            .balance_infos
            .into_iter()
            .map(|balance| DeepSeekBalanceInfo {
                currency: balance.currency,
                total_balance: balance.total_balance,
                granted_balance: balance.granted_balance,
                topped_up_balance: balance.topped_up_balance,
            })
            .collect(),
    })
}

pub fn parse_zhipu_coding_plan_usage(body: &str) -> Result<ZhipuCodingPlanUsage> {
    let envelope: ZhipuEnvelope = serde_json::from_str(body)?;
    if matches!(envelope.success, Some(false)) {
        let message = envelope
            .msg
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                envelope
                    .code
                    .map(|code| format!("provider returned code {code}"))
                    .unwrap_or_else(|| "provider returned unsuccessful response".to_string())
            });
        return Err(PureError::HttpError(format!(
            "Zhipu Coding Plan quota request failed: {message}"
        )));
    }
    let data = envelope.data.ok_or_else(|| {
        PureError::HttpError("Zhipu Coding Plan quota response missing data".into())
    })?;
    Ok(ZhipuCodingPlanUsage {
        level: data.level,
        limits: data
            .limits
            .into_iter()
            .map(zhipu_quota_limit)
            .collect::<Vec<_>>(),
    })
}

fn zhipu_quota_limit(limit: ZhipuQuotaLimitResponse) -> ZhipuQuotaLimit {
    let total = limit.usage;
    let current_value = limit.current_value;
    let remaining = match (total, current_value) {
        (Some(total), Some(current)) => Some((total - current).max(0.0)),
        _ => None,
    };
    ZhipuQuotaLimit {
        window: zhipu_quota_window(&limit),
        percentage: limit.percentage.unwrap_or(0.0),
        current_value,
        total,
        remaining,
        next_reset_at: limit.next_reset_time.map(|value| value / 1_000),
        usage_details: limit
            .usage_details
            .as_ref()
            .map(zhipu_tool_usage_details)
            .unwrap_or_default(),
    }
}

fn zhipu_quota_window(limit: &ZhipuQuotaLimitResponse) -> ZhipuQuotaWindow {
    match (limit.limit_type.as_str(), limit.unit, limit.number) {
        (ZHIPU_TOKEN_LIMIT_TYPE, Some(3), Some(5)) => ZhipuQuotaWindow::FiveHour,
        (ZHIPU_TOKEN_LIMIT_TYPE, Some(6), Some(1)) => ZhipuQuotaWindow::Weekly,
        (ZHIPU_TIME_LIMIT_TYPE, _, _) => ZhipuQuotaWindow::McpMonthly,
        _ => ZhipuQuotaWindow::Other(limit.limit_type.clone()),
    }
}

fn zhipu_tool_usage_details(value: &Value) -> Vec<ZhipuToolUsageDetail> {
    match value {
        Value::Array(items) => items.iter().filter_map(zhipu_tool_usage_detail).collect(),
        Value::Object(map) => map
            .iter()
            .map(|(name, value)| {
                zhipu_tool_usage_detail(value).unwrap_or_else(|| ZhipuToolUsageDetail {
                    name: humanize_tool_name(name),
                    current_value: value.as_f64(),
                    total: None,
                    percentage: None,
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn zhipu_tool_usage_detail(value: &Value) -> Option<ZhipuToolUsageDetail> {
    let object = value.as_object()?;
    let name = first_string(
        object,
        &[
            "name",
            "tool",
            "toolName",
            "type",
            "id",
            "service",
            "serviceName",
        ],
    )?;
    Some(ZhipuToolUsageDetail {
        name: humanize_tool_name(&name),
        current_value: first_number(object, &["currentValue", "current", "count", "used"]),
        total: first_number(object, &["total", "limit", "quota", "usage"]),
        percentage: first_number(object, &["percentage", "percent"]),
    })
}

fn first_string(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        map.get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn first_number(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| map.get(*key).and_then(number_value))
}

fn number_value(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
}

fn humanize_tool_name(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("web_search_prime")
        || trimmed.eq_ignore_ascii_case("web_search")
    {
        "Web Search".to_string()
    } else if trimmed.eq_ignore_ascii_case("web_reader") {
        "Web Reader".to_string()
    } else if trimmed.eq_ignore_ascii_case("zread") {
        "ZRead".to_string()
    } else {
        trimmed
            .replace(['_', '-'], " ")
            .split_whitespace()
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => {
                        let mut word = first.to_uppercase().collect::<String>();
                        word.push_str(chars.as_str());
                        word
                    }
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn required_token(info: &ProviderInfo) -> Result<String> {
    info.bearer_token
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| PureError::ConfigError("provider API key is not configured".to_string()))
}

fn endpoint_url(base_url: &str, path: &str) -> Result<String> {
    let mut url = reqwest::Url::parse(base_url.trim())
        .map_err(|error| PureError::ConfigError(format!("invalid provider base_url: {error}")))?;
    url.set_path(path);
    url.set_query(None);
    Ok(url.to_string())
}

fn reqwest_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(http_error)
}

fn http_error(error: reqwest::Error) -> PureError {
    PureError::HttpError(error.to_string())
}

pub fn zhipu_limit_by_window(
    limits: &[ZhipuQuotaLimit],
) -> BTreeMap<&'static str, &ZhipuQuotaLimit> {
    limits
        .iter()
        .filter_map(|limit| {
            let key = match limit.window {
                ZhipuQuotaWindow::FiveHour => "five_hour",
                ZhipuQuotaWindow::Weekly => "weekly",
                ZhipuQuotaWindow::McpMonthly => "mcp_monthly",
                ZhipuQuotaWindow::Other(_) => return None,
            };
            Some((key, limit))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::oneshot;

    use super::*;

    #[test]
    fn parses_deepseek_balance_response() {
        let usage = parse_deepseek_balance(
            r#"{
                "is_available": true,
                "balance_infos": [
                    {
                        "currency": "CNY",
                        "total_balance": "110.00",
                        "granted_balance": "10.00",
                        "topped_up_balance": "100.00"
                    }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(
            usage,
            DeepSeekBalanceUsage {
                is_available: true,
                balances: vec![DeepSeekBalanceInfo {
                    currency: "CNY".to_string(),
                    total_balance: "110.00".to_string(),
                    granted_balance: "10.00".to_string(),
                    topped_up_balance: "100.00".to_string(),
                }],
            }
        );
    }

    #[test]
    fn parses_zhipu_quota_windows_and_reset_time() {
        let usage = parse_zhipu_coding_plan_usage(
            r#"{
                "code": 200,
                "success": true,
                "data": {
                    "level": "pro",
                    "limits": [
                        {
                            "type": "TOKENS_LIMIT",
                            "unit": 3,
                            "number": 5,
                            "percentage": 40.5,
                            "currentValue": 10,
                            "usage": 100,
                            "nextResetTime": 1760000000123
                        },
                        {
                            "type": "TOKENS_LIMIT",
                            "unit": 6,
                            "number": 1,
                            "percentage": 52
                        },
                        {
                            "type": "TIME_LIMIT",
                            "percentage": 12.3,
                            "currentValue": 123,
                            "usage": 1000,
                            "usageDetails": [
                                {"toolName": "web_search_prime", "currentValue": 7, "usage": 100},
                                {"type": "web_reader", "count": "3"}
                            ]
                        }
                    ]
                }
            }"#,
        )
        .unwrap();

        assert_eq!(usage.level.as_deref(), Some("pro"));
        assert_eq!(usage.limits[0].window, ZhipuQuotaWindow::FiveHour);
        assert_eq!(usage.limits[0].remaining, Some(90.0));
        assert_eq!(usage.limits[0].next_reset_at, Some(1_760_000_000));
        assert_eq!(usage.limits[1].window, ZhipuQuotaWindow::Weekly);
        assert_eq!(usage.limits[2].window, ZhipuQuotaWindow::McpMonthly);
        assert_eq!(
            usage.limits[2].usage_details,
            vec![
                ZhipuToolUsageDetail {
                    name: "Web Search".to_string(),
                    current_value: Some(7.0),
                    total: Some(100.0),
                    percentage: None,
                },
                ZhipuToolUsageDetail {
                    name: "Web Reader".to_string(),
                    current_value: Some(3.0),
                    total: None,
                    percentage: None,
                },
            ]
        );
    }

    #[test]
    fn treats_zhipu_business_failure_as_error() {
        let error =
            parse_zhipu_coding_plan_usage(r#"{"code":1001,"success":false,"msg":"invalid token"}"#)
                .unwrap_err();

        assert!(error.to_string().contains("invalid token"));
    }

    #[tokio::test]
    async fn deepseek_balance_requires_token() {
        let info = ProviderInfo::deepseek(None);

        let error = query_deepseek_balance(&info).await.unwrap_err();

        assert!(error.to_string().contains("API key"));
    }

    #[tokio::test]
    async fn deepseek_balance_http_401_is_error() {
        let (base_url, _request_rx) = serve_once(http_response(
            "401 Unauthorized",
            r#"{"error":"unauthorized"}"#,
        ))
        .await;
        let mut info = ProviderInfo::deepseek(Some(base_url));
        info.bearer_token = Some("secret".to_string());

        let error = query_deepseek_balance(&info).await.unwrap_err();

        assert!(error.to_string().contains("HTTP 401"));
    }

    #[tokio::test]
    async fn zhipu_quota_uses_raw_authorization_token() {
        let (base_url, request_rx) = serve_once(http_response(
            "200 OK",
            r#"{"success":true,"data":{"level":"pro","limits":[]}}"#,
        ))
        .await;
        let mut info = ProviderInfo::zhipu_coding_plan(Some(base_url));
        info.bearer_token = Some("raw-token".to_string());

        query_zhipu_coding_plan_usage(&info).await.unwrap();
        let request = request_rx.await.unwrap().to_ascii_lowercase();

        assert!(request.contains("get /api/monitor/usage/quota/limit "));
        assert!(request.contains("authorization: raw-token"));
        assert!(!request.contains("authorization: bearer raw-token"));
    }

    async fn serve_once(response: String) -> (String, oneshot::Receiver<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (request_tx, request_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0_u8; 4096];
            let read = socket.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            let _ = request_tx.send(request);
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        });
        (format!("http://{address}/api/coding/paas/v4"), request_rx)
    }

    fn http_response(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }
}
