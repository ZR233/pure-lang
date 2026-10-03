//! Live collection only. A human/agent reads the evidence; no marker or automated acceptance verdict.
use anyhow::{Context, Result};
use pl_studio_runtime::{ConfigStore, StudioHostKind, StudioRuntime, StudioRuntimeOptions};
use std::{
    collections::BTreeSet,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

#[tokio::main]
async fn main() -> Result<()> {
    let artifacts = match std::env::var_os("ANYWORK_WORKFLOW_ARTIFACT_DIR") {
        Some(path) => PathBuf::from(path),
        None => tempfile::Builder::new()
            .prefix("anywork-collaboration-")
            .tempdir()?
            .keep(),
    };
    std::fs::create_dir_all(&artifacts)?;
    let home = artifacts.join("studio-home");
    let workspace = tempfile::Builder::new()
        .prefix("anywork-observation-workspace-")
        .tempdir()?
        .keep();
    std::fs::write(
        artifacts.join("workspace-path.txt"),
        workspace.to_string_lossy().as_bytes(),
    )?;
    std::fs::create_dir_all(&home)?;
    std::fs::create_dir_all(&workspace)?;
    let workspace_mode = match std::env::var("ANYWORK_OBSERVATION_WORKSPACE_MODE").as_deref() {
        Ok("worktree") => pl_protocol::ThreadWorkspaceMode::Worktree,
        Ok("local") | Err(std::env::VarError::NotPresent) => {
            pl_protocol::ThreadWorkspaceMode::Local
        }
        other => anyhow::bail!("invalid observation workspace mode: {other:?}"),
    };
    if workspace_mode == pl_protocol::ThreadWorkspaceMode::Worktree {
        initialize_repository(&workspace)?;
    }
    let installed = ConfigStore::default_app()?;
    // Copy bytes, never save a hydrated config: the credential store belongs to the user.
    std::fs::copy(installed.paths().config_file(), home.join("config.toml"))?;
    if let Ok(provider_id) = std::env::var("ANYWORK_OBSERVATION_PROVIDER") {
        let slug = std::env::var("ANYWORK_OBSERVATION_MODEL")
            .context("selected observation provider requires ANYWORK_OBSERVATION_MODEL")?;
        let mut config: toml::Value =
            toml::from_str(&std::fs::read_to_string(home.join("config.toml"))?)?;
        let providers = config
            .get_mut("models")
            .and_then(|v| v.get_mut("providers"))
            .and_then(toml::Value::as_table_mut)
            .context("provider table is missing")?;
        let selected = providers
            .get(&provider_id)
            .context("observation provider is not configured")?
            .clone();
        let provider: pl_model::config::ProviderConfig = selected.clone().try_into()?;
        let model = provider
            .effective_models()?
            .into_iter()
            .find(|m| m.slug == slug)
            .context("observation model is not configured")?;
        let effort = model
            .parameters
            .iter()
            .find(|p| p.name == "effort")
            .and_then(|p| p.candidates.first());
        providers.clear();
        providers.insert(provider_id.clone(), selected);
        let routes = config
            .get_mut("models")
            .and_then(|v| v.get_mut("routes"))
            .and_then(toml::Value::as_table_mut)
            .context("route table is missing")?;
        for (_, route) in routes.iter_mut() {
            let route = route.as_table_mut().context("invalid route")?;
            route.insert("provider".into(), provider_id.clone().into());
            route.insert("model".into(), slug.clone().into());
            if let Some(effort) = effort {
                route.insert("effort".into(), effort.clone().into());
            } else {
                route.remove("effort");
            }
        }
        let modes = config
            .get_mut("mode_model_routes")
            .and_then(toml::Value::as_table_mut)
            .context("mode route table is missing")?;
        for (_, route) in modes.iter_mut() {
            let route = route.as_table_mut().context("invalid mode route")?;
            route.insert("provider".into(), provider_id.clone().into());
            route.insert("model".into(), slug.clone().into());
            if let Some(effort) = effort {
                route.insert("effort".into(), effort.clone().into());
            } else {
                route.remove("effort");
            }
        }
        std::fs::write(home.join("config.toml"), toml::to_string(&config)?)?;
    }
    // Fail explicitly before runtime initialization can apply configuration recovery defaults.
    ConfigStore::for_studio_home(home.clone()).load()?;
    let prompt = match std::env::var_os("ANYWORK_OBSERVATION_PROMPT") {
        Some(path) => std::fs::read_to_string(path)?,
        None => "请派两个 fresh-context explorer 分别解释文件所有权和 Turn 生命周期。详细、自包含地派发。一个自然 final，另一个 finish_turn。等待完整报告，阅读后总结；不修改文件，不提交计划，不询问用户。".into(),
    };
    let seconds = std::env::var("ANYWORK_OBSERVATION_SECONDS")
        .ok()
        .map(|v| v.parse::<u64>())
        .transpose()?
        .unwrap_or(180);
    let runtime = StudioRuntime::initialize(StudioRuntimeOptions {
        studio_home: Some(home),
        host: StudioHostKind::Desktop,
        ..StudioRuntimeOptions::desktop()
    })
    .await?;
    let mut observers = Vec::new();
    let result = collect(
        &runtime,
        &mut observers,
        Observation {
            workspace: &workspace,
            artifacts: &artifacts,
            prompt,
            workspace_mode,
            seconds,
        },
    )
    .await;
    // Even failed collection must close the runtime and drain every evidence writer.
    let shutdown = runtime.shutdown_runtime().await;
    let observations = futures::future::join_all(observers).await;
    result?;
    shutdown?;
    for observation in observations {
        observation??;
    }
    std::fs::write(
        artifacts.join("observation.txt"),
        "Collection ended. Read the complete event streams, snapshots and wire evidence; no automated acceptance verdict was computed.\n",
    )?;
    Ok(())
}

struct Observation<'a> {
    workspace: &'a Path,
    artifacts: &'a Path,
    prompt: String,
    workspace_mode: pl_protocol::ThreadWorkspaceMode,
    seconds: u64,
}

