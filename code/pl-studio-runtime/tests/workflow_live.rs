//! 统一 workflow 的真实 provider 验收。
//!
//! 该测试只在显式 `--features live-tests -- --ignored` 时运行。它使用隔离的
//! Studio home 和没有 `.git` 的临时项目，验证模式 Skill、通用交互、工作流终态
//! 以及关机后重新打开 Thread 的持久化事实。

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use pl_protocol::{
    InteractionContent, InteractionResolution, InteractionStatus, ToolApprovalResolution,
    ToolApprovalResolutionPayload, UserInputAnswer, UserInputResolution,
};
use pl_studio_runtime::{
    ConfigPaths, ConfigStore, STUDIO_CONFIG_SCHEMA_VERSION, StudioConfig, StudioHostKind,
    StudioMode, StudioRuntime, StudioRuntimeOptions, StudioSubmitPromptOptions,
    StudioSubmitPromptRequest,
};

const LIVE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const POLL_INTERVAL: Duration = Duration::from_millis(200);
const VERIFY_MARKER: &str = "PURE_WORKFLOW_GUI_VERIFY_OK";
const PROMPT: &str = include_str!("../../../test-fixtures/workflow-live/prompt.md");

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "uses the installed Studio model configuration and incurs real model usage"]
async fn installed_config_workflow_mode_delivers_rust_project() -> Result<()> {
    let installed = ConfigStore::default_app()?;
    let installed_config = installed
        .load()
        .context("installed Studio config is required for workflow live acceptance")?;
    ensure!(
        installed_config.schema_version == STUDIO_CONFIG_SCHEMA_VERSION,
        "installed config schema is {}, expected {}",
        installed_config.schema_version,
        STUDIO_CONFIG_SCHEMA_VERSION
    );
    let route = installed_config.resolve_role(pl_studio_runtime::StudioRole::Planner)?;
    let base_url = route.endpoint.base_url.to_ascii_lowercase();
    ensure!(
        !["localhost", "127.0.0.1", "[::1]", "0.0.0.0"]
            .iter()
            .any(|host| base_url.contains(host)),
        "workflow live acceptance cannot use a local/scripted endpoint"
    );
    ensure!(
        route.endpoint.bearer_token.is_some(),
        "planner route has no credential resolved by the system credential store"
    );

    let root = tempfile::Builder::new()
        .prefix("pure-workflow-live-")
        .tempdir()?;
    let studio_home = root.path().join("studio-home");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&studio_home)?;
    copy_directory(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-fixtures/workflow-live/workspace")
            .as_path(),
        &workspace,
    )?;
    write_isolated_config(&installed, &studio_home, &installed_config)?;

    let runtime = StudioRuntime::with_options(StudioRuntimeOptions {
        studio_home: Some(studio_home.clone()),
        host: StudioHostKind::Desktop,
    })
    .await
    .map_err(anyhow::Error::new)?;
    runtime.start_runtime().await?;
    let project = runtime.open_project(&workspace).await?;
    let thread = runtime
        .create_thread(&project.id, "Workflow live acceptance")
        .await?;
    runtime
        .set_thread_mode(&thread.id, StudioMode::new("mode.task")?)
        .await?;
    runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: thread.id.clone(),
            input: pl_protocol::studio::StudioPromptInput {
                text: PROMPT.trim().to_string(),
                attachment_draft_ids: Vec::new(),
            },
            options: StudioSubmitPromptOptions::default(),
        })
        .await?;

    wait_for_terminal_workflow(&runtime, &thread.id).await?;
    let snapshot = runtime.thread_snapshot(&thread.id).await?;
    let workflow = snapshot
        .runtime
        .as_ref()
        .and_then(|runtime| runtime.workflow.as_ref())
        .context("workflow runtime projection is missing")?;
    let run = workflow
        .current_run
        .as_ref()
        .context("workflow run is missing")?;
    ensure!(
        run.current_stage_id == "completed",
        "workflow ended at `{}`",
        run.current_stage_id
    );
    let stages = run
        .history_tail
        .iter()
        .map(|transition| transition.from_stage_id.as_str())
        .chain(std::iter::once(run.current_stage_id.as_str()))
        .collect::<Vec<_>>();
    for expected in [
        "planning",
        "awaiting_confirmation",
        "editing_documents",
        "working",
        "reviewing",
        "completed",
    ] {
        ensure!(
            stages.contains(&expected),
            "workflow history is missing stage `{expected}`: {stages:?}"
        );
    }
    validate_fixture(&workspace).await?;

    runtime.shutdown_runtime().await?;
    let reopened = StudioRuntime::with_options(StudioRuntimeOptions {
        studio_home: Some(studio_home),
        host: StudioHostKind::Desktop,
    })
    .await
    .map_err(anyhow::Error::new)?;
    reopened.start_runtime().await?;
    let restored = reopened.thread_snapshot(&thread.id).await?;
    let restored_workflow = restored
        .runtime
        .as_ref()
        .and_then(|runtime| runtime.workflow.as_ref())
        .context("reopened Thread lost workflow projection")?;
    ensure!(
        restored_workflow.revision == workflow.revision
            && restored_workflow
                .current_run
                .as_ref()
                .is_some_and(|run| run.run_id == workflow.current_run.as_ref().unwrap().run_id),
        "reopened workflow identity or revision changed"
    );
    reopened.shutdown_runtime().await?;
    Ok(())
}

