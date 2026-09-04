//! Installed-provider HTTP acceptance for deterministic Skill discovery.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use pl_protocol::{
    SkillActivationCause, ThreadItemState, ThreadSnapshot, ThreadTextChannel, ThreadTurnPage,
    TurnState,
};
use pl_studio_runtime::{
    ConfigPaths, ConfigStore, ProjectRecord, STUDIO_CONFIG_SCHEMA_VERSION, StudioRuntime,
    StudioRuntimeOptions, StudioStartNewThreadResponse,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use super::{AppState, router};

const LIVE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const RELEASE_MARKER: &str = "PURE_SKILL_RELEASE_TRIAGE_LOADED";
const SLIDE_MARKER: &str = "PURE_SKILL_SLIDE_AUTHORING_LOADED";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LiveReceipt {
    provider: String,
    model: String,
    cases: Vec<CaseReceipt>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaseReceipt {
    case: &'static str,
    thread_id: String,
    turn_id: String,
    activation_cause: Option<&'static str>,
    tool_call_id: Option<String>,
    final_marker: Option<&'static str>,
    elapsed_millis: u64,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "uses the installed Studio Planner provider and incurs real model usage"]
async fn installed_config_agent_api_selects_skills_from_name_and_description() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_test_writer()
        .try_init();
    let installed = ConfigStore::default_app()?;
    let root = tempfile::Builder::new()
        .prefix("pure-skill-discovery-live-")
        .tempdir()?;
    let studio_home = root.path().join("studio-home");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace)?;
    write_isolated_config(&installed, &studio_home)?;
    let installed_config = ConfigStore::for_studio_home(&studio_home)
        .load()
        .context("isolated installed Studio config is invalid after schema normalization")?;
    let route = installed_config.resolve_role(pl_studio_runtime::StudioRole::Planner)?;
    let base_url = route.endpoint.base_url.to_ascii_lowercase();
    ensure!(
        !["localhost", "127.0.0.1", "[::1]", "0.0.0.0"]
            .iter()
            .any(|host| base_url.contains(host)),
        "Skill discovery live acceptance cannot use a local/scripted endpoint"
    );
    ensure!(
        route.endpoint.bearer_token.is_some(),
        "Planner route has no credential resolved by the system credential store"
    );
    let provider = route.provider_id.as_str().to_string();
    let model = route.model.slug.clone();
    write_skill_fixtures(&workspace)?;

    let runtime = StudioRuntime::with_options(StudioRuntimeOptions::http_server(Some(studio_home)))
        .await
        .map_err(anyhow::Error::new)?;
    runtime.start_runtime().await?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let shutdown = CancellationToken::new();
    let server_shutdown = shutdown.clone();
    let server_runtime = runtime.clone();
    let app_shutdown = shutdown.clone();
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

    let acceptance =
        run_http_acceptance(&format!("http://{address}"), &workspace, provider, model).await;
    shutdown.cancel();
    let runtime_shutdown = runtime.shutdown_runtime().await;
    let server_result = tokio::time::timeout(Duration::from_secs(15), server)
        .await
        .context("HTTP server did not stop after cancellation")?
        .context("HTTP server task panicked")?;

    acceptance?;
    runtime_shutdown?;
    server_result.context("HTTP server failed")?;
    Ok(())
}

