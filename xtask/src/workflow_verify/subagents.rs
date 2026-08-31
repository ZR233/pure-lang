//! Real native-GUI acceptance for directory and worktree child Agents.

use super::{
    copy_directory, current_home, resident, unix_nanos, user_config_state,
    write_isolated_live_config,
};
use crate::cli::VerifySubagentsOptions;
use crate::{paths, process};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const TOTAL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const STALL_TIMEOUT_SECONDS: u64 = 10 * 60;
const SENTINEL: &str = "PURE_SUBAGENTS_LIVE_OK";

#[derive(Debug, Clone)]
struct Route {
    provider: String,
    model: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireReceipt {
    schema_version: u32,
    capture_count: usize,
    calls: Vec<WireCall>,
    captures: Vec<CaptureReceipt>,
    output_markers: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireCall {
    #[serde(skip_serializing_if = "Option::is_none")]
    call_id: Option<String>,
    name: String,
    arguments: Value,
}

#[derive(Debug, Clone, Serialize)]
struct WireOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    call_id: Option<String>,
    content: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureReceipt {
    path: String,
    actor: String,
    calls: Vec<WireCall>,
}

const PROFILE_FIRST_LINES: &[(&str, &str)] = &[
    (
        "explorer",
        "你当前是父 Agent 派出的只读 explorer，只负责在指定范围内收集事实并汇报。你使用 fresh",
    ),
    (
        "executor",
        "你是父 Agent 按冻结 Agent Profile 派出的 executor。你在 directory assignment 中工作，且不继承",
    ),
    (
        "worktree_executor",
        "你是 Worktree 执行者。只在宿主分配的独立 Git worktree 中完成边界明确的实现任务；适用于",
    ),
    (
        "reviewer",
        "你是父 Agent 按冻结 Agent Profile 派出的 reviewer。你必须使用新建的 fresh context",
    ),
];

pub(super) fn run(options: VerifySubagentsOptions) -> Result<()> {
    ensure!(
        options.live && options.gui,
        "verify-subagents requires --live --gui because it uses real credentials, incurs model fees, and validates the native GUI"
    );
    let deadline = Instant::now() + TOTAL_TIMEOUT;
    let workspace_root = paths::workspace_root()?;
    let artifact_dir = workspace_root
        .join("target")
        .join("subagents-live-artifacts")
        .join(format!("gui-{}-{}", std::process::id(), unix_nanos()));
    let wire_dir = artifact_dir.join("wire");
    fs::create_dir_all(&wire_dir)?;
    println!("Subagents live artifacts: {}", artifact_dir.display());

    let installed_home = current_home()?.join(".pure");
    let installed_config = installed_home.join("config.toml");
    ensure!(
        installed_config.is_file(),
        "installed Studio config is required for live GUI acceptance: {}",
        installed_config.display()
    );
    let installed_state_before = user_config_state(&installed_home)?;
    let root = tempfile::Builder::new()
        .prefix("pure-subagents-live-gui-")
        .tempdir()
        .context("failed to create isolated subagents acceptance root")?;
    let studio_home = root.path().join("studio-home");
    let fixture = root.path().join("workspace");
    fs::create_dir_all(&studio_home)?;
    prepare_fixture(&fixture)?;
    let isolated_config = studio_home.join("config.toml");
    write_isolated_live_config(
        &installed_config,
        &isolated_config,
        &artifact_dir.join("model-routes.json"),
    )?;
    disable_all_live_profiles(&isolated_config)?;
    let executor = read_route(&isolated_config, "executor")?;
    let worktree_executor = read_route(&isolated_config, "worktree_executor")?;
    let explorer = read_route(&isolated_config, "explorer")?;
    let reviewer = read_route(&isolated_config, "reviewer")?;
    let installed_agents = installed_home.join("agents");
    if installed_agents.is_dir() {
        copy_directory(&installed_agents, &studio_home.join("agents"))?;
    }
    let prompt = workspace_root
        .join("test-fixtures")
        .join("subagents-live")
        .join("prompt.md");
    fs::copy(&prompt, artifact_dir.join("fixture-prompt.md"))?;
    fs::write(
        artifact_dir.join("acceptance-surface.txt"),
        "surface=gui\nscriptedProvider=false\nlive=true\nfixture=isolatedGit\n",
    )?;

    let acceptance = run_gui(GuiRun {
        workspace_root: &workspace_root,
        artifact_dir: &artifact_dir,
        wire_dir: &wire_dir,
        studio_home: &studio_home,
        fixture: &fixture,
        prompt: &prompt,
        executor: &executor,
        worktree_executor: &worktree_executor,
        explorer: &explorer,
        reviewer: &reviewer,
        deadline,
    })
    .and_then(|_| validate_fixture(&fixture, &artifact_dir))
    .and_then(|_| validate_wire(&wire_dir, &artifact_dir));

    let installed_state_after = user_config_state(&installed_home)?;
    fs::write(
        artifact_dir.join("installed-user-state.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schemaVersion": 1,
            "before": installed_state_before,
            "after": installed_state_after,
            "unchanged": installed_state_before == installed_state_after,
        }))?,
    )?;
    ensure!(
        installed_state_before == installed_state_after,
        "installed ~/.pure config or Agent files changed during isolated acceptance"
    );
    match acceptance {
        Ok(()) => {
            fs::write(artifact_dir.join("result.txt"), "completed\n")?;
            println!(
                "Subagents live acceptance completed: {}",
                artifact_dir.display()
            );
            Ok(())
        }
        Err(error) => {
            fs::write(
                artifact_dir.join("acceptance-error.txt"),
                format!("{error:#}\n"),
            )?;
            Err(error.context(format!(
                "subagents acceptance artifacts were preserved at {}",
                artifact_dir.display()
            )))
        }
    }
}

struct GuiRun<'a> {
    workspace_root: &'a Path,
    artifact_dir: &'a Path,
    wire_dir: &'a Path,
    studio_home: &'a Path,
    fixture: &'a Path,
    prompt: &'a Path,
    executor: &'a Route,
    worktree_executor: &'a Route,
    explorer: &'a Route,
    reviewer: &'a Route,
    deadline: Instant,
}

fn run_gui(run: GuiRun<'_>) -> Result<()> {
    let mut command = Command::new("cargo");
    command
        .args(["xtask", "run-gui", "--driver", "--log-level", "debug"])
        .current_dir(run.workspace_root)
        .env("PURE_STUDIO_HOME", run.studio_home)
        .env("PURE_STUDIO_WIRE_CAPTURE_DIR", run.wire_dir)
        .env(
            "PURE_STUDIO_NATIVE_LIFECYCLE_LOG",
            run.artifact_dir.join("native-lifecycle.log"),
        );
    let mut gui = resident::ResidentProcess::start(
        &mut command,
        &run.artifact_dir.join("gui.stdout.log"),
        &run.artifact_dir.join("gui.stderr.log"),
    )?;
    let acceptance = (|| {
        let remaining = run.deadline.saturating_duration_since(Instant::now());
        ensure!(!remaining.is_zero(), "subagents GUI acceptance timed out");
        let vm_service = gui.wait_for_vm_service(remaining)?;
        fs::write(
            run.artifact_dir.join("vm-service.txt"),
            format!("{vm_service}\n"),
        )?;
        let args = driver_args(&run, &vm_service);
        let display = process::display_command("dart", &args);
        let mut driver = process::path_command("dart", &args);
        driver.current_dir(paths::studio_app_dir(run.workspace_root));
        let driver_stdout = run.artifact_dir.join("driver.stdout.log");
        resident::run_logged_with_timeout(
            &mut driver,
            &display,
            &driver_stdout,
            &run.artifact_dir.join("driver.stderr.log"),
            run.deadline.saturating_duration_since(Instant::now()),
        )?;
        write_driver_receipt(
            &driver_stdout,
            &run.artifact_dir.join("terminal-receipt.json"),
        )
    })();
    let process_tree = gui.write_process_tree(&run.artifact_dir.join("last-process-tree.txt"));
    let cleanup = gui.stop();
    match acceptance {
        Err(error) => {
            let mut context = Vec::new();
            if let Err(extra) = process_tree {
                context.push(format!("process tree artifact failed: {extra:#}"));
            }
            if let Err(extra) = cleanup {
                context.push(format!("GUI cleanup failed: {extra:#}"));
            }
            if context.is_empty() {
                Err(error)
            } else {
                Err(error.context(context.join("; ")))
            }
        }
        Ok(()) => {
            process_tree?;
            cleanup
        }
    }
}

