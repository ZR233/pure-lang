//! Real-provider rework scenarios, isolated from production scheduling and user state.
use std::{collections::BTreeMap, fs, path::Path, time::Instant};

use anyhow::{Context, Result, bail, ensure};
use pl_protocol::{InteractionContent, InteractionStatus, ThreadToolState};
use pl_studio_runtime::{
    ConfigStore, StudioConfig, StudioHostKind, StudioRuntime, StudioRuntimeOptions, ThreadModeId,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[path = "workflow_rework_evidence.rs"]
mod evidence;

#[derive(Clone, Copy, Debug)]
enum WorkspaceKind {
    Directory,
    Worktree,
}

impl WorkspaceKind {
    fn name(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::Worktree => "worktree",
        }
    }
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Call {
    id: String,
    turn_id: String,
    completed_at: i64,
    name: String,
    arguments: serde_json::Value,
    output: String,
}

#[derive(Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Actor {
    id: String,
    role: String,
    calls: Vec<Call>,
    snapshot: pl_protocol::ThreadSnapshot,
}

pub(super) async fn run(installed: &ConfigStore, config: &StudioConfig) -> Result<()> {
    for kind in [WorkspaceKind::Directory, WorkspaceKind::Worktree] {
        run_scenario(installed, config, kind).await?;
    }
    Ok(())
}

pub(super) async fn run_selected(
    installed: &ConfigStore,
    config: &StudioConfig,
    name: &str,
) -> Result<()> {
    let kind = match name {
        "directory" => WorkspaceKind::Directory,
        "worktree" => WorkspaceKind::Worktree,
        _ => bail!("rework scenario must be directory or worktree"),
    };
    run_scenario(installed, config, kind).await
}

