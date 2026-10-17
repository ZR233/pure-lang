//! Startup-cached OpenAI-compatible `/models` inventories for catalog acceptance.
//!
//! The control file is read once when the fixture process starts. Changing the
//! file never affects a running listener; operators rewrite it and restart.

use std::sync::{Arc, Mutex};
use std::{fs, net::SocketAddr, path::Path, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use super::AppState;

/// Default control-file name written by the manual-GUI coordinator.
pub const CONTROL_FILE_NAME: &str = "model-catalog.json";

/// MiMo TTS/ASR slugs the product catalog excludes after a successful decode.
pub const MIMO_DISCOVERY_EXCLUDED_SLUGS: [&str; 4] = [
    "mimo-v2.5-asr",
    "mimo-v2.5-tts",
    "mimo-v2.5-tts-voiceclone",
    "mimo-v2.5-tts-voicedesign",
];

/// Path suffixes served under the fixture `/v1` prefix.
pub const OPENAI_PATH: &str = "/v1/openai/models";
pub const OPENAI_SECOND_PATH: &str = "/v1/openai-second/models";
pub const DEEPSEEK_PATH: &str = "/v1/deepseek/models";
pub const MIMO_PATH: &str = "/v1/mimo/models";

/// Loopback suffixes used by discovery instances. Inference is served at
/// `/{suffix}/responses` and `/{suffix}/chat/completions` by stripping the
/// suffix and forwarding to the existing strict-script handler.
pub const PROVIDER_SUFFIXES: [&str; 4] = ["openai", "openai-second", "deepseek", "mimo"];

/// One catalog GET recorded separately from the strict conversation script.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogHit {
    pub provider: String,
    pub path: String,
    pub status: u16,
    pub rejected: bool,
}

/// Per-provider catalog GET counters written to evidence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogProviderHits {
    pub hits: usize,
    pub rejected: usize,
    pub last_status: Option<u16>,
}

/// Structured catalog evidence; never mixed into `requests.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogRequestReport {
    pub openai: ModelCatalogProviderHits,
    #[serde(rename = "openai-second")]
    pub openai_second: ModelCatalogProviderHits,
    pub deepseek: ModelCatalogProviderHits,
    pub mimo: ModelCatalogProviderHits,
    pub requests: Vec<ModelCatalogHit>,
}

#[derive(Clone, Default)]
pub(super) struct ModelCatalogHits {
    inner: Arc<Mutex<ModelCatalogRequestReport>>,
}

impl ModelCatalogHits {
    fn record(&self, provider: CatalogProvider, path: &str, status: u16, rejected: bool) {
        let mut report = self.inner.lock().expect("fixture state poisoned");
        let bucket = match provider {
            CatalogProvider::Openai => &mut report.openai,
            CatalogProvider::OpenaiSecond => &mut report.openai_second,
            CatalogProvider::DeepSeek => &mut report.deepseek,
            CatalogProvider::Mimo => &mut report.mimo,
        };
        bucket.hits += 1;
        if rejected {
            bucket.rejected += 1;
        }
        bucket.last_status = Some(status);
        report.requests.push(ModelCatalogHit {
            provider: provider.instance().to_owned(),
            path: path.to_owned(),
            status,
            rejected,
        });
    }

    pub(super) fn snapshot(&self) -> ModelCatalogRequestReport {
        self.inner.lock().expect("fixture state poisoned").clone()
    }
}

/// One provider's startup inventory and failure switch.
///
/// Failure switches are exclusive in evaluation order: `hang` wins, then
/// `protocolError`, then explicit `status` or `reject`. Later flags are not
/// applied once an earlier one matches; operators must not rely on silent
/// combination.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelCatalogProviderControl {
    /// Exact slugs returned in the OpenAI-compatible list. An empty list is a
    /// successful replacement, not a transport failure.
    #[serde(default)]
    pub models: Vec<String>,
    /// When true, answer with HTTP 401 unless [`status`](Self::status) is set.
    #[serde(default)]
    pub reject: bool,
    /// Optional explicit HTTP status. Non-2xx values fail discovery as HTTP
    /// errors; 2xx still returns the model list unless [`protocol_error`] is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// Return HTTP 200 with an error envelope so decode fails as protocol error.
    #[serde(default)]
    pub protocol_error: bool,
    /// Hold the request until the fixture stops so the 10s discovery deadline
    /// expires. Used for failure isolation, not hot reload.
    #[serde(default)]
    pub hang: bool,
}

/// Control file covering the four loopback discovery instances.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelCatalogControl {
    #[serde(default)]
    pub openai: ModelCatalogProviderControl,
    #[serde(rename = "openai-second", default)]
    pub openai_second: ModelCatalogProviderControl,
    #[serde(default)]
    pub deepseek: ModelCatalogProviderControl,
    #[serde(default)]
    pub mimo: ModelCatalogProviderControl,
}

#[derive(Clone, Copy)]
enum CatalogProvider {
    Openai,
    OpenaiSecond,
    DeepSeek,
    Mimo,
}