fn driver_args(run: &GuiRun<'_>, vm_service: &str) -> Vec<OsString> {
    let mut args = [
        "run",
        "test_driver/subagents_acceptance_driver.dart",
        "--vm-service-url",
        vm_service,
        "--workspace",
    ]
    .into_iter()
    .map(OsString::from)
    .collect::<Vec<_>>();
    args.push(run.fixture.as_os_str().to_owned());
    args.push(OsString::from("--prompt-file"));
    args.push(run.prompt.as_os_str().to_owned());
    args.push(OsString::from("--settings-screenshot"));
    args.push(
        run.artifact_dir
            .join("agents-settings.png")
            .into_os_string(),
    );
    args.push(OsString::from("--final-screenshot"));
    args.push(run.artifact_dir.join("terminal.png").into_os_string());
    for (name, value) in [
        ("--executor-provider", run.executor.provider.as_str()),
        ("--executor-model", run.executor.model.as_str()),
        (
            "--worktree-provider",
            run.worktree_executor.provider.as_str(),
        ),
        ("--worktree-model", run.worktree_executor.model.as_str()),
        ("--explorer-provider", run.explorer.provider.as_str()),
        ("--explorer-model", run.explorer.model.as_str()),
        ("--reviewer-provider", run.reviewer.provider.as_str()),
        ("--reviewer-model", run.reviewer.model.as_str()),
        ("--timeout-seconds", "1800"),
    ] {
        args.push(OsString::from(name));
        args.push(OsString::from(value));
    }
    args.push(OsString::from("--stall-timeout-seconds"));
    args.push(OsString::from(STALL_TIMEOUT_SECONDS.to_string()));
    args
}

fn write_driver_receipt(log: &Path, output: &Path) -> Result<()> {
    let mut completed = None;
    let mut shutdown = None;
    for line in fs::read_to_string(log)?.lines() {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if record.get("result").and_then(Value::as_str) == Some("completed") {
            completed = Some(record.clone());
        }
        if record.get("event").and_then(Value::as_str) == Some("studioShutdownCompleted") {
            shutdown = Some(record);
        }
    }
    let completed = completed.context("Flutter Driver emitted no completed receipt")?;
    let shutdown = shutdown.context("Flutter Driver emitted no shutdown receipt")?;
    let rendered = serde_json::to_string(&completed)?;
    ensure!(
        rendered.contains(SENTINEL),
        "Flutter Driver receipt does not contain {SENTINEL}"
    );
    validate_driver_snapshot(&completed)?;
    fs::write(
        output,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schemaVersion": 1,
            "completed": completed,
            "shutdown": shutdown,
        }))?,
    )?;
    Ok(())
}

fn validate_driver_snapshot(completed: &Value) -> Result<()> {
    let workspace = completed
        .get("workspace")
        .context("Flutter Driver receipt has no workspace")?;
    ensure!(
        workspace.get("threadMode").and_then(Value::as_str) == Some("mode.task"),
        "Flutter Driver terminal workspace is not mode.task"
    );
    let roles = workspace
        .get("agents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|agent| agent.get("role").and_then(Value::as_str))
        .collect::<std::collections::HashSet<_>>();
    for role in ["explorer", "executor", "worktree_executor", "reviewer"] {
        ensure!(roles.contains(role), "Driver receipt lacks {role} Profile");
    }
    let timeline = workspace
        .get("timeline")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| row.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>();
    ensure!(
        timeline
            .iter()
            .any(|text| text.contains("REVIEWER_READ_ONLY_APPROVED")),
        "Driver receipt lacks reviewer approval marker"
    );
    let workflow = completed
        .get("workflow")
        .context("Driver receipt has no workflow")?;
    let history = workflow
        .get("currentRun")
        .and_then(|run| run.get("history"))
        .and_then(Value::as_array)
        .context("Driver receipt workflow has no history")?;
    ensure!(
        history.iter().any(|entry| {
            entry.get("fromStageId").and_then(Value::as_str) == Some("integrating")
                || entry.get("toStageId").and_then(Value::as_str) == Some("integrating")
        }),
        "Driver receipt workflow history lacks integrating"
    );
    Ok(())
}

fn prepare_fixture(path: &Path) -> Result<()> {
    fs::create_dir_all(path.join("src"))?;
    fs::create_dir_all(path.join("design"))?;
    fs::create_dir_all(path.join("allowed"))?;
    fs::create_dir_all(path.join("forbidden"))?;
    fs::write(
        path.join("Cargo.toml"),
        "[package]\nname = \"subagents-live-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    fs::write(
        path.join("src/lib.rs"),
        "pub fn fixture_ready() -> bool { true }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn fixture_is_ready() { assert!(super::fixture_ready()); }\n}\n",
    )?;
    fs::write(path.join("allowed/.gitkeep"), "")?;
    fs::write(path.join("forbidden/.gitkeep"), "")?;
    fs::write(path.join(".gitignore"), "/target/\n/.pure/\n")?;
    run_git(path, &["init", "-b", "main"])?;
    run_git(path, &["add", "."])?;
    run_git(
        path,
        &[
            "-c",
            "user.name=Pure Acceptance",
            "-c",
            "user.email=pure-acceptance@example.invalid",
            "commit",
            "-m",
            "test: initialize subagents fixture",
        ],
    )?;
    Ok(())
}

fn validate_fixture(fixture: &Path, artifacts: &Path) -> Result<()> {
    ensure!(
        fs::read_to_string(fixture.join("design/subagents-orchestration.md"))?
            .contains("ROOT_DESIGN_MARKER"),
        "root did not create the required design marker"
    );
    ensure!(
        fs::read_to_string(fixture.join("allowed/directory.txt"))? == "directory child accepted\n",
        "directory child output is missing or incorrect"
    );
    ensure!(
        !fixture.join("forbidden/denied.txt").exists(),
        "directory child bypassed writablePaths"
    );
    ensure!(
        fs::read_to_string(fixture.join("worktree_result.txt"))? == "worktree child committed\n",
        "worktree child commit was not integrated"
    );
    let log = run_git(fixture, &["log", "--format=%H%x09%s"])?;
    ensure!(
        log.lines()
            .any(|line| line.ends_with("\tfeat: worktree executor marker")),
        "main Git history has no explicit worktree executor integration"
    );
    let branches = run_git(fixture, &["branch", "--format=%(refname:short)"])?;
    ensure!(
        !branches
            .lines()
            .any(|branch| branch.starts_with("pure-agent-")),
        "Pure-owned worktree branch was not cleaned: {branches}"
    );
    let worktrees = run_git(fixture, &["worktree", "list", "--porcelain"])?;
    ensure!(
        worktrees
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count()
            == 1,
        "child worktree was not cleaned: {worktrees}"
    );
    let status = run_git(fixture, &["status", "--short"])?;
    fs::write(artifacts.join("final-git-status.txt"), &status)?;
    fs::write(artifacts.join("final-git-log.txt"), &log)?;
    fs::write(artifacts.join("final-worktree-list.txt"), &worktrees)?;
    fs::write(
        artifacts.join("final-file-diff.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schemaVersion": 1,
            "files": [
                file_receipt(fixture, "allowed/directory.txt")?,
                file_receipt(fixture, "worktree_result.txt")?,
            ],
            "rejectedPathPresent": false,
        }))?,
    )?;
    let mut tests = Command::new("cargo");
    tests.args(["test"]).current_dir(fixture);
    resident::run_logged(
        &mut tests,
        "final subagents fixture cargo test",
        &artifacts.join("final-cargo-test.stdout.log"),
        &artifacts.join("final-cargo-test.stderr.log"),
    )
}