async fn run_scenario(
    installed: &ConfigStore,
    config: &StudioConfig,
    kind: WorkspaceKind,
) -> Result<()> {
    let artifacts = std::env::var_os("PURE_STUDIO_WORKFLOW_ARTIFACT_DIR")
        .map(std::path::PathBuf::from)
        .context("rework live acceptance requires PURE_STUDIO_WORKFLOW_ARTIFACT_DIR")?
        .join(format!("rework-{}", kind.name()));
    fs::create_dir_all(&artifacts)?;
    let usage_before = usage_captures()?;
    let request_baseline = request_capture_count()?;
    // Retain evidence and source copies; temporary projects are removed after shutdown. No secret is
    // copied inline; the existing credential-store contract is used by write_isolated_config.
    // A non-Git fixture must live outside the checkout: placing it under target/
    // would let Git discover the user's repository through parent directories.
    let fixture = tempfile::Builder::new()
        .prefix(&format!("pure-rework-{}-", kind.name()))
        .tempdir()?;
    let workspace = fixture.path().join("workspace");
    fs::write(
        artifacts.join("workspace-location.txt"),
        format!("{}\n", workspace.display()),
    )?;
    let studio_home = artifacts.join("studio-home");
    fs::create_dir_all(&workspace)?;
    super::copy_directory(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-fixtures/workflow-rework/workspace"),
        &workspace,
    )?;
    let mut prompt =
        include_str!("../../../../test-fixtures/workflow-rework/prompt.md").to_string();
    if matches!(kind, WorkspaceKind::Worktree) {
        let rules = fs::read_to_string(workspace.join("AGENTS.md"))?.replace(
            "This fixture intentionally has no Git repository. Do not initialize one or create worktree state.",
            "This fixture is a Git repository. Implement in isolated worktrees; only the root integrates commits."
        );
        fs::write(workspace.join("AGENTS.md"), rules)?;
        fs::write(workspace.join(".gitignore"), "target/\n.pure/\n")?;
        command(&workspace, &artifacts, "git-init", "git", &["init"]).await?;
        command(&workspace, &artifacts, "git-add", "git", &["add", "."]).await?;
        command(
            &workspace,
            &artifacts,
            "git-baseline",
            "git",
            &[
                "-c",
                "user.name=Workflow Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "-m",
                "test: initialize isolated fixture",
            ],
        )
        .await?;
        prompt = prompt.replace("The project is intentionally not a Git repository.",
            "The project is a Git repository. Use isolated worktree implementation assignments; root alone integrates commits. For fixture commits use git -c user.name='Workflow Fixture' -c user.email=fixture@example.invalid without changing Git configuration.");
    }
    fs::write(artifacts.join("prompt.md"), &prompt)?;
    super::write_isolated_config(installed, &studio_home, config)?;
    let runtime = StudioRuntime::with_options(StudioRuntimeOptions {
        studio_home: Some(studio_home),
        host: StudioHostKind::Desktop,
    })
    .await
    .map_err(anyhow::Error::new)?;
    runtime.start_runtime().await?;
    let outcome = async {
        let project = runtime.open_project(&workspace).await?;
        let thread = runtime
            .create_thread(
                &project.id,
                &format!("Rework {} live acceptance", kind.name()),
            )
            .await?;
        runtime
            .set_thread_mode(&thread.id, ThreadModeId::new("mode.task")?)
            .await?;
        let observer = observe_turns(&runtime, &thread.id).await?;
        runtime
            .start_turn(
                thread.id.clone(),
                pl_protocol::studio::StartTurnRequest {
                    input: pl_protocol::studio::StudioPromptInput {
                        text: prompt,
                        attachment_draft_ids: Vec::new(),
                    },
                },
            )
            .await?;
        let mut actors = BTreeMap::new();
        let result = drive(
            &runtime,
            &thread.id,
            &workspace,
            &artifacts,
            &mut actors,
            &observer.updates,
            request_baseline,
        )
        .await;
        // Capture before checking the result so failures retain the actual first attempt.
        collect(&runtime, &thread.id, &mut actors).await?;
        fs::write(
            artifacts.join("actors.json"),
            serde_json::to_vec_pretty(&actors)?,
        )?;
        evidence::write_verification_report(&actors, &artifacts)?;
        result?;
        evidence::validate(&actors, &thread.id, kind, &artifacts)?;
        verify_delivery(&workspace, &artifacts, kind).await?;
        fs::write(artifacts.join("result.txt"), "completed\n")?;
        println!(
            "Real rework {} acceptance passed: {}",
            kind.name(),
            artifacts.display()
        );
        Ok::<_, anyhow::Error>(())
    }
    .await;
    let usage_result = save_cache_usage(&usage_before, &artifacts);
    let copy_result = (|| -> Result<()> {
        let delivered_files = artifacts.join("delivered-files");
        fs::create_dir_all(&delivered_files)?;
        for directory in ["src", "tests", "design"] {
            let source = workspace.join(directory);
            if source.is_dir() {
                let destination = delivered_files.join(directory);
                fs::create_dir_all(&destination)?;
                super::copy_directory(&source, &destination)?;
            }
        }
        Ok(())
    })();
    let shutdown = runtime.shutdown_runtime().await;
    fs::write(
        artifacts.join("persistence-result.txt"),
        match &shutdown {
            Ok(_) => "shutdown and persistence drain succeeded\n".to_string(),
            Err(error) => format!("shutdown/persistence failed: {error}\n"),
        },
    )?;
    // Preserve unfinished worktree source and Git metadata before the TempDir is dropped.
    let archive = artifacts.join("workspace-source");
    preserve_workspace(&workspace, &archive)?;
    if let Err(error) = usage_result.and(copy_result) {
        return Err(error.context(format!("cache evidence capture failed; scenario result: {outcome:?}; shutdown result: {shutdown:?}")));
    }
    match (outcome, shutdown) {
        (Ok(()), Ok(_)) => Ok(()),
        (Err(error), Ok(_)) => {
            Err(error.context(format!("rework artifacts: {}", artifacts.display())))
        }
        (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(shutdown)) => {
            Err(error.context(format!("runtime shutdown also failed: {shutdown}")))
        }
    }
}