async fn collect(
    runtime: &StudioRuntime,
    observers: &mut Vec<tokio::task::JoinHandle<Result<()>>>,
    observation: Observation<'_>,
) -> Result<()> {
    let Observation {
        workspace,
        artifacts,
        prompt,
        workspace_mode,
        seconds,
    } = observation;
    let project = runtime.open_project(workspace).await?;
    let created = runtime
        .create_thread_command(
            project.id,
            pl_protocol::studio::CreateThreadRequest {
                title: Some("真实协作过程观察".into()),
                input: pl_protocol::studio::StudioPromptInput {
                    input_id: format!("observation-{}", std::process::id()),
                    text: prompt,
                    attachment_draft_ids: vec![],
                },
                mode: "mode.simple".into(),
                workspace_mode,
            },
        )
        .await?;
    let thread = created.thread;
    std::fs::write(
        artifacts.join("root-thread.json"),
        serde_json::to_vec_pretty(&thread)?,
    )?;
    let mut observed = BTreeSet::new();
    observers.push(observe(runtime, &thread.id, artifacts).await?);
    observed.insert(thread.id.clone());
    println!(
        "Observing real API execution for {seconds}s. Artifacts: {}. No automatic pass/fail verdict.",
        artifacts.display()
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
    while tokio::time::Instant::now() < deadline {
        // The product directory exists before a new Thread's first history write.
        // Do not read a cold timeline while its SQLite schema is still initializing.
        let state = runtime.read_state().await?;
        if let Some(directory) = state.agent_directory.state.value() {
            for agent in directory
                .agents
                .iter()
                .filter(|agent| agent.root_thread_id == thread.id)
            {
                if observed.insert(agent.thread_id.clone()) {
                    observers.push(observe(runtime, &agent.thread_id, artifacts).await?);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    for id in &observed {
        let mut cursor = None;
        let mut turns = Vec::new();
        loop {
            let page = runtime
                .list_thread_turns(id, cursor.as_deref(), 200)
                .await?;
            turns.extend(page.turns);
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        std::fs::write(
            artifacts.join(format!("{id}.turns.json")),
            serde_json::to_vec_pretty(&turns)?,
        )?;
        let snapshot = runtime.thread_snapshot(id).await?;
        std::fs::write(
            artifacts.join(format!("{id}.snapshot.json")),
            serde_json::to_vec_pretty(&snapshot)?,
        )?;
        let page = runtime
            .list_timeline_items(id, pl_protocol::TimelineQuery::Latest, 200)
            .await?;
        std::fs::write(
            artifacts.join(format!("{id}.timeline.json")),
            serde_json::to_vec_pretty(&page)?,
        )?;
    }
    Ok(())
}

fn initialize_repository(workspace: &Path) -> Result<()> {
    for args in [
        vec!["init"],
        vec!["config", "user.name", "anywork observation"],
        vec!["config", "user.email", "observation@anywork.invalid"],
    ] {
        run_git(workspace, &args)?;
    }
    std::fs::write(
        workspace.join("README.md"),
        "Isolated live collaboration workspace.\n",
    )?;
    run_git(workspace, &["add", "README.md"])?;
    run_git(
        workspace,
        &["commit", "-m", "chore: initialize observation workspace"],
    )
}

fn run_git(workspace: &Path, args: &[&str]) -> Result<()> {
    let output = std::process::Command::new("git")
        .arg("-c")
        .arg("core.hooksPath=")
        .args(args)
        .current_dir(workspace)
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "observation git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

async fn observe(
    runtime: &StudioRuntime,
    id: &str,
    artifacts: &Path,
) -> Result<tokio::task::JoinHandle<Result<()>>> {
    let mut stream = runtime
        .subscribe_thread(pl_protocol::ThreadSubscriptionRequest {
            thread_id: id.into(),
        })
        .await?;
    let mut file = std::fs::File::create(artifacts.join(format!("{id}.events.jsonl")))?;
    Ok(tokio::spawn(async move {
        while let Some(event) = stream.recv().await? {
            serde_json::to_writer(&mut file, &event)?;
            file.write_all(b"\n")?;
            file.flush()?;
        }
        Ok(())
    }))
}