fn file_receipt(root: &Path, relative: &str) -> Result<Value> {
    let bytes = fs::read(root.join(relative))?;
    Ok(serde_json::json!({
        "path": relative,
        "sha256": format!("{:x}", Sha256::digest(&bytes)),
        "content": String::from_utf8(bytes)?,
    }))
}

fn disable_all_live_profiles(path: &Path) -> Result<()> {
    let mut config = fs::read_to_string(path)?.parse::<toml::Table>()?;
    let mut disabled = config
        .get("disabled_system_agents")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for profile in ["explorer", "executor", "worktree_executor", "reviewer"] {
        if !disabled.iter().any(|value| value.as_str() == Some(profile)) {
            disabled.push(toml::Value::String(profile.to_string()));
        }
    }
    config.insert(
        "disabled_system_agents".to_string(),
        toml::Value::Array(disabled),
    );
    fs::write(path, toml::to_string_pretty(&config)?)?;
    Ok(())
}

fn read_route(path: &Path, role: &str) -> Result<Route> {
    let config = fs::read_to_string(path)?.parse::<toml::Table>()?;
    let route = config
        .get("models")
        .and_then(toml::Value::as_table)
        .and_then(|models| models.get("routes"))
        .and_then(toml::Value::as_table)
        .and_then(|routes| routes.get(role))
        .and_then(toml::Value::as_table)
        .with_context(|| format!("isolated config has no {role} route"))?;
    Ok(Route {
        provider: route
            .get("provider")
            .and_then(toml::Value::as_str)
            .context("route has no provider")?
            .to_string(),
        model: route
            .get("model")
            .and_then(toml::Value::as_str)
            .context("route has no model")?
            .to_string(),
    })
}

fn validate_wire(wire_dir: &Path, artifacts: &Path) -> Result<()> {
    let mut captures = Vec::new();
    collect_json(wire_dir, &mut captures)?;
    ensure!(
        !captures.is_empty(),
        "live acceptance produced no wire captures"
    );
    let mut capture_receipts = Vec::new();
    let mut calls = Vec::new();
    let mut outputs: Vec<WireOutput> = Vec::new();
    for path in &captures {
        let capture: Value = serde_json::from_slice(&fs::read(path)?)?;
        let body = capture
            .get("wireBody")
            .context("wire capture has no body")?;
        let actor = classify_capture(body)?;
        let mut capture_calls = Vec::new();
        collect_calls(body, &mut capture_calls);
        calls.extend(capture_calls.iter().cloned());
        capture_receipts.push(CaptureReceipt {
            path: path.display().to_string(),
            actor,
            calls: capture_calls,
        });
        collect_outputs(body, &mut outputs);
    }
    deduplicate_calls(&mut calls);
    deduplicate_outputs(&mut outputs);
    ensure_spawn_calls(&calls)?;
    ensure_workspace_receipts(&calls, &outputs)?;
    ensure_profile_messages(&calls)?;
    ensure_reviewer_history(&capture_receipts)?;
    ensure_root_history(&capture_receipts)?;
    ensure_submissions(&calls, &outputs)?;
    ensure_finding_re_review(&calls, &outputs)?;
    ensure_orchestration_order(&calls)?;
    ensure!(
        calls.iter().any(|call| {
            call.name == "write_file"
                && call.arguments.get("path").and_then(Value::as_str)
                    == Some("forbidden/denied.txt")
        }),
        "wire captures contain no forbidden built-in write attempt"
    );
    ensure!(
        calls.iter().any(|call| {
            call.name == "exec"
                && call
                    .arguments
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| command.contains("cherry-pick"))
        }),
        "wire captures contain no explicit Git cherry-pick"
    );
    ensure!(
        calls.iter().any(|call| {
            call.name == "close_agent"
                && call
                    .arguments
                    .get("workspaceDisposition")
                    .and_then(Value::as_str)
                    == Some("cleanup")
        }),
        "wire captures contain no explicit worktree cleanup"
    );
    let output = outputs
        .iter()
        .map(|output| output.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for marker in [
        "outside the directory Agent writablePaths boundary",
        "DIRECTORY_DENIAL_OBSERVED",
        "WORKTREE_COMMIT_READY",
        "ROOT_DESIGN_MARKER",
    ] {
        ensure!(
            output.replace(' ', "").contains(&marker.replace(' ', "")),
            "wire tool results contain no `{marker}` receipt"
        );
    }
    fs::write(
        artifacts.join("subagents-wire-receipt.json"),
        serde_json::to_vec_pretty(&WireReceipt {
            schema_version: 1,
            capture_count: captures.len(),
            calls,
            captures: capture_receipts,
            output_markers: vec![
                "directoryRejection",
                "directoryWorkspaceReceipt",
                "worktreeWorkspaceReceipt",
                "explicitCherryPick",
                "explicitCleanup",
            ],
        })?,
    )?;
    Ok(())
}

fn classify_capture(body: &Value) -> Result<String> {
    let rendered = role_text(body);
    let matches = PROFILE_FIRST_LINES
        .iter()
        .filter(|(_, first)| rendered.contains(first))
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    ensure!(
        matches.len() <= 1,
        "wire capture matches multiple built-in Profile identities: {matches:?}"
    );
    if rendered.contains("# Unified Root Agent") {
        ensure!(
            matches.is_empty(),
            "wire capture matches root and built-in Profile identities"
        );
        return Ok("root".to_string());
    }
    Ok(matches.first().copied().unwrap_or("unknown").to_string())
}

fn role_text(value: &Value) -> String {
    fn visit(value: &Value, out: &mut String) {
        match value {
            Value::Array(items) => items.iter().for_each(|item| visit(item, out)),
            Value::Object(object) => {
                let role = object.get("role").and_then(Value::as_str);
                if matches!(role, Some("system" | "developer")) {
                    if let Some(content) = object.get("content") {
                        collect_text(content, out);
                    }
                }
                object.values().for_each(|item| visit(item, out));
            }
            _ => {}
        }
    }
    fn collect_text(value: &Value, out: &mut String) {
        match value {
            Value::String(text) => {
                out.push_str(text);
                out.push('\n');
            }
            Value::Array(items) => items.iter().for_each(|item| collect_text(item, out)),
            Value::Object(object) => object.values().for_each(|item| collect_text(item, out)),
            _ => {}
        }
    }
    let mut out = String::new();
    visit(value, &mut out);
    out
}