fn preserve_workspace(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_name() == "target" {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            continue;
        }
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            preserve_workspace(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

async fn verify_delivery(workspace: &Path, artifacts: &Path, kind: WorkspaceKind) -> Result<()> {
    // Independent harness checks are deliberately separate from agent verification.
    fs::write(
        workspace.join("tests/harness_rework_verification.rs"),
        "#[test]\nfn independent_separator_boundary() {\n    assert_eq!(\n    workflow_live_fixture::normalize::normalize_key(\"Cache--Edge\"),\n    Ok(\"cache-edge\".to_string()),\n    );\n}\n",
    )?;
    command(
        workspace,
        artifacts,
        "harness-final-tests",
        "cargo",
        &["test"],
    )
    .await?;
    if matches!(kind, WorkspaceKind::Worktree) {
        let worktrees = command(
            workspace,
            artifacts,
            "harness-worktrees",
            "git",
            &["worktree", "list", "--porcelain"],
        )
        .await?;
        ensure!(
            worktrees
                .lines()
                .filter(|line| line.starts_with("worktree "))
                .count()
                == 1,
            "temporary child worktrees were not cleaned"
        );
        let branches = command(
            workspace,
            artifacts,
            "harness-branches",
            "git",
            &["branch", "--list", "pure-agent-*"],
        )
        .await?;
        ensure!(
            branches.trim().is_empty(),
            "temporary child branches were not cleaned"
        );
    }
    Ok(())
}

pub(super) async fn replay_saved(artifacts: &Path) -> Result<()> {
    let kind = match artifacts.file_name().and_then(|name| name.to_str()) {
        Some("rework-directory") => WorkspaceKind::Directory,
        Some("rework-worktree") => WorkspaceKind::Worktree,
        _ => bail!("expected a saved rework-directory or rework-worktree artifact directory"),
    };
    let actors: BTreeMap<String, Actor> =
        serde_json::from_slice(&fs::read(artifacts.join("actors.json"))?)?;
    let root = actors
        .values()
        .find(|actor| actor.role == "root")
        .context("saved root missing")?;
    if !artifacts.join("injection.json").exists() {
        evidence::validate_checkpoint(&actors, &root.id)?;
        evidence::write_verification_report(&actors, artifacts)?;
        fs::write(
            artifacts.join("replay-result.txt"),
            "initial checkpoint replay passed; no injection or full rework approval evidence\n",
        )?;
        return Ok(());
    }
    evidence::validate(&actors, &root.id, kind, artifacts)?;
    let location = fs::read_to_string(artifacts.join("workspace-location.txt"))?;
    let workspace = if Path::new(location.trim()).exists() {
        std::path::PathBuf::from(location.trim())
    } else {
        artifacts.join("workspace-source")
    };
    for relative in [
        "src/normalize.rs",
        "tests/normalize.rs",
        "design/task-workflows.md",
    ] {
        ensure!(
            fs::read(workspace.as_path().join(relative))?
                == fs::read(artifacts.join("delivered-files").join(relative))?,
            "saved reviewed content changed before replay: {relative}"
        );
    }
    verify_delivery(workspace.as_path(), artifacts, kind).await?;
    fs::write(artifacts.join("replay-result.txt"), "passed: revalidated real provider snapshots; independently checked delivered project; no new model calls
")?;
    println!(
        "Saved real-provider evidence and delivery verified: {}",
        artifacts.display()
    );
    Ok(())
}

type TurnUpdates =
    tokio::sync::watch::Receiver<std::result::Result<Option<pl_protocol::Turn>, String>>;

struct TurnObserver {
    updates: TurnUpdates,
    _task: tokio_util::task::AbortOnDropHandle<()>,
}

async fn observe_turns(runtime: &StudioRuntime, thread_id: &str) -> Result<TurnObserver> {
    let mut subscription = runtime
        .subscribe_thread(pl_protocol::ThreadSubscriptionRequest {
            thread_id: thread_id.to_string(),
        })
        .await?;
    let (turn_tx, turn_updates) =
        tokio::sync::watch::channel(Ok::<_, String>(None::<pl_protocol::Turn>));
    let task = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        while let Some(update) = subscription.recv().await {
            match update {
                pl_protocol::ThreadSubscriptionUpdate::Snapshot { snapshot } => {
                    if let Some(turn) = snapshot.active_turn {
                        let _ = turn_tx.send_replace(Ok(Some(turn)));
                    }
                }
                pl_protocol::ThreadSubscriptionUpdate::Notification { notification } => {
                    match notification.notification {
                        pl_protocol::ThreadNotification::TurnStarted { turn }
                        | pl_protocol::ThreadNotification::TurnUpdated { turn }
                        | pl_protocol::ThreadNotification::TurnCompleted { turn } => {
                            let _ = turn_tx.send_replace(Ok(Some(turn)));
                        }
                        pl_protocol::ThreadNotification::Lagged { .. } => {
                            let _ = turn_tx.send_replace(Err("root turn observer lagged".into()));
                            break;
                        }
                        pl_protocol::ThreadNotification::ItemStarted { .. }
                        | pl_protocol::ThreadNotification::ItemDelta { .. }
                        | pl_protocol::ThreadNotification::ItemCompleted { .. }
                        | pl_protocol::ThreadNotification::InteractionChanged { .. }
                        | pl_protocol::ThreadNotification::ThreadRuntimeUpdated { .. } => {}
                    }
                }
            }
        }
    }));
    Ok(TurnObserver {
        updates: turn_updates,
        _task: task,
    })
}

