mod script;
mod sse;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use pl_studio_runtime::{
    AgentModelConfig, AgentRoleId, ConfigPaths, ConfigStore, ModelInfo, ModelParameter,
    ModelRouteConfig, PermissionMode, ProviderConfig, ProviderId, ReasoningEffort, StudioConfig,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use script::{ScriptProgress, next_step, observe_request, response, role};

pub(super) struct ServerOptions {
    workspace: PathBuf,
    config_home: PathBuf,
    request_log: PathBuf,
}

impl ServerOptions {
    pub(super) fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Self> {
        let mut arguments = arguments.into_iter();
        let mut workspace = None;
        let mut config_home = None;
        let mut request_log = None;
        while let Some(name) = arguments.next() {
            let value = arguments
                .next()
                .with_context(|| format!("missing value for {}", name.to_string_lossy()))?;
            match name.to_string_lossy().as_ref() {
                "--workspace" => workspace = Some(PathBuf::from(value)),
                "--config-home" => config_home = Some(PathBuf::from(value)),
                "--request-log" => request_log = Some(PathBuf::from(value)),
                unknown => bail!("unknown argument: {unknown}"),
            }
        }
        let options = Self {
            workspace: workspace.context("missing --workspace")?,
            config_home: config_home.context("missing --config-home")?,
            request_log: request_log.context("missing --request-log")?,
        };
        options.validate()?;
        Ok(options)
    }

    fn validate(&self) -> Result<()> {
        for (name, path) in [
            ("workspace", &self.workspace),
            ("config home", &self.config_home),
            ("request log", &self.request_log),
        ] {
            if !path.is_absolute() {
                bail!("{name} must be an absolute path: {}", path.display());
            }
        }
        if !self.workspace.is_dir() {
            bail!("workspace does not exist: {}", self.workspace.display());
        }
        Ok(())
    }
}

struct ServerState {
    workspace: PathBuf,
    request_log: PathBuf,
    progress: Mutex<ScriptProgress>,
}

pub(super) async fn run(options: ServerOptions) -> Result<()> {
    if let Some(parent) = options.request_log.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let base_url = format!("http://{}", listener.local_addr()?);
    write_config(&options.config_home, base_url.clone())?;
    let state = Arc::new(ServerState {
        workspace: options.workspace,
        request_log: options.request_log,
        progress: Mutex::new(ScriptProgress::default()),
    });
    println!("PURE_TASK_PROVIDER_READY {base_url}");
    std::io::stdout().flush()?;

    loop {
        let (socket, _) = listener.accept().await?;
        let request_state = state.clone();
        tokio::spawn(async move {
            serve_request(socket, request_state).await;
        });
    }
}

async fn serve_request(mut socket: TcpStream, state: Arc<ServerState>) {
    let result = async {
        let request = read_json_request(&mut socket).await?;
        let mut progress = state.progress.lock().await;
        let role = role(&request)?;
        observe_request(&mut progress, &request);
        let step = next_step(&mut progress, role);
        let (action, body) = response(&mut progress, &state.workspace, role, step)?;
        append_request_log(&state.request_log, role.label(), step, action, &request)?;
        println!("SCRIPTED_TASK_PROVIDER {}[{step}] {action}", role.label());
        std::io::stdout().flush()?;
        drop(progress);
        write_response(&mut socket, "200 OK", "text/event-stream", &body).await
    }
    .await;

    if let Err(error) = result {
        let message = format!("scripted Task provider failed: {error:#}");
        let _ = append_error_log(&state.request_log, &message);
        eprintln!("{message}");
        let _ = write_response(&mut socket, "400 Bad Request", "text/plain", &message).await;
    }
}

fn write_config(home: &Path, base_url: String) -> Result<()> {
    std::fs::create_dir_all(home)
        .with_context(|| format!("failed to create config home {}", home.display()))?;
    let mut model = ModelInfo::fallback("local-responses");
    model.context_window = Some(128_000);
    model.parameters = vec![ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["none".to_string()],
        wire: BTreeMap::new(),
    }];
    let mut info = pl_model::ProviderInfo::openai(Some(base_url));
    info.connection_mode = pl_model::ProviderConnectionMode::Http;
    info.default_model = "local-responses".to_string();
    let provider = ProviderConfig::from_provider_info(info, vec![model]);
    let provider_id = ProviderId::new("local")?;
    let route = ModelRouteConfig {
        provider: provider_id.clone(),
        model: "local-responses".to_string(),
        effort: Some(ReasoningEffort::new("none")),
    };
    let mut config = StudioConfig::default_config();
    config.models = AgentModelConfig {
        providers: BTreeMap::from([(provider_id, provider)]),
        routes: ["explorer", "planner", "executor", "reviewer"]
            .into_iter()
            .map(|role| {
                (
                    AgentRoleId::new(role).expect("fixed role id"),
                    route.clone(),
                )
            })
            .collect(),
    };
    config.runtime.permission_mode = PermissionMode::FullAccess;
    config.runtime.tool_capabilities.skills = false;
    config.runtime.tool_capabilities.mcp = false;
    config.runtime.tool_capabilities.lsp = false;
    config.skills.enabled = false;
    config.skills.auto_learn = false;
    ConfigStore::new(ConfigPaths::from_home(home)).save(&config)?;
    Ok(())
}

fn append_request_log(
    path: &Path,
    role: &str,
    step: usize,
    action: &str,
    request: &serde_json::Value,
) -> Result<()> {
    append_log(
        path,
        &serde_json::json!({
            "kind": "request",
            "role": role,
            "step": step,
            "action": action,
            "request": request,
        }),
    )
}

fn append_error_log(path: &Path, error: &str) -> Result<()> {
    append_log(path, &serde_json::json!({"kind": "error", "error": error}))
}

fn append_log(path: &Path, value: &serde_json::Value) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    writeln!(file, "{value}")?;
    file.flush()?;
    Ok(())
}

async fn read_json_request(socket: &mut TcpStream) -> Result<serde_json::Value> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let (header_end, content_length) = loop {
        let count = socket.read(&mut chunk).await?;
        if count == 0 {
            bail!("model request closed before headers completed");
        }
        buffer.extend_from_slice(&chunk[..count]);
        if let Some(header_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&buffer[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .context("model request has no content-length")?;
            break (header_end, content_length);
        }
    };
    let body_start = header_end + 4;
    while buffer.len() < body_start + content_length {
        let count = socket.read(&mut chunk).await?;
        if count == 0 {
            bail!("model request closed before body completed");
        }
        buffer.extend_from_slice(&chunk[..count]);
    }
    serde_json::from_slice(&buffer[body_start..body_start + content_length])
        .context("model request body is not JSON")
}

async fn write_response(
    socket: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
) -> Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(response.as_bytes()).await?;
    socket.shutdown().await?;
    Ok(())
}