fn ensure_profile_messages(calls: &[WireCall]) -> Result<()> {
    let required = ["explorer", "executor", "worktree_executor", "reviewer"];
    let spawns = calls
        .iter()
        .filter(|call| call.name == "spawn_agent")
        .collect::<Vec<_>>();
    for profile in required {
        let profile_spawns = spawns
            .iter()
            .filter(|call| call.arguments.get("profileId").and_then(Value::as_str) == Some(profile))
            .collect::<Vec<_>>();
        ensure!(
            !profile_spawns.is_empty(),
            "wire captures contain no {profile} spawn"
        );
        for spawn in profile_spawns {
            ensure!(
                spawn.arguments.get("forkTurns").and_then(Value::as_str) == Some("none"),
                "{profile} spawn did not freeze forkTurns:none"
            );
            let message = spawn
                .arguments
                .get("message")
                .and_then(Value::as_str)
                .with_context(|| format!("{profile} spawn has no message"))?;
            let markers = [
                "purpose",
                "baseline",
                "ownership",
                "forbidden",
                "steps",
                "completion_failure",
                "evidence",
                "workspace_git_cleanup",
            ];
            let mut cursor = 0;
            for (index, marker) in markers.iter().enumerate() {
                let token = format!("[[CHILD_CONTRACT:{marker}]]");
                let at = message[cursor..]
                    .find(&token)
                    .with_context(|| format!("{profile} message lacks {token}"))?
                    + cursor;
                let start = at + token.len();
                let end = markers
                    .get(index + 1)
                    .and_then(|next| message[start..].find(&format!("[[CHILD_CONTRACT:{next}]]")))
                    .map(|offset| start + offset)
                    .unwrap_or(message.len());
                ensure!(
                    !message[start..end].trim().is_empty(),
                    "{profile} message has empty {token}"
                );
                cursor = end;
            }
        }
    }
    let explorers = spawns
        .iter()
        .filter(|call| call.arguments.get("profileId").and_then(Value::as_str) == Some("explorer"))
        .collect::<Vec<_>>();
    ensure!(
        explorers.len() >= 2,
        "fewer than two explorer spawn messages"
    );
    ensure!(
        explorers[0].arguments.get("message") != explorers[1].arguments.get("message"),
        "two explorer messages do not define distinct purpose/ownership"
    );
    let section = |call: &WireCall, name: &str| -> Result<String> {
        let message = call
            .arguments
            .get("message")
            .and_then(Value::as_str)
            .context("spawn message missing")?;
        let marker = format!("[[CHILD_CONTRACT:{name}]]");
        let start = message
            .find(&marker)
            .map(|i| i + marker.len())
            .context("contract section missing")?;
        let end = message[start..]
            .find("[[CHILD_CONTRACT:")
            .map(|i| start + i)
            .unwrap_or(message.len());
        ensure!(
            !message[start..end].trim().is_empty(),
            "contract section empty"
        );
        Ok(message[start..end].trim().to_string())
    };
    ensure!(
        section(explorers[0], "purpose")? != section(explorers[1], "purpose")?,
        "explorer purposes are not distinct"
    );
    ensure!(
        section(explorers[0], "ownership")? != section(explorers[1], "ownership")?,
        "explorer ownership sections are not distinct"
    );
    ensure!(
        explorers[0].call_id.is_some()
            && explorers[1].call_id.is_some()
            && explorers[0].call_id != explorers[1].call_id,
        "two explorer spawns do not have distinct call IDs"
    );
    Ok(())
}

fn ensure_reviewer_history(captures: &[CaptureReceipt]) -> Result<()> {
    let reviewer = captures
        .iter()
        .filter(|capture| capture.actor == "reviewer")
        .collect::<Vec<_>>();
    ensure!(!reviewer.is_empty(), "no classified Reviewer capture");
    for capture in reviewer {
        for call in &capture.calls {
            ensure!(
                REVIEWER_READ_ONLY.contains(&call.name.as_str()),
                "classified reviewer capture contains non-read-only tool: {}",
                call.name
            );
        }
    }
    Ok(())
}

fn ensure_root_history(captures: &[CaptureReceipt]) -> Result<()> {
    let root = captures
        .iter()
        .find(|capture| capture.actor == "root")
        .context("no classified Root capture")?;
    let has = |name: &str, pred: fn(&WireCall) -> bool| {
        root.calls
            .iter()
            .any(|call| call.name == name && pred(call))
    };
    ensure!(
        has("write_file", |call| call
            .arguments
            .get("path")
            .and_then(Value::as_str)
            == Some("design/subagents-orchestration.md")),
        "root capture lacks design write_file"
    );
    ensure!(
        has("exec", |call| call
            .arguments
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|c| c.contains("cherry-pick"))),
        "root capture lacks cherry-pick exec"
    );
    ensure!(
        has("close_agent", |call| call
            .arguments
            .get("workspaceDisposition")
            .and_then(Value::as_str)
            == Some("cleanup")),
        "root capture lacks cleanup close_agent"
    );
    ensure!(
        has("exec", |call| call
            .arguments
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|c| c.contains("cargo test"))),
        "root capture lacks final cargo test"
    );
    for capture in captures
        .iter()
        .filter(|capture| matches!(capture.actor.as_str(), "explorer" | "reviewer"))
    {
        ensure!(
            !capture.calls.iter().any(|call| call.name == "write_file"
                || call.name == "close_agent"
                || call.name == "exec"
                    && call
                        .arguments
                        .get("command")
                        .and_then(Value::as_str)
                        .is_some_and(|c| c.contains("cherry-pick") || c.contains("cargo test"))),
            "root-only call observed in {} capture",
            capture.actor
        );
    }
    Ok(())
}

const REVIEWER_READ_ONLY: &[&str] = &[
    "read_file",
    "list_files",
    "stat_path",
    "lsp_capabilities",
    "lsp_query",
    "git_status",
    "git_diff",
    "git_workspace_info",
    "read_session_note",
    "search_session_note",
];

#[allow(dead_code)]
fn is_mutating_shell_command(command: &str) -> bool {
    let words = command.split_whitespace().collect::<Vec<_>>();
    command.contains('>')
        || (words.contains(&"sed") && words.iter().any(|word| word.starts_with("-i")))
        || words
            .iter()
            .any(|word| ["tee", "touch", "rm", "mv", "cp"].contains(word))
        || words.windows(2).any(|pair| {
            pair[0] == "git"
                && [
                    "add",
                    "commit",
                    "reset",
                    "checkout",
                    "restore",
                    "clean",
                    "cherry-pick",
                    "merge",
                    "rebase",
                    "rm",
                    "mv",
                ]
                .contains(&pair[1])
        })
        || words.first() == Some(&"git")
            && words.get(1) == Some(&"branch")
            && words.iter().any(|word| *word == "-d" || *word == "-D")
        || words.windows(3).any(|w| w == ["git", "worktree", "remove"])
}

fn bound_receipts(calls: &[WireCall], outputs: &[WireOutput]) -> Result<Vec<(WireCall, Value)>> {
    calls
        .iter()
        .filter(|call| call.name == "spawn_agent")
        .map(|spawn| {
            let id = spawn
                .call_id
                .as_ref()
                .context("spawn_agent has no call_id")?;
            let output = outputs
                .iter()
                .find(|output| output.call_id.as_ref() == Some(id))
                .with_context(|| format!("spawn {id} has no same-call-id output"))?;
            let receipt: Value =
                serde_json::from_str(&output.content).context("spawn output is not JSON")?;
            ensure!(
                receipt.get("profileId") == spawn.arguments.get("profileId"),
                "spawn receipt profileId does not match call args"
            );
            ensure!(
                receipt
                    .get("agentId")
                    .and_then(Value::as_str)
                    .is_some_and(|id| !id.is_empty()),
                "spawn receipt has no agentId"
            );
            Ok((spawn.clone(), receipt))
        })
        .collect()
}