async fn drive(
    runtime: &StudioRuntime,
    root: &str,
    workspace: &Path,
    artifacts: &Path,
    actors: &mut BTreeMap<String, Actor>,
    turn_updates: &TurnUpdates,
    request_baseline: usize,
) -> Result<()> {
    let deadline = Instant::now() + std::time::Duration::from_secs(20 * 60);
    let mut injected = false;
    while Instant::now() < deadline {
        ensure!(
            request_capture_count()?.saturating_sub(request_baseline) < 100,
            "live acceptance reached its 100 request limit; no automatic retry"
        );
        let snapshot = runtime.thread_snapshot(root).await?;
        for interaction in snapshot
            .interactions
            .iter()
            .filter(|item| item.status() == InteractionStatus::Pending)
        {
            let checkpoint = matches!(&interaction.content, InteractionContent::UserInput(input)
                if input.questions().iter().any(|question| question.id == "fixture_review_checkpoint"));
            if checkpoint {
                ensure!(!injected, "review checkpoint repeated");
                collect(runtime, root, actors).await?;
                evidence::validate_checkpoint(actors, root)?;
                inject(workspace, artifacts).await?;
                injected = true;
            }
            super::resolve_interaction(runtime, interaction).await?;
        }
        let latest = turn_updates.borrow().clone().map_err(anyhow::Error::msg)?;
        if snapshot
            .runtime
            .as_ref()
            .and_then(|runtime| runtime.workflow.as_ref())
            .and_then(|workflow| workflow.current_run.as_ref())
            .is_some_and(|run| run.lifecycle == pl_protocol::WorkflowRunLifecycle::Terminal)
            && let Some(turn) = latest.as_ref()
            && matches!(turn.state, pl_protocol::TurnState::Completed(_))
        {
            let turn_id = turn.id.as_str();
            ensure!(
                injected,
                "workflow completed without the fault injection checkpoint"
            );
            let complete = snapshot
                .items
                .iter()
                .filter(|item| item.turn_id == turn_id)
                .filter_map(pl_protocol::ThreadItem::tool)
                .find(|tool| tool.invocation().name() == "complete")
                .context("root completed without complete tool evidence")?;
            let ThreadToolState::Succeeded(done) = complete.state() else {
                bail!("root complete tool did not succeed");
            };
            let receipt: serde_json::Value = serde_json::from_str(done.output().result())?;
            ensure!(
                receipt.get("status").and_then(serde_json::Value::as_str) == Some("completed"),
                "root completion receipt is not completed"
            );
            super::write_completion_receipt("mode.task", root, turn_id, &receipt)?;
            return Ok(());
        }
        if let Some(turn) = latest
            && matches!(
                turn.state,
                pl_protocol::TurnState::Failed(_)
                    | pl_protocol::TurnState::Cancelled(_)
                    | pl_protocol::TurnState::BudgetLimited(_)
            )
        {
            bail!("rework root failed before completion: {:?}", turn.state);
        }
        tokio::time::sleep(super::POLL_INTERVAL).await;
    }
    bail!("rework live scenario exceeded 60 minutes")
}