impl CatalogProvider {
    fn path(self) -> &'static str {
        match self {
            Self::Openai => OPENAI_PATH,
            Self::OpenaiSecond => OPENAI_SECOND_PATH,
            Self::DeepSeek => DEEPSEEK_PATH,
            Self::Mimo => MIMO_PATH,
        }
    }

    fn owned_by(self) -> &'static str {
        match self {
            Self::Openai | Self::OpenaiSecond => "openai",
            Self::DeepSeek => "deepseek",
            Self::Mimo => "mimo",
        }
    }

    fn instance(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::OpenaiSecond => "openai-second",
            Self::DeepSeek => "deepseek",
            Self::Mimo => "mimo",
        }
    }
}

impl ModelCatalogControl {
    /// Reads and caches the control file. The caller must not reread later.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read model catalog control {}", path.display()))?;
        let control: Self = serde_json::from_str(&text).with_context(|| {
            format!("model catalog control {} is not valid JSON", path.display())
        })?;
        control.validate()?;
        Ok(control)
    }

    /// Writes the control file so an operator or coordinator can replace it
    /// before the next process start.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        self.validate()?;
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, format!("{}\n", serde_json::to_string_pretty(self)?))
            .with_context(|| format!("failed to write model catalog control {}", path.display()))
    }

    /// Inventory A: known bundled slugs, a withdrawable `fixture-model-old`,
    /// unknown `mimo-v2.6-pro`, and MiMo's four excluded TTS/ASR slugs.
    pub fn inventory_a() -> Self {
        let openai = ModelCatalogProviderControl {
            models: vec![
                "gpt-6-astra".into(),
                "gpt-6.1-sol".into(),
                "fixture-model-old".into(),
            ],
            ..ModelCatalogProviderControl::default()
        };
        Self {
            openai,
            openai_second: ModelCatalogProviderControl {
                models: vec![
                    "gpt-6-sol".into(),
                    "gpt-6.1-sol".into(),
                    "fixture-model-old".into(),
                ],
                ..ModelCatalogProviderControl::default()
            },
            deepseek: ModelCatalogProviderControl {
                models: vec!["deepseek-flash".into(), "fixture-model-old".into()],
                ..ModelCatalogProviderControl::default()
            },
            mimo: ModelCatalogProviderControl {
                models: mimo_inventory(true),
                ..ModelCatalogProviderControl::default()
            },
        }
    }

    /// Inventory B: same as A with `fixture-model-old` withdrawn.
    pub fn inventory_b() -> Self {
        let openai = ModelCatalogProviderControl {
            models: vec!["gpt-6-astra".into()],
            ..ModelCatalogProviderControl::default()
        };
        Self {
            openai,
            openai_second: ModelCatalogProviderControl {
                models: vec!["gpt-6-sol".into()],
                ..ModelCatalogProviderControl::default()
            },
            deepseek: ModelCatalogProviderControl {
                models: vec!["deepseek-flash".into()],
                ..ModelCatalogProviderControl::default()
            },
            mimo: ModelCatalogProviderControl {
                models: mimo_inventory(false),
                ..ModelCatalogProviderControl::default()
            },
        }
    }

    /// Empty successful lists for every instance.
    pub fn inventory_empty() -> Self {
        Self::default()
    }

    /// Every instance returns HTTP 401 so discovery falls back to bundled.
    pub fn inventory_reject_all() -> Self {
        let rejected = ModelCatalogProviderControl {
            reject: true,
            ..ModelCatalogProviderControl::default()
        };
        Self {
            openai: rejected.clone(),
            openai_second: rejected.clone(),
            deepseek: rejected.clone(),
            mimo: rejected,
        }
    }

    /// Only `catalog-openai-second` fails; the other instances stay on inventory A.
    pub fn inventory_reject_second() -> Self {
        let mut control = Self::inventory_a();
        control.openai_second = ModelCatalogProviderControl {
            models: Vec::new(),
            reject: true,
            ..ModelCatalogProviderControl::default()
        };
        control
    }

    /// Named presets the acceptance CLI can materialize into a control file.
    pub fn named(name: &str) -> Result<Self> {
        match name {
            "a" | "inventory-a" => Ok(Self::inventory_a()),
            "b" | "inventory-b" => Ok(Self::inventory_b()),
            "empty" => Ok(Self::inventory_empty()),
            "reject-all" => Ok(Self::inventory_reject_all()),
            "reject-second" => Ok(Self::inventory_reject_second()),
            other => bail!("unknown model catalog inventory: {other}"),
        }
    }

    fn validate(&self) -> Result<()> {
        for (name, provider) in self.providers() {
            if let Some(status) = provider.status {
                ensure!(
                    (100..600).contains(&status),
                    "{name} status {status} is not an HTTP status"
                );
            }
            for slug in &provider.models {
                ensure!(
                    !slug.trim().is_empty(),
                    "{name} contains an empty model slug"
                );
            }
        }
        Ok(())
    }

    fn providers(&self) -> [(&'static str, &ModelCatalogProviderControl); 4] {
        [
            ("openai", &self.openai),
            ("openai-second", &self.openai_second),
            ("deepseek", &self.deepseek),
            ("mimo", &self.mimo),
        ]
    }

    fn entry(&self, provider: CatalogProvider) -> &ModelCatalogProviderControl {
        match provider {
            CatalogProvider::Openai => &self.openai,
            CatalogProvider::OpenaiSecond => &self.openai_second,
            CatalogProvider::DeepSeek => &self.deepseek,
            CatalogProvider::Mimo => &self.mimo,
        }
    }
}