fn ensure_workspace_receipts(calls: &[WireCall], outputs: &[WireOutput]) -> Result<()> {
    let receipts = bound_receipts(calls, outputs)?;
    let directory = receipts
        .iter()
        .map(|(_, receipt)| receipt)
        .find(|receipt| receipt.get("profileId").and_then(Value::as_str) == Some("executor"))
        .context("wire tool results contain no executor workspace receipt")?;
    let workspace = directory
        .get("workspace")
        .and_then(Value::as_object)
        .context("executor spawn receipt has no workspace")?;
    ensure!(
        workspace.get("mode").and_then(Value::as_str) == Some("directory"),
        "executor spawn receipt does not freeze directory mode"
    );
    for profile in ["explorer", "reviewer"] {
        let receipt = receipts
            .iter()
            .map(|(_, receipt)| receipt)
            .find(|receipt| receipt.get("profileId").and_then(Value::as_str) == Some(profile))
            .with_context(|| format!("wire tool results contain no {profile} workspace receipt"))?;
        ensure!(
            receipt
                .get("workspace")
                .and_then(Value::as_object)
                .and_then(|workspace| workspace.get("mode"))
                .and_then(Value::as_str)
                == Some("unrestricted"),
            "{profile} spawn receipt does not freeze unrestricted mode"
        );
    }
    let writable_paths = workspace
        .get("writablePaths")
        .and_then(Value::as_array)
        .context("directory workspace receipt has no canonical writablePaths")?;
    ensure!(
        writable_paths.len() == 1
            && writable_paths[0].as_str().is_some_and(
                |path| Path::new(path).is_absolute() && Path::new(path).ends_with("allowed")
            ),
        "directory workspace receipt does not contain one canonical absolute allowed path"
    );

    let worktree = receipts
        .iter()
        .map(|(_, receipt)| receipt)
        .find(|receipt| {
            receipt.get("profileId").and_then(Value::as_str) == Some("worktree_executor")
        })
        .context("wire tool results contain no worktree_executor workspace receipt")?;
    let workspace = worktree
        .get("workspace")
        .and_then(Value::as_object)
        .context("worktree_executor spawn receipt has no workspace")?;
    ensure!(
        workspace.get("mode").and_then(Value::as_str) == Some("worktree"),
        "worktree_executor spawn receipt does not freeze worktree mode"
    );
    let assignment = workspace
        .get("worktree")
        .and_then(Value::as_object)
        .context("worktree workspace receipt has no assignment")?;
    ensure!(
        assignment
            .get("branch")
            .and_then(Value::as_str)
            .is_some_and(|branch| branch.starts_with("pure-agent-")),
        "worktree workspace receipt has no Pure-owned branch"
    );
    ensure!(
        assignment
            .get("baseCommit")
            .and_then(Value::as_str)
            .is_some_and(
                |commit| commit.len() == 40 && commit.bytes().all(|byte| byte.is_ascii_hexdigit())
            ),
        "worktree workspace receipt has no frozen full base commit"
    );
    Ok(())
}

fn ensure_spawn_calls(calls: &[WireCall]) -> Result<()> {
    let executor = calls.iter().find(|call| {
        call.name == "spawn_agent"
            && call.arguments.get("profileId").and_then(Value::as_str) == Some("executor")
    });
    let executor = executor.context("wire captures contain no executor spawn")?;
    ensure!(
        executor.arguments.get("writablePaths") == Some(&serde_json::json!(["allowed"])),
        "executor spawn did not freeze writablePaths=[allowed]"
    );
    ensure!(
        calls.iter().any(|call| {
            call.name == "spawn_agent"
                && call.arguments.get("profileId").and_then(Value::as_str)
                    == Some("worktree_executor")
                && call.arguments.get("writablePaths").is_none()
        }),
        "wire captures contain no canonical worktree_executor spawn"
    );
    Ok(())
}

fn ensure_orchestration_order(calls: &[WireCall]) -> Result<()> {
    let spawn_indices = |profile: &str| {
        calls
            .iter()
            .enumerate()
            .filter(|(_, call)| {
                call.name == "spawn_agent"
                    && call.arguments.get("profileId").and_then(Value::as_str) == Some(profile)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
    };
    let is_wait = |call: &WireCall| {
        matches!(
            call.name.as_str(),
            "wait_agents" | "read_agent_session" | "read_agent_submissions"
        )
    };
    let explorer_wait = calls
        .iter()
        .position(is_wait)
        .context("wire captures contain no child wait/read operation")?;
    let explorers = spawn_indices("explorer");
    ensure!(
        explorers.len() >= 2,
        "wire captures contain fewer than two explorer spawns"
    );
    ensure!(
        explorers[1] < explorer_wait,
        "explorer spawns were not both issued before the first wait/read"
    );
    let executors = spawn_indices("executor");
    let worktrees = spawn_indices("worktree_executor");
    ensure!(
        !executors.is_empty() && !worktrees.is_empty(),
        "wire captures contain no implementation profiles"
    );
    let implementation_first = *executors.iter().chain(worktrees.iter()).min().unwrap();
    ensure!(
        explorer_wait < implementation_first,
        "implementation spawn occurred before the explorer wait/read"
    );
    let implementation_wait = calls
        .iter()
        .enumerate()
        .skip(implementation_first + 1)
        .find(|(_, call)| is_wait(call))
        .map(|(index, _)| index)
        .context("implementation spawns were not followed by a wait/read")?;
    ensure!(
        executors.iter().all(|index| *index < implementation_wait)
            && worktrees.iter().all(|index| *index < implementation_wait),
        "both implementation spawns must precede the implementation wait/read"
    );
    let cherry_pick = calls
        .iter()
        .position(|call| {
            call.name == "exec"
                && call
                    .arguments
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| command.contains("cherry-pick"))
        })
        .context("wire captures contain no explicit cherry-pick")?;
    ensure!(
        implementation_wait < cherry_pick,
        "cherry-pick occurred before implementation wait/read"
    );
    let cleanup = calls
        .iter()
        .position(|call| {
            call.name == "close_agent"
                && call
                    .arguments
                    .get("workspaceDisposition")
                    .and_then(Value::as_str)
                    == Some("cleanup")
        })
        .context("wire captures contain no explicit cleanup")?;
    let reviewer = spawn_indices("reviewer");
    ensure!(
        !reviewer.is_empty(),
        "wire captures contain no reviewer spawn"
    );
    ensure!(cleanup > cherry_pick, "cleanup occurred before cherry-pick");
    ensure!(
        reviewer[0] > cleanup,
        "reviewer was not spawned after integration and cleanup"
    );
    ensure!(
        calls[reviewer[0]].arguments.get("writablePaths").is_none(),
        "reviewer spawn unexpectedly requested writablePaths"
    );
    Ok(())
}

fn ensure_finding_re_review(calls: &[WireCall], outputs: &[WireOutput]) -> Result<()> {
    let finding = outputs
        .iter()
        .any(|output| output.content.contains("REVIEWER_FINDING"));
    if !finding {
        return Ok(());
    }
    let reviewers = calls
        .iter()
        .enumerate()
        .filter(|(_, call)| {
            call.name == "spawn_agent"
                && call.arguments.get("profileId").and_then(Value::as_str) == Some("reviewer")
        })
        .collect::<Vec<_>>();
    ensure!(
        reviewers.len() >= 2,
        "REVIEWER_FINDING requires a second reviewer spawn"
    );
    ensure!(
        reviewers[0].1.call_id.is_some()
            && reviewers[1].1.call_id.is_some()
            && reviewers[0].1.call_id != reviewers[1].1.call_id,
        "finding re-review requires two different reviewer callIds"
    );
    let second_reviewer = reviewers[1].0;
    let first_reviewer = reviewers[0].0;
    ensure!(
        calls.iter().enumerate().any(|(index, call)| {
            index > first_reviewer
                && index < second_reviewer
                && call.name == "spawn_agent"
                && matches!(
                    call.arguments.get("profileId").and_then(Value::as_str),
                    Some("executor") | Some("worktree_executor")
                )
        }),
        "REVIEWER_FINDING requires a new implementation spawn"
    );
    let implementation_spawn = calls
        .iter()
        .enumerate()
        .find(|(index, call)| {
            *index > first_reviewer
                && *index < second_reviewer
                && call.name == "spawn_agent"
                && matches!(
                    call.arguments.get("profileId").and_then(Value::as_str),
                    Some("executor") | Some("worktree_executor")
                )
        })
        .map(|(index, _)| index)
        .context("REVIEWER_FINDING requires a new implementation spawn")?;
    let integration = calls.iter().enumerate().any(|(index, call)| {
        index > implementation_spawn
            && index < second_reviewer
            && call.name == "exec"
            && call
                .arguments
                .get("command")
                .and_then(Value::as_str)
                .is_some_and(|command| {
                    command.contains("cherry-pick")
                        || command.contains("cargo test")
                        || command.contains("git diff --check")
                })
    });
    ensure!(
        integration,
        "REVIEWER_FINDING lacks second integration evidence"
    );
    ensure!(
        outputs
            .iter()
            .any(|output| output.content.contains("REVIEWER_READ_ONLY_APPROVED")),
        "REVIEWER_FINDING lacks final reviewer approval"
    );
    Ok(())
}

fn collect_calls(body: &Value, calls: &mut Vec<WireCall>) {
    for item in body
        .get("input")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if item.get("type").and_then(Value::as_str) == Some("function_call")
            && let Some(name) = item.get("name").and_then(Value::as_str)
            && let Some(arguments) = parse_arguments(item.get("arguments"))
        {
            calls.push(WireCall {
                call_id: item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                name: name.to_string(),
                arguments,
            });
        }
    }
    for message in body
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for call in message
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let function = call.get("function").unwrap_or(call);
            if let Some(name) = function.get("name").and_then(Value::as_str)
                && let Some(arguments) = parse_arguments(function.get("arguments"))
            {
                calls.push(WireCall {
                    call_id: call
                        .get("id")
                        .or_else(|| function.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    name: name.to_string(),
                    arguments,
                });
            }
        }
    }
}