async fn collect(
    runtime: &StudioRuntime,
    root: &str,
    actors: &mut BTreeMap<String, Actor>,
) -> Result<()> {
    // Active snapshots are authoritative and already contain the materialized timeline.
    // Avoid read_state (unrelated services) and cold-history queries during model work.
    let snapshot = runtime
        .thread_snapshot(root)
        .await
        .context("capture root memory snapshot")?;
    let mut identities = Vec::new();
    for tool in snapshot
        .items
        .iter()
        .filter_map(pl_protocol::ThreadItem::tool)
    {
        if tool.invocation().name() != "spawn_agent" {
            continue;
        }
        if let ThreadToolState::Succeeded(done) = tool.state() {
            let receipt: serde_json::Value = serde_json::from_str(done.output().result())?;
            let id = receipt
                .get("agentId")
                .and_then(serde_json::Value::as_str)
                .context("spawn receipt missing agentId")?;
            let role = receipt
                .get("profileId")
                .and_then(serde_json::Value::as_str)
                .context("spawn receipt missing profileId")?;
            identities.push((id.to_string(), role.to_string()));
        }
    }
    capture_into(actors, root.to_string(), "root".into(), snapshot)?;
    for (id, role) in identities {
        let snapshot = runtime
            .thread_snapshot(&id)
            .await
            .with_context(|| format!("capture child memory snapshot {id}"))?;
        capture_into(actors, id, role, snapshot)?;
    }
    Ok(())
}

fn capture_into(
    actors: &mut BTreeMap<String, Actor>,
    id: String,
    role: String,
    mut snapshot: pl_protocol::ThreadSnapshot,
) -> Result<()> {
    if let Some(previous) = actors.get(&id) {
        for item in &previous.snapshot.items {
            if !snapshot.items.iter().any(|current| current.id == item.id) {
                snapshot.items.push(item.clone());
            }
        }
        snapshot.items.sort_by_key(|item| item.ordinal);
    }
    actors.insert(id.clone(), capture_actor(id, role, snapshot)?);
    Ok(())
}

fn capture_actor(id: String, role: String, snapshot: pl_protocol::ThreadSnapshot) -> Result<Actor> {
    let mut calls = Vec::new();
    for item in &snapshot.items {
        let Some(tool) = item.tool() else {
            continue;
        };
        let output = match tool.state() {
            ThreadToolState::Succeeded(state) => state.output().result().to_string(),
            ThreadToolState::Failed(state) => format!("TOOL_FAILED: {:?}", state.failure()),
            ThreadToolState::Denied(state) => format!("TOOL_DENIED: {state:?}"),
            ThreadToolState::Cancelled(state) => format!("TOOL_CANCELLED: {state:?}"),
            ThreadToolState::Started(_)
            | ThreadToolState::Streaming(_)
            | ThreadToolState::AwaitingApproval(_)
            | ThreadToolState::Approved(_)
            | ThreadToolState::Running(_) => continue,
        };
        calls.push(Call {
            id: tool.invocation().tool_call_id().to_string(),
            turn_id: item.turn_id.clone(),
            completed_at: tool
                .state()
                .terminal_at()
                .context("completed tool missing timestamp")?,
            name: tool.invocation().name().to_string(),
            arguments: serde_json::from_str(tool.invocation().arguments())?,
            output,
        });
    }
    Ok(Actor {
        id,
        role,
        calls,
        snapshot,
    })
}

async fn inject(workspace: &Path, artifacts: &Path) -> Result<()> {
    let path = workspace.join("src/normalize.rs");
    let original = fs::read_to_string(&path)?;
    let function = original
        .find("pub fn normalize_key(")
        .context("normalize_key implementation missing")?;
    let signature = &original[function..];
    let parameter = signature
        .split_once('(')
        .context("normalize signature missing (")?
        .1
        .split_once(':')
        .context("normalize signature missing parameter")?
        .0
        .trim();
    ensure!(
        !parameter.is_empty()
            && parameter
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "unsupported normalize parameter"
    );
    let body = function + signature.find('{').context("normalize body missing")? + 1;
    let fault = format!(
        "\n    if {parameter} == \"Cache--Edge\" {{\n        return Ok(\"cache--edge\".to_string());\n    }}\n"
    );
    let mut changed = original.clone();
    changed.insert_str(body, &fault);
    fs::write(&path, &changed)?;
    let tests = workspace.join("tests/normalize.rs");
    let mut regression =
        fs::read_to_string(&tests).context("original executor did not deliver normalize tests")?;
    regression.push_str("\n#[test]\nfn review_checkpoint_separator_regression() {\n    assert_eq!(\n        workflow_live_fixture::normalize::normalize_key(\"Cache--Edge\"),\n        Ok(\"cache-edge\".to_string()),\n    );\n}\n");
    fs::write(tests, regression)?;
    fs::write(
        artifacts.join("injection.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "path": path, "beforeSha256": format!("{:x}", Sha256::digest(original.as_bytes())),
            "afterSha256": format!("{:x}", Sha256::digest(changed.as_bytes())),
            "defect": "internal separator run incorrectly retained", "input": "Cache--Edge", "expected": "cache-edge"
        }))?,
    )?;
    let output = tokio::process::Command::new("cargo")
        .args([
            "test",
            "--test",
            "normalize",
            "review_checkpoint_separator_regression",
        ])
        .current_dir(workspace)
        .kill_on_drop(true)
        .output()
        .await?;
    fs::write(
        artifacts.join("harness-injected-test.stdout.log"),
        &output.stdout,
    )?;
    fs::write(
        artifacts.join("harness-injected-test.stderr.log"),
        &output.stderr,
    )?;
    ensure!(
        !output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .contains("review_checkpoint_separator_regression ... FAILED"),
        "injected defect did not produce the expected assertion failure"
    );
    // Record injected integration changes in a Git fixture so rework can synchronize cleanly.
    if workspace.join(".git").exists() {
        command(
            workspace,
            artifacts,
            "injection-add",
            "git",
            &["add", "src/normalize.rs", "tests/normalize.rs"],
        )
        .await?;
        command(
            workspace,
            artifacts,
            "injection-commit",
            "git",
            &[
                "-c",
                "user.name=Workflow Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "-m",
                "test: inject review boundary regression",
            ],
        )
        .await?;
    }
    Ok(())
}