fn mimo_inventory(include_old: bool) -> Vec<String> {
    let mut models = vec![
        "mimo-v2.5-pro".into(),
        "mimo-v2.5".into(),
        "mimo-v2.6-pro".into(),
    ];
    if include_old {
        models.push("fixture-model-old".into());
    }
    models.extend(MIMO_DISCOVERY_EXCLUDED_SLUGS.map(str::to_owned));
    models
}

fn list_body(owned_by: &str, models: &[String]) -> Value {
    json!({
        "object": "list",
        "data": models.iter().map(|id| json!({
            "id": id,
            "object": "model",
            "owned_by": owned_by,
        })).collect::<Vec<_>>(),
    })
}

fn error_body(message: &str) -> Value {
    json!({
        "error": {
            "code": "fixture_catalog_rejected",
            "message": message,
        }
    })
}

async fn respond(state: AppState, provider: CatalogProvider) -> Response {
    let Some(control) = state.model_catalog.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let entry = control.entry(provider);
    if entry.hang {
        // Record before waiting so a coordinator can observe the hang without
        // racing shutdown; the request still blocks until the listener stops.
        state
            .model_catalog_hits
            .record(provider, provider.path(), 503, true);
        state.stopping.cancelled().await;
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if entry.protocol_error {
        state
            .model_catalog_hits
            .record(provider, provider.path(), 200, true);
        return Json(json!({
            "error": {
                "code": "invalid_request",
                "message": "fixture catalog protocol error",
            },
            "success": false,
        }))
        .into_response();
    }
    let status = entry.status.unwrap_or(if entry.reject { 401 } else { 200 });
    if !(200..300).contains(&status) {
        state
            .model_catalog_hits
            .record(provider, provider.path(), status, true);
        return (
            StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(error_body("fixture rejected the catalog request")),
        )
            .into_response();
    }
    state
        .model_catalog_hits
        .record(provider, provider.path(), status, false);
    Json(list_body(provider.owned_by(), &entry.models)).into_response()
}

pub(super) async fn openai_models(State(state): State<AppState>) -> Response {
    respond(state, CatalogProvider::Openai).await
}

pub(super) async fn openai_second_models(State(state): State<AppState>) -> Response {
    respond(state, CatalogProvider::OpenaiSecond).await
}

pub(super) async fn deepseek_models(State(state): State<AppState>) -> Response {
    respond(state, CatalogProvider::DeepSeek).await
}

pub(super) async fn mimo_models(State(state): State<AppState>) -> Response {
    respond(state, CatalogProvider::Mimo).await
}

/// Probes the four catalog endpoints of a listening fixture and prints one line
/// per path. Hanging providers are skipped so the check can finish.
pub async fn self_check(address: SocketAddr, control: &ModelCatalogControl) -> Result<()> {
    for (provider, entry) in [
        (CatalogProvider::Openai, &control.openai),
        (CatalogProvider::OpenaiSecond, &control.openai_second),
        (CatalogProvider::DeepSeek, &control.deepseek),
        (CatalogProvider::Mimo, &control.mimo),
    ] {
        if entry.hang {
            println!(
                "{} skipped=hang (probe continues until shutdown)",
                provider.path()
            );
            continue;
        }
        let (status, body) = get_path(address, provider.path()).await?;
        let preview: Value = serde_json::from_str(&body).unwrap_or(Value::String(body.clone()));
        println!(
            "{} status={status} body={}",
            provider.path(),
            serde_json::to_string(&preview)?
        );
    }
    Ok(())
}

async fn get_path(address: SocketAddr, path: &str) -> Result<(u16, String)> {
    let mut stream = tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(address))
        .await
        .context("catalog self-check connect timed out")?
        .with_context(|| format!("catalog self-check could not connect to {address}"))?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await?;
    let text = String::from_utf8_lossy(&bytes);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_ref(), ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .context("catalog self-check response had no HTTP status")?;
    Ok((status, body.trim().to_owned()))
}

/// Starts catalog GETs against hanging providers so shutdown can record them.
pub async fn kick_hang_probes(
    address: SocketAddr,
    control: &ModelCatalogControl,
) -> Vec<tokio::task::JoinHandle<Result<(u16, String)>>> {
    let mut tasks = Vec::new();
    for (provider, entry) in [
        (CatalogProvider::Openai, &control.openai),
        (CatalogProvider::OpenaiSecond, &control.openai_second),
        (CatalogProvider::DeepSeek, &control.deepseek),
        (CatalogProvider::Mimo, &control.mimo),
    ] {
        if entry.hang {
            let path = provider.path();
            tasks.push(tokio::spawn(async move { get_path(address, path).await }));
        }
    }
    tasks
}