fn parse_arguments(value: Option<&Value>) -> Option<Value> {
    match value? {
        Value::String(value) => serde_json::from_str(value).ok(),
        Value::Object(_) => value.cloned(),
        _ => None,
    }
}

fn collect_outputs(body: &Value, outputs: &mut Vec<WireOutput>) {
    for item in body
        .get("input")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if item.get("type").and_then(Value::as_str) == Some("function_call_output")
            && let Some(output) = item.get("output").and_then(Value::as_str)
        {
            outputs.push(WireOutput {
                call_id: item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                content: output.to_string(),
            });
        }
    }
    for message in body
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if message.get("role").and_then(Value::as_str) == Some("tool")
            && let Some(output) = message.get("content").and_then(Value::as_str)
        {
            outputs.push(WireOutput {
                call_id: message
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                content: output.to_string(),
            });
        }
    }
}

fn ensure_submissions(calls: &[WireCall], outputs: &[WireOutput]) -> Result<()> {
    let profiles = calls
        .iter()
        .position(|call| call.name == "list_agent_profiles")
        .context("no list_agent_profiles")?;
    let first_spawn = calls
        .iter()
        .position(|call| call.name == "spawn_agent")
        .context("no spawn")?;
    ensure!(
        profiles < first_spawn,
        "list_agent_profiles occurred after spawn"
    );
    let receipts = bound_receipts(calls, outputs)?;
    let mut required = Vec::new();
    for (spawn, receipt) in receipts {
        let profile = spawn
            .arguments
            .get("profileId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if matches!(
            profile.as_str(),
            "explorer" | "executor" | "worktree_executor"
        ) {
            let agent = receipt
                .get("agentId")
                .and_then(Value::as_str)
                .unwrap()
                .to_string();
            required.push((
                profile,
                agent,
                calls
                    .iter()
                    .position(|call| call.call_id == spawn.call_id)
                    .unwrap(),
            ));
        }
    }
    ensure!(
        required.iter().filter(|(p, _, _)| *p == "explorer").count() >= 2,
        "fewer than two explorer receipts"
    );
    let first_impl = required
        .iter()
        .filter(|(p, _, _)| *p != "explorer")
        .map(|(_, _, i)| *i)
        .min()
        .context("no implementation spawn")?;
    for (profile, agent, spawn_index) in required {
        let read = calls
            .iter()
            .enumerate()
            .find(|(i, call)| {
                *i > if profile == "explorer" {
                    first_spawn
                } else {
                    spawn_index
                } && call.name == "read_agent_submissions"
                    && call.arguments.get("target").and_then(Value::as_str) == Some(agent.as_str())
            })
            .map(|(_, c)| c)
            .with_context(|| format!("{profile} agent {agent} has no targeted submissions read"))?;
        let read_id = read
            .call_id
            .as_ref()
            .context("read_agent_submissions has no call_id")?;
        let output = outputs
            .iter()
            .find(|output| output.call_id.as_ref() == Some(read_id))
            .context("submissions read has no bound output")?;
        let result: Value =
            serde_json::from_str(&output.content).context("submissions output is not JSON")?;
        ensure!(
            result
                .get("total")
                .and_then(Value::as_u64)
                .is_some_and(|n| n >= 1),
            "submissions total is less than one"
        );
        ensure!(
            result
                .get("items")
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty()),
            "submissions items are empty"
        );
        if profile == "explorer" {
            ensure!(
                calls
                    .iter()
                    .position(|call| call.call_id == read.call_id)
                    .unwrap()
                    < first_impl,
                "explorer submissions read occurred after implementation spawn"
            );
        }
    }
    Ok(())
}

fn deduplicate_calls(calls: &mut Vec<WireCall>) {
    let mut seen = std::collections::HashSet::new();
    calls.retain(|call| {
        let key = call
            .call_id
            .clone()
            .unwrap_or_else(|| format!("{}:{}", call.name, call.arguments));
        seen.insert(key)
    });
}

fn deduplicate_outputs(outputs: &mut Vec<WireOutput>) {
    let mut seen = std::collections::HashSet::new();
    outputs.retain(|output| {
        output
            .call_id
            .as_ref()
            .map_or(true, |id| seen.insert(id.clone()))
    });
}

fn collect_json(directory: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_json(&path, output)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            output.push(path);
        }
    }
    output.sort();
    Ok(())
}