async fn command(
    workspace: &Path,
    artifacts: &Path,
    label: &str,
    program: &str,
    args: &[&str],
) -> Result<String> {
    let output = tokio::process::Command::new(program)
        .args(args)
        .current_dir(workspace)
        .kill_on_drop(true)
        .output()
        .await?;
    fs::write(
        artifacts.join(format!("{label}.stdout.log")),
        &output.stdout,
    )?;
    fs::write(
        artifacts.join(format!("{label}.stderr.log")),
        &output.stderr,
    )?;
    fs::write(
        artifacts.join(format!("{label}.command.json")),
        serde_json::to_vec_pretty(
            &serde_json::json!({"actor":"harness", "program":program, "args":args, "cwd":workspace, "exitCode":output.status.code()}),
        )?,
    )?;
    ensure!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn request_capture_count() -> Result<usize> {
    let directory = std::env::var_os("PURE_STUDIO_WIRE_CAPTURE_DIR")
        .context("request budget requires wire capture")?;
    if !Path::new(&directory).exists() {
        return Ok(0);
    }
    Ok(fs::read_dir(directory)?
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.ends_with("-full.json") || name.ends_with("-incremental.json")
        })
        .count())
}

fn usage_captures() -> Result<std::collections::BTreeSet<std::path::PathBuf>> {
    let Some(directory) = std::env::var_os("PURE_STUDIO_WIRE_CAPTURE_DIR") else {
        return Ok(Default::default());
    };
    fs::read_dir(directory)?
        .filter_map(|entry| match entry {
            Ok(entry) if entry.file_name().to_string_lossy().starts_with("usage-") => {
                Some(Ok(entry.path()))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error.into())),
        })
        .collect()
}

fn save_cache_usage(
    before: &std::collections::BTreeSet<std::path::PathBuf>,
    artifacts: &Path,
) -> Result<()> {
    let after = usage_captures()?;
    let mut records = Vec::new();
    for path in after.difference(before) {
        let raw: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
        let cached = [
            "/usage/input_tokens_details/cached_tokens",
            "/usage/prompt_tokens_details/cached_tokens",
            "/usage/prompt_cache_hit_tokens",
        ]
        .iter()
        .find_map(|pointer| raw.pointer(pointer).and_then(serde_json::Value::as_u64));
        records.push(serde_json::json!({
            "source":path, "model":raw.get("model"), "responseId":raw.get("responseId"),
            "reportedCacheReadTokens":cached,
            "observability":if cached.is_some() { "reported" } else { "notObservable" },
            "raw":raw
        }));
    }
    fs::write(
        artifacts.join("cache-usage.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "scope":"all provider responses during this isolated scenario; not an A/B savings estimate",
            "observability":if records.is_empty() { "notObservable" } else { "seeEachRecord" }, "records":records
        }))?,
    )?;
    Ok(())
}
