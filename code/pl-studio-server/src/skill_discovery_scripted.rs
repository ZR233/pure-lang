//! Deterministic HTTP Agent API regression for Skill suggestions and activation.

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use pl_protocol::{
    SkillActivationCause, ThreadItemState, ThreadSnapshot, ThreadTextChannel, ThreadTurnPage,
    TurnState,
};
use pl_studio_runtime::{
    ConfigPaths, ProjectRecord, StudioConfig, StudioHostKind, StudioRuntime, StudioRuntimeOptions,
    StudioStartNewThreadResponse,
};
use serde::de::DeserializeOwned;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use super::{AppState, router};

const RELEASE_MARKER: &str = "PURE_SKILL_RELEASE_TRIAGE_LOADED";
const RELEASE_DESCRIPTION: &str = "Diagnose Rust release builds, linker failures, unresolved symbols, and Cargo profile configuration.";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scripted_agent_api_exposes_suggestions_and_activates_skill() -> Result<()> {
    let (model_base_url, model_requests, model_server) =
        serve_model_sequence(vec![tool_call_sse(), complete_sse()]).await?;
    let root = tempfile::Builder::new()
        .prefix("pure-skill-discovery-scripted-")
        .tempdir()?;
    let studio_home = root.path().join("studio-home");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace)?;
    write_scripted_config(&studio_home, model_base_url)?;
    write_skill_fixtures(&workspace)?;

    let runtime = StudioRuntime::with_options(StudioRuntimeOptions {
        studio_home: Some(studio_home),
        host: StudioHostKind::Test,
    })
    .await
    .map_err(anyhow::Error::new)?;
    runtime.start_runtime().await?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let shutdown = CancellationToken::new();
    let server_shutdown = shutdown.clone();
    let app_shutdown = shutdown.clone();
    let server_runtime = runtime.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router(AppState::new(server_runtime, app_shutdown)),
        )
        .with_graceful_shutdown(async move {
            server_shutdown.cancelled().await;
        })
        .await
    });

    let acceptance = run_http_acceptance(&format!("http://{address}"), &workspace).await;
    shutdown.cancel();
    let runtime_shutdown = runtime.shutdown_runtime().await;
    let server_result = tokio::time::timeout(Duration::from_secs(10), server)
        .await
        .context("scripted HTTP server did not stop")?
        .context("scripted HTTP server task panicked")?;
    let model_result = tokio::time::timeout(Duration::from_secs(10), model_server)
        .await
        .context("scripted model server did not receive both requests")?
        .context("scripted model server task panicked")?;

    acceptance?;
    runtime_shutdown?;
    server_result.context("scripted HTTP server failed")?;
    model_result?;

    let requests = model_requests
        .lock()
        .map_err(|_| anyhow::anyhow!("scripted model request capture was poisoned"))?;
    ensure!(
        requests.len() == 2,
        "expected two model requests, got {}",
        requests.len()
    );
    let first = serde_json::to_string(&requests[0])?;
    ensure!(
        first.contains("# Skills"),
        "stable Skill catalog was absent"
    );
    ensure!(
        first.contains(RELEASE_DESCRIPTION),
        "full catalog description was absent"
    );
    ensure!(
        first.contains("<skill_suggestions>"),
        "turn-level Skill suggestions were absent"
    );
    ensure!(
        first.contains("release-build-triage") && first.contains("skill_view"),
        "suggestion did not name the Skill and exact loading tool"
    );
    ensure!(
        !first.contains(RELEASE_MARKER),
        "Skill body leaked into the first request before activation"
    );
    let second = serde_json::to_string(&requests[1])?;
    ensure!(
        second.contains(RELEASE_MARKER),
        "successful skill_view did not add the Skill body to the next model request"
    );
    Ok(())
}