fn run_git(directory: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_is_a_git_repository_with_an_initial_head() {
        let root = tempfile::tempdir().unwrap();
        prepare_fixture(root.path()).unwrap();
        assert_eq!(
            run_git(root.path(), &["rev-parse", "--abbrev-ref", "HEAD"])
                .unwrap()
                .trim(),
            "main"
        );
        assert_eq!(
            fs::read_to_string(root.path().join("src/lib.rs")).unwrap(),
            "pub fn fixture_ready() -> bool { true }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn fixture_is_ready() { assert!(super::fixture_ready()); }\n}\n"
        );
    }

    #[test]
    fn spawn_receipt_requires_two_different_profiles_and_directory_paths() {
        let calls = vec![
            WireCall {
                call_id: Some("e".into()),
                name: "spawn_agent".to_string(),
                arguments: serde_json::json!({"profileId":"explorer"}),
            },
            WireCall {
                call_id: Some("x".into()),
                name: "spawn_agent".to_string(),
                arguments: serde_json::json!({
                    "profileId": "executor",
                    "writablePaths": ["allowed"]
                }),
            },
            WireCall {
                call_id: Some("w".into()),
                name: "spawn_agent".to_string(),
                arguments: serde_json::json!({"profileId": "worktree_executor"}),
            },
            WireCall {
                call_id: Some("r".into()),
                name: "spawn_agent".into(),
                arguments: serde_json::json!({"profileId":"reviewer"}),
            },
        ];
        ensure_spawn_calls(&calls).unwrap();
        let receipts = [
            ("e", "explorer", serde_json::json!({"mode":"unrestricted"})),
            ("x", "executor", serde_json::json!({"mode":"directory","writablePaths":["/tmp/fixture/allowed"]})),
            ("w", "worktree_executor", serde_json::json!({"mode":"worktree","worktree":{"branch":"pure-agent-child","baseCommit":"0123456789abcdef0123456789abcdef01234567"}})),
            ("r", "reviewer", serde_json::json!({"mode":"unrestricted"})),
        ].into_iter().map(|(id, profile, workspace)| WireOutput { call_id: Some(id.into()), content: serde_json::json!({"profileId":profile,"agentId":id,"workspace":workspace}).to_string() }).collect::<Vec<_>>();
        ensure_workspace_receipts(&calls, &receipts).unwrap();
    }

    #[test]
    fn isolated_live_config_disables_all_four_profiles() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config.toml");
        fs::write(&config, "disabled_system_agents = []\n").unwrap();
        disable_all_live_profiles(&config).unwrap();
        let table = fs::read_to_string(config)
            .unwrap()
            .parse::<toml::Table>()
            .unwrap();
        let disabled = table
            .get("disabled_system_agents")
            .and_then(toml::Value::as_array)
            .unwrap();
        for profile in ["explorer", "executor", "worktree_executor", "reviewer"] {
            assert!(disabled.iter().any(|value| value.as_str() == Some(profile)));
        }
    }

    #[test]
    fn live_schema_matches_runtime_schema() {
        assert_eq!(super::super::LIVE_CONFIG_SCHEMA_VERSION, 17);
    }

    #[test]
    fn capture_classification_accepts_responses_and_chat_and_rejects_ambiguity() {
        let responses = serde_json::json!({"input":[{"role":"system","content":[{"type":"input_text","text":PROFILE_FIRST_LINES[0].1}]}]});
        assert_eq!(classify_capture(&responses).unwrap(), "explorer");
        let chat =
            serde_json::json!({"messages":[{"role":"system","content":PROFILE_FIRST_LINES[3].1}]});
        assert_eq!(classify_capture(&chat).unwrap(), "reviewer");
        let ambiguous = serde_json::json!({"input":[{"role":"system","content":[{"text":PROFILE_FIRST_LINES[0].1},{"text":PROFILE_FIRST_LINES[3].1}]}]});
        assert!(classify_capture(&ambiguous).is_err());
        assert_eq!(
            classify_capture(&serde_json::json!({"input":[]})).unwrap(),
            "unknown"
        );
        assert_eq!(
            classify_capture(
                &serde_json::json!({"input":[{"role":"user","content":PROFILE_FIRST_LINES[0].1}]})
            )
            .unwrap(),
            "unknown"
        );
    }

    #[test]
    fn reviewer_mutation_is_rejected_but_read_is_allowed() {
        let read = CaptureReceipt {
            path: "responses.json".into(),
            actor: "reviewer".into(),
            calls: vec![WireCall {
                call_id: Some("r1".into()),
                name: "read_file".into(),
                arguments: serde_json::json!({"path":"Cargo.toml"}),
            }],
        };
        assert!(ensure_reviewer_history(&[read]).is_ok());
        let mutation = CaptureReceipt {
            path: "chat.json".into(),
            actor: "reviewer".into(),
            calls: vec![WireCall {
                call_id: Some("r2".into()),
                name: "exec".into(),
                arguments: serde_json::json!({"command":"git commit -am bad"}),
            }],
        };
        assert!(ensure_reviewer_history(&[mutation]).is_err());
    }

    #[test]
    fn reviewer_mutation_command_filter_covers_real_tools_and_shell_forms() {
        for name in [
            "write_file",
            "apply_patch",
            "delete_file",
            "copy_file",
            "move_file",
            "delete_path",
            "copy_path",
            "move_path",
            "write_session_note",
            "apply_session_note_patch",
        ] {
            let capture = CaptureReceipt {
                path: "x".into(),
                actor: "reviewer".into(),
                calls: vec![WireCall {
                    call_id: Some(name.into()),
                    name: name.into(),
                    arguments: serde_json::json!({}),
                }],
            };
            assert!(ensure_reviewer_history(&[capture]).is_err(), "{name}");
        }
        for command in [
            "sed -n '1,4p' file",
            "git diff --stat",
            "cargo test -p pl-xtask",
        ] {
            assert!(!is_mutating_shell_command(command), "{command}");
        }
        for command in [
            "printf x>file",
            "printf x >> file",
            "tee file",
            "sed -i s/a/b/ file",
            "git worktree remove /tmp/x",
            "git branch -D child",
        ] {
            assert!(is_mutating_shell_command(command), "{command}");
        }
    }

    #[test]
    fn duplicate_call_ids_are_removed_without_reordering_first_occurrence() {
        let mut calls = vec![
            WireCall {
                call_id: Some("a".into()),
                name: "spawn_agent".into(),
                arguments: serde_json::json!({"profileId":"explorer"}),
            },
            WireCall {
                call_id: Some("a".into()),
                name: "spawn_agent".into(),
                arguments: serde_json::json!({"profileId":"explorer"}),
            },
            WireCall {
                call_id: Some("b".into()),
                name: "wait_agents".into(),
                arguments: serde_json::json!({}),
            },
        ];
        deduplicate_calls(&mut calls);
        assert_eq!(
            calls
                .iter()
                .map(|call| call.call_id.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("a"), Some("b")]
        );
    }

    #[test]
    fn profile_message_missing_or_empty_contract_section_fails() {
        let mut canonical = profile_message_calls();
        canonical[2].arguments["message"] = Value::String(String::new());
        assert!(ensure_profile_messages(&canonical).is_err());
    }

    #[test]
    fn profile_message_contract_requires_all_profiles_and_sections() {
        ensure_profile_messages(&profile_message_calls()).unwrap();
    }

    #[test]
    fn every_reviewer_message_requires_the_complete_contract() {
        let mut calls = profile_message_calls();
        let mut second = calls
            .iter()
            .find(|call| {
                call.arguments.get("profileId").and_then(Value::as_str) == Some("reviewer")
            })
            .cloned()
            .unwrap();
        second.call_id = Some("reviewer-2".into());
        second.arguments["message"] = Value::String(
            second.arguments["message"]
                .as_str()
                .unwrap()
                .replace("[[CHILD_CONTRACT:evidence]]", ""),
        );
        calls.push(second);
        assert!(ensure_profile_messages(&calls).is_err());

        let mut complete = profile_message_calls();
        let mut second = complete
            .iter()
            .find(|call| {
                call.arguments.get("profileId").and_then(Value::as_str) == Some("reviewer")
            })
            .cloned()
            .unwrap();
        second.call_id = Some("reviewer-2".into());
        complete.push(second);
        ensure_profile_messages(&complete).unwrap();
    }

    fn profile_message_calls() -> Vec<WireCall> {
        let mut calls = Vec::new();
        for (profile, id) in [
            ("explorer", "explorer-1"),
            ("explorer", "explorer-2"),
            ("executor", "executor-1"),
            ("worktree_executor", "worktree-1"),
            ("reviewer", "reviewer-1"),
        ] {
            let message = [
                "purpose",
                "baseline",
                "ownership",
                "forbidden",
                "steps",
                "completion_failure",
                "evidence",
                "workspace_git_cleanup",
            ]
            .into_iter()
            .map(|section| format!("[[CHILD_CONTRACT:{section}]]\ncontent\n"))
            .collect::<String>();
            let message = if profile == "explorer" && id == "explorer-2" {
                message
                    .replace(
                        "[[CHILD_CONTRACT:purpose]]\ncontent",
                        "[[CHILD_CONTRACT:purpose]]\nsecond purpose",
                    )
                    .replace(
                        "[[CHILD_CONTRACT:ownership]]\ncontent",
                        "[[CHILD_CONTRACT:ownership]]\nsecond ownership",
                    )
            } else {
                message
            };
            calls.push(WireCall {
                call_id: Some(id.into()),
                name: "spawn_agent".into(),
                arguments: serde_json::json!({
                    "profileId": profile,
                    "forkTurns": "none",
                    "message": message
                }),
            });
        }
        calls
    }

    fn orchestration_call(id: &str, name: &str, arguments: Value) -> WireCall {
        WireCall {
            call_id: Some(id.into()),
            name: name.into(),
            arguments,
        }
    }

    #[test]
    fn orchestration_order_accepts_explorer_wait_then_both_implementations() {
        let calls = vec![
            orchestration_call(
                "e1",
                "spawn_agent",
                serde_json::json!({"profileId":"explorer"}),
            ),
            orchestration_call(
                "e2",
                "spawn_agent",
                serde_json::json!({"profileId":"explorer"}),
            ),
            orchestration_call("w1", "wait_agents", serde_json::json!({})),
            orchestration_call(
                "x1",
                "spawn_agent",
                serde_json::json!({"profileId":"executor"}),
            ),
            orchestration_call(
                "x2",
                "spawn_agent",
                serde_json::json!({"profileId":"worktree_executor"}),
            ),
            orchestration_call("w2", "read_agent_session", serde_json::json!({})),
            orchestration_call(
                "i1",
                "exec",
                serde_json::json!({"command":"git cherry-pick abc"}),
            ),
            orchestration_call(
                "c1",
                "close_agent",
                serde_json::json!({"workspaceDisposition":"cleanup"}),
            ),
            orchestration_call(
                "r1",
                "spawn_agent",
                serde_json::json!({"profileId":"reviewer"}),
            ),
        ];
        ensure_orchestration_order(&calls).unwrap();
    }

    #[test]
    fn orchestration_order_rejects_wait_after_only_one_implementation_spawn() {
        let calls = vec![
            orchestration_call(
                "e1",
                "spawn_agent",
                serde_json::json!({"profileId":"explorer"}),
            ),
            orchestration_call(
                "e2",
                "spawn_agent",
                serde_json::json!({"profileId":"explorer"}),
            ),
            orchestration_call("w1", "wait_agents", serde_json::json!({})),
            orchestration_call(
                "x1",
                "spawn_agent",
                serde_json::json!({"profileId":"executor"}),
            ),
            orchestration_call("w2", "read_agent_session", serde_json::json!({})),
            orchestration_call(
                "x2",
                "spawn_agent",
                serde_json::json!({"profileId":"worktree_executor"}),
            ),
            orchestration_call(
                "i1",
                "exec",
                serde_json::json!({"command":"git cherry-pick abc"}),
            ),
            orchestration_call(
                "c1",
                "close_agent",
                serde_json::json!({"workspaceDisposition":"cleanup"}),
            ),
            orchestration_call(
                "r1",
                "spawn_agent",
                serde_json::json!({"profileId":"reviewer"}),
            ),
        ];
        assert!(ensure_orchestration_order(&calls).is_err());
    }

    #[test]
    fn orchestration_order_rejects_cleanup_before_cherry_pick() {
        let calls = vec![
            orchestration_call(
                "e1",
                "spawn_agent",
                serde_json::json!({"profileId":"explorer"}),
            ),
            orchestration_call(
                "e2",
                "spawn_agent",
                serde_json::json!({"profileId":"explorer"}),
            ),
            orchestration_call("w1", "wait_agents", serde_json::json!({})),
            orchestration_call(
                "x1",
                "spawn_agent",
                serde_json::json!({"profileId":"executor"}),
            ),
            orchestration_call(
                "x2",
                "spawn_agent",
                serde_json::json!({"profileId":"worktree_executor"}),
            ),
            orchestration_call("w2", "read_agent_session", serde_json::json!({})),
            orchestration_call(
                "c1",
                "close_agent",
                serde_json::json!({"workspaceDisposition":"cleanup"}),
            ),
            orchestration_call(
                "i1",
                "exec",
                serde_json::json!({"command":"git cherry-pick abc"}),
            ),
            orchestration_call(
                "r1",
                "spawn_agent",
                serde_json::json!({"profileId":"reviewer"}),
            ),
        ];
        assert!(ensure_orchestration_order(&calls).is_err());
    }

    fn finding_calls() -> Vec<WireCall> {
        vec![
            orchestration_call(
                "r1",
                "spawn_agent",
                serde_json::json!({"profileId":"reviewer"}),
            ),
            orchestration_call(
                "x1",
                "spawn_agent",
                serde_json::json!({"profileId":"executor"}),
            ),
            orchestration_call("i1", "exec", serde_json::json!({"command":"cargo test"})),
            orchestration_call(
                "r2",
                "spawn_agent",
                serde_json::json!({"profileId":"reviewer"}),
            ),
        ]
    }

    #[test]
    fn finding_re_review_without_finding_allows_one_reviewer() {
        let calls = vec![orchestration_call(
            "r1",
            "spawn_agent",
            serde_json::json!({"profileId":"reviewer"}),
        )];
        ensure_finding_re_review(&calls, &[]).unwrap();
    }

    #[test]
    fn finding_re_review_requires_second_reviewer() {
        let calls = finding_calls();
        assert!(
            ensure_finding_re_review(
                &calls[..2],
                &[WireOutput {
                    call_id: None,
                    content: "REVIEWER_FINDING".into()
                }]
            )
            .is_err()
        );
    }

    #[test]
    fn finding_re_review_accepts_complete_rework_sequence() {
        let calls = finding_calls();
        ensure_finding_re_review(
            &calls,
            &[
                WireOutput {
                    call_id: None,
                    content: "REVIEWER_FINDING".into(),
                },
                WireOutput {
                    call_id: None,
                    content: "REVIEWER_READ_ONLY_APPROVED".into(),
                },
            ],
        )
        .unwrap();
    }
}