async fn wait_for_terminal_workflow(runtime: &StudioRuntime, thread_id: &str) -> Result<()> {
    let deadline = Instant::now() + LIVE_TIMEOUT;
    while Instant::now() < deadline {
        let snapshot = runtime.thread_snapshot(thread_id).await?;
        for interaction in snapshot
            .interactions
            .iter()
            .filter(|interaction| interaction.status() == InteractionStatus::Pending)
        {
            resolve_interaction(runtime, interaction).await?;
        }
        if snapshot
            .runtime
            .as_ref()
            .and_then(|runtime| runtime.workflow.as_ref())
            .and_then(|workflow| workflow.current_run.as_ref())
            .is_some_and(|run| {
                run.lifecycle == pl_protocol::WorkflowRunLifecycle::Terminal
                    && run.current_stage_id == "completed"
            })
        {
            return Ok(());
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    bail!("workflow live acceptance exceeded 30 minutes")
}

async fn resolve_interaction(
    runtime: &StudioRuntime,
    interaction: &pl_protocol::InteractionRequest,
) -> Result<()> {
    let resolution = match &interaction.content {
        InteractionContent::UserInput(input) => {
            let answers = input
                .questions()
                .iter()
                .map(|question| {
                    let answer = question
                        .options
                        .as_ref()
                        .and_then(|options| options.first())
                        .map(|option| option.label.clone())
                        .unwrap_or_else(|| "confirm".to_string());
                    (
                        question.id.clone(),
                        UserInputAnswer {
                            answers: vec![answer],
                        },
                    )
                })
                .collect::<HashMap<_, _>>();
            InteractionResolution::UserInput(UserInputResolution { answers })
        }
        InteractionContent::ToolApproval(_) => {
            InteractionResolution::ToolApproval(ToolApprovalResolutionPayload {
                decision: ToolApprovalResolution::Approved,
                reason: Some("workflow live acceptance".to_string()),
            })
        }
    };
    runtime
        .resolve_interaction(interaction.interaction_id.clone(), resolution)
        .await?;
    Ok(())
}

async fn validate_fixture(workspace: &Path) -> Result<()> {
    ensure!(
        !workspace.join(".git").exists(),
        "workflow fixture must not initialize Git"
    );
    ensure!(
        !workspace.join(".pure").exists(),
        "workflow fixture must not create .pure state"
    );
    for path in [
        "src/normalize.rs",
        "src/validate.rs",
        "design/task-workflows.md",
    ] {
        ensure!(
            workspace.join(path).is_file(),
            "workflow did not create `{path}`"
        );
    }
    let mut tests = tokio::process::Command::new("cargo");
    tests.args(["test"]).current_dir(workspace);
    let status = tests.status().await?;
    ensure!(status.success(), "fixture cargo test failed");
    let output = tokio::process::Command::new("cargo")
        .args(["run", "--quiet", "--bin", "fixture_verify"])
        .current_dir(workspace)
        .output()
        .await?;
    ensure!(output.status.success(), "fixture verifier failed");
    ensure!(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line.trim() == VERIFY_MARKER),
        "fixture verifier did not print `{VERIFY_MARKER}`"
    );
    Ok(())
}

fn write_isolated_config(
    installed: &ConfigStore,
    studio_home: &Path,
    config: &StudioConfig,
) -> Result<()> {
    let source = fs::read_to_string(installed.paths().config_file())?;
    let mut table = source.parse::<toml::Table>()?;
    table.insert(
        "schema_version".to_string(),
        toml::Value::Integer(i64::from(STUDIO_CONFIG_SCHEMA_VERSION)),
    );
    let models = table
        .get_mut("models")
        .and_then(toml::Value::as_table_mut)
        .context("installed config has no models table")?;
    let providers = models
        .get_mut("providers")
        .and_then(toml::Value::as_table_mut)
        .context("installed config has no provider table")?;
    for (_, provider) in providers.iter_mut() {
        if let Some(provider) = provider.as_table_mut() {
            ensure!(
                provider.remove("bearer_token").is_none(),
                "inline provider credentials are not allowed in live acceptance"
            );
        }
    }
    ensure!(
        config
            .resolve_role(pl_studio_runtime::StudioRole::Planner)?
            .endpoint
            .bearer_token
            .is_some(),
        "planner credential missing"
    );
    fs::create_dir_all(studio_home)?;
    fs::write(
        ConfigPaths::from_config_dir(studio_home).config_file(),
        toml::to_string_pretty(&table)?,
    )?;
    Ok(())
}

fn copy_directory(source: &Path, target: &Path) -> Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&target_path)?;
            copy_directory(&source_path, &target_path)?;
        } else {
            fs::copy(source_path, target_path)?;
        }
    }
    Ok(())
}
