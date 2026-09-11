//! Small complete GUI acceptance; all artifacts survive failures for inspection.
use super::*;
use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement};
use serde_json::Value;

pub(super) fn run() -> Result<()> {
    let root = paths::workspace_root()?;
    let artifacts = root
        .join("target/workflow-live-artifacts")
        .join(format!("gui-minimal-{}", unix_nanos()));
    let home = artifacts.join("home");
    // A nested directory would inherit this repository's Git root during project resolution.
    let workspace = tempfile::Builder::new()
        .prefix("pure-workflow-minimal-")
        .tempdir()?
        .keep();
    fs::create_dir_all(&artifacts)?;
    fs::write(
        artifacts.join("workspace-path.txt"),
        workspace.to_string_lossy().as_bytes(),
    )?;
    let wire = artifacts.join("wire");
    for directory in [&home, &workspace, &wire] {
        fs::create_dir_all(directory)?;
    }
    println!("Workflow live artifacts: {}", artifacts.display());
    let installed = current_home()?.join(".pure");
    let before = user_config_state(&installed)?;
    let mut source: toml::Table = fs::read_to_string(installed.join("config.toml"))?.parse()?;
    upgrade_live_config_copy(&mut source)?;
    let original: pl_studio_runtime::config::StudioConfig =
        toml::Value::Table(source).try_into()?;
    original.validate()?;
    let mut config = pl_studio_runtime::config::StudioConfig::default_config();
    let id = pl_studio_runtime::config::ProviderId::new("deepseek")?;
    let provider = original
        .models
        .providers
        .get(&id)
        .context("installed deepseek provider is required")?
        .clone();
    config.models.providers.insert(id, provider);
    config.validate()?;
    fs::write(home.join("config.toml"), toml::to_string_pretty(&config)?)?;
    let prompt = root.join("test-fixtures/workflow-live/minimal-prompt.md");
    fs::copy(&prompt, artifacts.join("prompt.md"))?;
    let acceptance = run_gui_attempt(GuiAttempt {
        workspace_root: &root,
        artifact_dir: &artifacts,
        wire_dir: &wire,
        studio_home: &home,
        fixture_workspace: &workspace,
        prompt: &prompt,
        mode: "new",
        attempt: 1,
        studio_mode: "mode.task",
        scope: WorkflowAcceptanceScope::Minimal,
        deadline: Instant::now() + Duration::from_secs(20 * 60),
    });
    let diagnostics = write_metrics(&home, &wire, &artifacts, acceptance.is_ok());
    let unchanged = user_config_state(&installed)? == before;
    fs::write(
        artifacts.join("installed-config-unchanged.txt"),
        unchanged.to_string(),
    )?;
    if let Err(error) = &acceptance {
        fs::write(artifacts.join("acceptance-error.txt"), format!("{error:#}"))?;
    }
    acceptance?;
    diagnostics?;
    ensure!(
        unchanged,
        "installed configuration changed during acceptance"
    );
    ensure!(
        !fs::read_dir(&home)?
            .any(|entry| entry.is_ok_and(|e| e.file_name().to_string_lossy().contains("rejected"))),
        "GUI rejected the isolated config"
    );
    ensure!(
        fs::read(workspace.join("hello.txt"))? == b"hello\n",
        "incorrect hello.txt bytes"
    );
    let result = Command::new("python3").args(["-c", "from pathlib import Path; assert Path('hello.txt').read_bytes() == b'hello\\n'; print('PURE_MINIMAL_VERIFY_OK')"]).current_dir(&workspace).output()?;
    fs::write(artifacts.join("independent-python.log"), &result.stdout)?;
    ensure!(
        result.status.success(),
        "independent Python assertion failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let mut files = Vec::new();
    collect_relative_files(&workspace, &workspace, &mut files)?;
    ensure!(
        files
            .iter()
            .all(|f| f == "hello.txt" || f.starts_with("target/pure/")),
        "unexpected deliverables: {files:?}"
    );
    fs::write(
        artifacts.join("workspace-files.json"),
        serde_json::to_vec_pretty(&files)?,
    )?;
    Ok(())
}

fn write_metrics(home: &Path, wire: &Path, artifacts: &Path, driver_passed: bool) -> Result<()> {
    let mut captures = Vec::new();
    for entry in fs::read_dir(wire)? {
        let path = entry?.path();
        if !path.to_string_lossy().ends_with("-full.json") {
            continue;
        }
        let capture: Value = serde_json::from_slice(&fs::read(&path)?)?;
        ensure!(
            capture.pointer("/wireBody/model").and_then(Value::as_str) == Some("deepseek-flash")
                && capture
                    .pointer("/wireBody/reasoning_effort")
                    .and_then(Value::as_str)
                    == Some("high"),
            "unexpected live model route"
        );
        captures.push(
            path.file_name()
                .context("capture has no name")?
                .to_string_lossy()
                .into_owned(),
        );
    }
    let (rows, performance) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let db = Database::connect(format!(
                "sqlite://{}?mode=ro",
                home.join("studio/sessions.sqlite").display()
            ))
            .await?;
            let rows = db.query_all_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT envelope FROM session_entries ORDER BY session_id, ordinal",
            ))
            .await?;
            let studio = Database::connect(format!("sqlite://{}?mode=ro", home.join("studio/studio.sqlite").display())).await?;
            let costs = studio.query_all_raw(Statement::from_string(DatabaseBackend::Sqlite, "SELECT payload_json FROM studio_objects WHERE object_kind = 'modelPerformance'")).await?;
            Ok::<_, sea_orm::DbErr>((rows, costs))
        })?;
    let mut rejections = Vec::new();
    let mut turns = std::collections::BTreeMap::new();
    let mut attempts = std::collections::BTreeMap::new();
    for row in rows {
        let envelope: Value = serde_json::from_str(&row.try_get::<String>("", "envelope")?)?;
        let payload: Value = serde_json::from_str(
            envelope["payload"]
                .as_str()
                .context("missing commit payload")?,
        )?;
        if let Some(attempt) = payload.get("attempt").filter(|v| !v.is_null()) {
            attempts.insert(
                (
                    payload["threadId"].to_string(),
                    attempt["attemptId"].to_string(),
                ),
                attempt.clone(),
            );
        }
        if payload
            .pointer("/attempt/outcome/kind")
            .and_then(Value::as_str)
            == Some("rejected")
        {
            rejections.push(payload["attempt"].clone());
        }
        if let Some(turn) = payload.get("turn").filter(|v| !v.is_null()) {
            turns.insert(
                (payload["threadId"].to_string(), turn["turnId"].to_string()),
                turn.clone(),
            );
        }
    }
    let mut usage = std::collections::BTreeMap::new();
    for attempt in attempts.values() {
        let outcome = &attempt["outcome"];
        let output = if outcome["kind"] == "committed" {
            &outcome["value"]
        } else {
            &outcome["value"]["output"]
        };
        for field in [
            "inputTokens",
            "outputTokens",
            "cacheReadTokens",
            "cacheWriteTokens",
            "reasoningTokens",
        ] {
            *usage.entry(field).or_insert(0_u64) += output["usage"][field].as_u64().unwrap_or(0);
        }
    }
    let mut cost_snapshots = Vec::new();
    for row in performance {
        let snapshot: Value = serde_json::from_str(&row.try_get::<String>("", "payload_json")?)?;
        cost_snapshots.push(snapshot);
    }
    let metrics = serde_json::json!({"driverPassed": driver_passed, "modelResponseClass": if rejections.is_empty() { "clean" } else { "corrected" }, "modelAttempts": attempts.len(), "usage": usage, "costSnapshots": cost_snapshots, "wireRequests": captures.len(), "captures": captures, "rejections": rejections, "turns": turns.values().collect::<Vec<_>>()});
    fs::write(
        artifacts.join("metrics.json"),
        serde_json::to_vec_pretty(&metrics)?,
    )?;
    ensure!(
        metrics["wireRequests"].as_u64().unwrap_or(0) > 0,
        "no real model requests"
    );
    Ok(())
}