async fn run_http_acceptance(
    base_url: &str,
    workspace: &Path,
    provider: String,
    model: String,
) -> Result<()> {
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

    let cases = vec![
        run_case(
            &client,
            base_url,
            &project.id,
            "description",
            "A Rust binary succeeds in debug builds but its release Cargo profile fails during final linking with unresolved symbols. Diagnose the release linker problem. Activate only the single most relevant installed skill before answering, do not load a formatting skill, and include the exact acceptance marker required by the loaded skill.",
            Some(("release-build-triage", RELEASE_MARKER)),
        )
        .await?,
        run_case(
            &client,
            base_url,
            &project.id,
            "name",
            "Use the installed slide-deck-authoring skill to outline a three-slide presentation with speaker notes. The skill name here is ordinary text, not a slash command. Activate exactly that one skill and include the exact acceptance marker required by its body.",
            Some(("slide-deck-authoring", SLIDE_MARKER)),
        )
        .await?,
        run_case(
            &client,
            base_url,
            &project.id,
            "negative",
            "Do not use any skill or call any tool. Reply briefly with the result of 2 + 2.",
            None,
        )
        .await?,
    ];

    let receipt = LiveReceipt {
        provider,
        model,
        cases,
    };
    let receipt_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/skill-discovery-live");
    fs::create_dir_all(&receipt_dir)?;
    fs::write(
        receipt_dir.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    Ok(())
}

async fn run_case(
    client: &reqwest::Client,
    base_url: &str,
    project_id: &str,
    case: &'static str,
    prompt: &str,
    expected: Option<(&'static str, &'static str)>,
) -> Result<CaseReceipt> {
    let started = Instant::now();
    let created: StudioStartNewThreadResponse = post_json(
        client,
        &format!("{base_url}/api/v1/projects/{project_id}/threads"),
        &serde_json::json!({
            "title": format!("Skill discovery live {case}"),
            "mode": "mode.simple",
            "input": {"text": prompt, "attachmentDraftIds": []},
        }),
    )
    .await?;
    wait_for_completed_turn(
        client,
        base_url,
        &created.thread.id,
        &created.submission.turn_id,
    )
    .await?;
    let snapshot: ThreadSnapshot = get_json(
        client,
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
    let active_skills = snapshot
        .runtime
        .as_ref()
        .map(|runtime| runtime.active_skills.as_slice())
        .unwrap_or_default();
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
    ensure!(!final_text.trim().is_empty(), "{case} final reply is empty");

    let (activation_cause, tool_call_id, final_marker) = if let Some((name, marker)) = expected {
        ensure!(
            activations.len() == 1,
            "{case} expected exactly one Skill Item, got {:?}",
            activations
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>()
        );
        let activation = activations[0];
        ensure!(
            activation.name == name,
            "{case} activated `{}` instead of `{name}`",
            activation.name
        );
        ensure!(
            active_skills == [name],
            "{case} activeSkills mismatch: {active_skills:?}"
        );
        let tool_call_id = match &activation.cause {
            SkillActivationCause::Tool { tool_call_id } => {
                ensure!(!tool_call_id.is_empty(), "{case} toolCallId is empty");
                tool_call_id.clone()
            }
            SkillActivationCause::UserGesture { .. } => {
                bail!("{case} activation was incorrectly attributed to UserGesture")
            }
        };
        ensure!(
            final_text.contains(marker),
            "{case} final reply did not contain `{marker}`: {final_text}"
        );
        (Some("tool"), Some(tool_call_id), Some(marker))
    } else {
        ensure!(
            activations.is_empty(),
            "negative case unexpectedly emitted Skill Items: {:?}",
            activations
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>()
        );
        ensure!(
            active_skills.is_empty(),
            "negative case unexpectedly activated Skills: {active_skills:?}"
        );
        (None, None, None)
    };

    Ok(CaseReceipt {
        case,
        thread_id: created.thread.id,
        turn_id: created.submission.turn_id,
        activation_cause,
        tool_call_id,
        final_marker,
        elapsed_millis: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    })
}

async fn wait_for_completed_turn(
    client: &reqwest::Client,
    base_url: &str,
    thread_id: &str,
    turn_id: &str,
) -> Result<()> {
    let deadline = Instant::now() + LIVE_TIMEOUT;
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
                state => bail!("Turn {turn_id} ended without completion: {state:?}"),
            };
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    bail!("Turn {turn_id} exceeded the 10 minute live-test timeout")
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

fn write_skill_fixtures(workspace: &Path) -> Result<()> {
    write_skill(
        workspace,
        "release-build-triage",
        "Diagnose Rust release builds, linker failures, unresolved symbols, and Cargo profile configuration.",
        RELEASE_MARKER,
    )?;
    write_skill(
        workspace,
        "slide-deck-authoring",
        "Author presentations and slide decks with clear structure, visual hierarchy, and speaker notes.",
        SLIDE_MARKER,
    )?;
    write_skill(
        workspace,
        "rust-formatting",
        "Format Rust source code and apply consistent rustfmt style without diagnosing linker failures.",
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
            "---\nname: {name}\ndescription: {description}\n---\n# {name}\n\nThis Skill is loaded for an Agent API acceptance run. Do not load another Skill. Complete the user's requested answer, and include the exact marker `{marker}` in the final response to prove this body entered model context.\n"
        ),
    )?;
    Ok(())
}

fn write_isolated_config(installed: &ConfigStore, studio_home: &Path) -> Result<()> {
    let source = fs::read_to_string(installed.paths().config_file())?;
    let mut table = source.parse::<toml::Table>()?;
    table.insert(
        "schema_version".to_string(),
        toml::Value::Integer(i64::from(STUDIO_CONFIG_SCHEMA_VERSION)),
    );
    let providers = table
        .get_mut("models")
        .and_then(toml::Value::as_table_mut)
        .and_then(|models| models.get_mut("providers"))
        .and_then(toml::Value::as_table_mut)
        .context("installed config has no models.providers table")?;
    for (_, provider) in providers.iter_mut() {
        if let Some(provider) = provider.as_table_mut() {
            ensure!(
                provider.remove("bearer_token").is_none(),
                "inline provider credentials are not allowed in live acceptance"
            );
        }
    }
    fs::create_dir_all(studio_home)?;
    fs::write(
        ConfigPaths::from_config_dir(studio_home).config_file(),
        toml::to_string_pretty(&table)?,
    )?;
    Ok(())
}