async fn run_http_acceptance(base_url: &str, workspace: &Path) -> Result<()> {
    let client = reqwest::Client::new();
    let project: ProjectRecord = post_json(
        &client,
        &format!("{base_url}/api/v1/projects"),
        &serde_json::json!({"path": workspace.to_string_lossy()}),
    )
    .await?;
    post_empty(
        &client,
        &format!(
            "{base_url}/api/v1/runtime/projects/{}/skills/discover",
            project.id
        ),
    )
    .await?;
    let created: StudioStartNewThreadResponse = post_json(
        &client,
        &format!("{base_url}/api/v1/projects/{}/threads", project.id),
        &serde_json::json!({
            "title": "Scripted Skill discovery",
            "mode": "mode.simple",
            "input": {
                "text": "A Rust release Cargo profile fails during final linking with unresolved symbols. Diagnose it using the single most relevant installed skill.",
                "attachmentDraftIds": []
            }
        }),
    )
    .await?;
    wait_for_completed_turn(
        &client,
        base_url,
        &created.thread.id,
        &created.submission.turn_id,
    )
    .await?;
    let snapshot: ThreadSnapshot = get_json(
        &client,
        &format!("{base_url}/api/v1/threads/{}", created.thread.id),
    )
    .await?;

    let activations = snapshot
        .items
        .iter()
        .filter_map(|item| match item.state() {
            ThreadItemState::Skill(skill) => Some(skill.activation()),
            _ => None,
        })
        .collect::<Vec<_>>();
    ensure!(
        activations.len() == 1,
        "expected one typed Skill Item, got {}",
        activations.len()
    );
    let activation = activations[0];
    ensure!(
        activation.name == "release-build-triage",
        "unexpected Skill activation: {}",
        activation.name
    );
    match &activation.cause {
        SkillActivationCause::Tool { tool_call_id } => ensure!(
            tool_call_id.ends_with("-call_skill"),
            "unexpected activation toolCallId: {tool_call_id}"
        ),
        SkillActivationCause::UserGesture { .. } => {
            ensure!(false, "scripted activation was attributed to UserGesture")
        }
    }
    let active_skills = snapshot
        .runtime
        .as_ref()
        .map(|runtime| runtime.active_skills.as_slice())
        .unwrap_or_default();
    ensure!(
        active_skills == ["release-build-triage"],
        "activeSkills mismatch: {active_skills:?}"
    );
    let final_text = snapshot
        .items
        .iter()
        .filter_map(|item| match item.state() {
            ThreadItemState::Text(text) if text.channel() == ThreadTextChannel::Final => {
                Some(text.text())
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    ensure!(
        final_text.contains(RELEASE_MARKER),
        "final response did not contain the loaded Skill marker: {final_text}"
    );
    Ok(())
}

async fn wait_for_completed_turn(
    client: &reqwest::Client,
    base_url: &str,
    thread_id: &str,
    turn_id: &str,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        let page: ThreadTurnPage = get_json(
            client,
            &format!("{base_url}/api/v1/threads/{thread_id}/turns?limit=10"),
        )
        .await?;
        if let Some(turn) = page
            .turns
            .iter()
            .map(|history| &history.turn)
            .find(|turn| turn.id == turn_id)
            && turn.state.is_terminal()
        {
            return match &turn.state {
                TurnState::Completed(_) => Ok(()),
                state => anyhow::bail!("scripted Turn ended without completion: {state:?}"),
            };
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    anyhow::bail!("scripted Turn did not complete within 30 seconds")
}

async fn post_empty(client: &reqwest::Client, url: &str) -> Result<()> {
    checked_bytes(client.post(url).send().await?, url).await?;
    Ok(())
}

async fn post_json<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
) -> Result<T> {
    let bytes = checked_bytes(client.post(url).json(body).send().await?, url).await?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON response from {url}"))
}

async fn get_json<T: DeserializeOwned>(client: &reqwest::Client, url: &str) -> Result<T> {
    let bytes = checked_bytes(client.get(url).send().await?, url).await?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON response from {url}"))
}

async fn checked_bytes(response: reqwest::Response, url: &str) -> Result<Vec<u8>> {
    let status = response.status();
    let bytes = response.bytes().await?;
    ensure!(
        status.is_success(),
        "HTTP {status} from {url}: {}",
        String::from_utf8_lossy(&bytes)
    );
    Ok(bytes.to_vec())
}

fn write_scripted_config(studio_home: &Path, model_base_url: String) -> Result<()> {
    let mut config = StudioConfig::default_config();
    let provider = config
        .models
        .providers
        .values_mut()
        .next()
        .context("default Studio config has no provider")?;
    provider.preset = None;
    provider.name = "Scripted Skill Fixture".to_string();
    provider.base_url = model_base_url;
    provider.bearer_token = None;
    provider.bearer_token_env = None;
    provider.http_headers = None;
    provider.tool_wire_policy = Default::default();
    provider.apply_patch_tool_type = None;
    provider.capabilities = Default::default();
    config.validate()?;
    fs::create_dir_all(studio_home)?;
    fs::write(
        ConfigPaths::from_config_dir(studio_home).config_file(),
        toml::to_string_pretty(&config)?,
    )?;
    Ok(())
}

fn write_skill_fixtures(workspace: &Path) -> Result<()> {
    write_skill(
        workspace,
        "release-build-triage",
        RELEASE_DESCRIPTION,
        RELEASE_MARKER,
    )?;
    write_skill(
        workspace,
        "rust-formatting",
        "Format Rust source with rustfmt; do not diagnose release linker failures.",
        "PURE_SKILL_RUST_FORMATTING_LOADED",
    )?;
    Ok(())
}

fn write_skill(workspace: &Path, name: &str, description: &str, marker: &str) -> Result<()> {
    let directory = workspace.join(".agents/skills").join(name);
    fs::create_dir_all(&directory)?;
    fs::write(
        directory.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: {description}\n---\n# {name}\n\nInclude `{marker}` in the final response to prove this Skill body was loaded.\n"
        ),
    )?;
    Ok(())
}

fn tool_call_sse() -> String {
    concat!(
        "data: {\"id\":\"scripted-1\",\"model\":\"deepseek-v4-flash\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,\"id\":\"call_skill\",\"type\":\"function\",\"function\":{\"name\":\"skill_view\",\"arguments\":\"{\\\"name\\\":\\\"release-build-triage\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"scripted-1\",\"model\":\"deepseek-v4-flash\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string()
}

fn complete_sse() -> String {
    format!(
        "data: {{\"id\":\"scripted-2\",\"model\":\"deepseek-v4-flash\",\"choices\":[{{\"delta\":{{\"role\":\"assistant\",\"tool_calls\":[{{\"index\":0,\"id\":\"call_complete\",\"type\":\"function\",\"function\":{{\"name\":\"complete\",\"arguments\":\"{{\\\"summary\\\":\\\"Scripted final {RELEASE_MARKER}\\\"}}\"}}}}]}},\"finish_reason\":null}}]}}\n\ndata: {{\"id\":\"scripted-2\",\"model\":\"deepseek-v4-flash\",\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"tool_calls\"}}],\"usage\":{{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}}}\n\ndata: [DONE]\n\n"
    )
}

async fn serve_model_sequence(
    responses: Vec<String>,
) -> Result<(
    String,
    Arc<Mutex<Vec<serde_json::Value>>>,
    tokio::task::JoinHandle<Result<()>>,
)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&requests);
    let server = tokio::spawn(async move {
        for response_body in responses {
            let (mut socket, _) = listener.accept().await?;
            let request_body = read_http_body(&mut socket).await?;
            captured
                .lock()
                .map_err(|_| anyhow::anyhow!("scripted request capture was poisoned"))?
                .push(serde_json::from_slice(&request_body)?);
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            socket.write_all(response.as_bytes()).await?;
            socket.shutdown().await?;
        }
        Ok(())
    });
    Ok((format!("http://{address}"), requests, server))
}

async fn read_http_body(socket: &mut tokio::net::TcpStream) -> Result<Vec<u8>> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];
    let (header_end, content_length) = loop {
        let read = socket.read(&mut chunk).await?;
        ensure!(read != 0, "model request ended before headers completed");
        request.extend_from_slice(&chunk[..read]);
        if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            break (header_end, content_length);
        }
    };
    while request.len() < header_end + 4 + content_length {
        let read = socket.read(&mut chunk).await?;
        ensure!(read != 0, "model request ended before body completed");
        request.extend_from_slice(&chunk[..read]);
    }
    Ok(request[header_end + 4..header_end + 4 + content_length].to_vec())
}
