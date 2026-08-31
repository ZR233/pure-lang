//! Real native-GUI acceptance for directory and worktree child Agents.

use super::{
    copy_directory, current_home, resident, unix_nanos, user_config_state,
    write_isolated_live_config,
};
use crate::cli::VerifySubagentsOptions;
use crate::{paths, process};
use anyhow::{Context, Result, bail, ensure};
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
const DIRECTORY_MARKER: &str = "DIRECTORY_MARKER";
const WORKTREE_RESULT_MARKER: &str = "WORKTREE_RESULT_MARKER";
const EXPLORER_STEPS_FIXTURE_SOURCE_V1: &str = "LIVE_EXPLORER_STEPS_V1: fixture-source\n1. 只读读取 Cargo.toml。\n2. 只读读取 src/lib.rs。\n3. 输出要求：总结这两个文件中与 Task workflow 和 live artifact 相关的有限源码事实，并与 root 注入的已编译阶段图及 Profile/spawn facts 对照；完成后直接 final reply。";
const EXPLORER_STEPS_WORKSPACE_GIT_V1: &str = "LIVE_EXPLORER_STEPS_V1: workspace-git\n1. 只读读取 .gitignore。\n2. 只调用 git_workspace_info。\n3. 只调用 git_status。\n4. 输出要求：总结 workspace/Git lifecycle 元数据，并在完成后直接 final reply。";

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
    ensure_marker_file(
        fixture,
        "allowed/directory.txt",
        DIRECTORY_MARKER,
        "directory child output is missing or incorrect",
    )?;
    ensure!(
        !fixture.join("forbidden/denied.txt").exists(),
        "directory child bypassed writablePaths"
    );
    ensure_marker_file(
        fixture,
        "worktree_result.txt",
        WORKTREE_RESULT_MARKER,
        "worktree child commit was not integrated",
    )?;
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
                file_receipt(fixture, "design/subagents-orchestration.md")?,
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

fn ensure_marker_file(fixture: &Path, relative: &str, marker: &str, error: &str) -> Result<()> {
    let content = fs::read_to_string(fixture.join(relative))?;
    ensure!(content.contains(marker), "{error}");
    Ok(())
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
    ensure_profile_messages(&calls, &outputs)?;
    ensure_reviewer_history(&capture_receipts)?;
    ensure_root_history(&capture_receipts, &calls, &outputs)?;
    ensure_submissions(&calls, &outputs)?;
    ensure_finding_re_review(&calls, &outputs)?;
    ensure_orchestration_order(&calls, &outputs)?;
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
                if matches!(role, Some("system" | "developer"))
                    && let Some(content) = object.get("content")
                {
                    collect_text(content, out);
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

fn ensure_profile_messages(calls: &[WireCall], outputs: &[WireOutput]) -> Result<()> {
    let required = ["explorer", "executor", "worktree_executor", "reviewer"];
    let receipts = bound_receipts(calls, outputs)?;
    let spawns = receipts.iter().map(|(spawn, _)| spawn).collect::<Vec<_>>();
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
        explorers.len() == 2,
        "wire captures must contain exactly two explorer spawn messages"
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
    let explorer_steps = explorers
        .iter()
        .map(|explorer| section(explorer, "steps"))
        .collect::<Result<Vec<_>>>()?;
    let canonical = [
        EXPLORER_STEPS_FIXTURE_SOURCE_V1,
        EXPLORER_STEPS_WORKSPACE_GIT_V1,
    ];
    for (index, (steps, expected)) in explorer_steps.iter().zip(canonical).enumerate() {
        let normalized = normalize_explorer_steps(steps);
        ensure!(
            normalized == expected,
            "explorer {} steps do not exactly match canonical block",
            index + 1
        );
    }
    Ok(())
}

fn normalize_explorer_steps(steps: &str) -> String {
    let normalized = steps.replace("\r\n", "\n").trim().to_owned();
    let opening = "```text\n";
    let closing = "\n```";
    if normalized.len() >= opening.len() + closing.len()
        && normalized.starts_with(opening)
        && normalized.ends_with(closing)
    {
        normalized[opening.len()..normalized.len() - closing.len()].to_owned()
    } else {
        normalized
    }
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

fn ensure_root_history(
    captures: &[CaptureReceipt],
    calls: &[WireCall],
    outputs: &[WireOutput],
) -> Result<()> {
    let root_calls = captures
        .iter()
        .filter(|capture| capture.actor == "root")
        .flat_map(|capture| capture.calls.iter())
        .collect::<Vec<_>>();
    ensure!(!root_calls.is_empty(), "no classified Root capture");
    let has = |name: &str, pred: fn(&WireCall) -> bool| {
        root_calls
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
    for capture in captures.iter().filter(|capture| capture.actor != "root") {
        ensure!(
            !capture.calls.iter().any(|call| {
                (call.name == "write_file"
                    && call.arguments.get("path").and_then(Value::as_str)
                        == Some("design/subagents-orchestration.md"))
                    || (call.name == "close_agent"
                        && call
                            .arguments
                            .get("workspaceDisposition")
                            .and_then(Value::as_str)
                            == Some("cleanup"))
                    || (call.name == "exec"
                        && call
                            .arguments
                            .get("command")
                            .and_then(Value::as_str)
                            .is_some_and(|command| command.contains("cherry-pick")))
            }),
            "root-only call observed in {} capture",
            capture.actor
        );
    }
    let approval = reviewer_submission_evidence(calls, outputs)?
        .last()
        .context("no reviewer approval evidence")?
        .approval_index
        .context("last reviewer lacks approval")?;
    let root_final_test_ids = captures
        .iter()
        .filter(|capture| capture.actor == "root")
        .flat_map(|capture| capture.calls.iter())
        .filter(|call| {
            call.name == "exec"
                && call
                    .arguments
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| command.contains("cargo test"))
        })
        .filter_map(|call| call.call_id.as_deref())
        .collect::<std::collections::HashSet<_>>();
    ensure!(
        calls.iter().enumerate().any(|(index, call)| {
            index > approval
                && call.name == "exec"
                && call
                    .call_id
                    .as_deref()
                    .is_some_and(|id| root_final_test_ids.contains(id))
                && call
                    .arguments
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| command.contains("cargo test"))
        }),
        "root capture lacks final cargo test after reviewer approval"
    );
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
    "report_progress",
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
    let mut receipts = Vec::new();
    let mut agent_ids = std::collections::HashSet::new();
    for spawn in calls.iter().filter(|call| call.name == "spawn_agent") {
        let id = spawn
            .call_id
            .as_ref()
            .context("spawn_agent has no call_id")?;
        let output = outputs
            .iter()
            .find(|output| output.call_id.as_ref() == Some(id))
            .with_context(|| format!("spawn {id} has no same-call-id output"))?;
        // A failed tool invocation is captured as plain text. Keep the call in the
        // audit trail, but only bind a successfully parsed JSON receipt below.
        let content = output.content.trim();
        let receipt: Value = match serde_json::from_str(content) {
            Ok(receipt) => receipt,
            Err(error) if looks_like_json_output(content) => {
                bail!(
                    "spawn {id} output looks like JSON but is malformed (bound_receipts): {error}"
                )
            }
            Err(_) if is_plain_text_failed_spawn(content) => continue,
            Err(error) => {
                bail!(
                    "spawn {id} output is neither a canonical JSON receipt nor an explicit plain-text tool failure (bound_receipts): {error}"
                )
            }
        };
        ensure!(
            receipt.is_object(),
            "spawn receipt is not a canonical JSON object"
        );
        ensure!(
            receipt
                .get("profileId")
                .and_then(Value::as_str)
                .is_some_and(|profile| !profile.is_empty()),
            "spawn receipt has no profileId"
        );
        ensure!(
            receipt.get("profileId") == spawn.arguments.get("profileId"),
            "spawn receipt profileId does not match call args"
        );
        let agent_id = receipt
            .get("agentId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .context("spawn receipt has no agentId")?;
        ensure!(
            agent_ids.insert(agent_id.clone()),
            "successful spawn receipts reuse agentId `{agent_id}`"
        );
        receipts.push((spawn.clone(), receipt));
    }
    Ok(receipts)
}

fn looks_like_json_output(content: &str) -> bool {
    matches!(content.chars().next(), Some('{' | '[' | '"'))
        || content.starts_with("null")
        || content.starts_with("true")
        || content.starts_with("false")
}

fn is_plain_text_failed_spawn(content: &str) -> bool {
    content.starts_with("Tool execution error:")
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

fn ensure_orchestration_order(calls: &[WireCall], outputs: &[WireOutput]) -> Result<()> {
    let receipts = bound_receipts(calls, outputs)?;
    let successful_spawns = |profile: &str| -> Result<Vec<(usize, String)>> {
        receipts
            .iter()
            .filter(|(spawn, receipt)| {
                spawn.arguments.get("profileId").and_then(Value::as_str) == Some(profile)
                    && receipt.get("profileId").and_then(Value::as_str) == Some(profile)
            })
            .map(|(spawn, receipt)| {
                let call_id = spawn
                    .call_id
                    .as_ref()
                    .context("successful spawn has no call_id")?;
                let index = calls
                    .iter()
                    .position(|call| call.call_id.as_ref() == Some(call_id))
                    .context("successful spawn is absent from flattened calls")?;
                let agent_id = receipt
                    .get("agentId")
                    .and_then(Value::as_str)
                    .context("successful spawn receipt has no agentId")?;
                Ok((index, agent_id.to_string()))
            })
            .collect()
    };
    let is_wait = |call: &WireCall| {
        matches!(
            call.name.as_str(),
            "wait_agents" | "read_agent_session" | "read_agent_submissions"
        )
    };
    let profiles = calls
        .iter()
        .position(|call| call.name == "list_agent_profiles")
        .context("wire captures contain no root Profile query")?;
    let confirmation = calls
        .iter()
        .position(|call| call.name == "request_user_input")
        .context("wire captures contain no plan confirmation")?;
    let design_write = calls
        .iter()
        .position(|call| {
            call.name == "write_file"
                && call.arguments.get("path").and_then(Value::as_str)
                    == Some("design/subagents-orchestration.md")
        })
        .context("wire captures contain no root design write")?;
    let explorer_wait = calls
        .iter()
        .position(is_wait)
        .context("wire captures contain no child wait/read operation")?;
    let explorers = successful_spawns("explorer")?;
    ensure!(
        explorers.len() == 2,
        "wire captures contain exactly two successful explorer spawns"
    );
    ensure!(
        profiles < explorers[0].0,
        "root Profile query must precede explorer spawns"
    );
    ensure!(
        explorers[1].0 == explorers[0].0 + 1,
        "two successful explorer spawns must be adjacent in flattened calls"
    );
    ensure!(
        explorers[1].0 < explorer_wait,
        "explorer spawns were not both issued before the first wait/read"
    );
    let explorer_agent_ids = explorers
        .iter()
        .take(2)
        .map(|(_, agent_id)| agent_id.clone())
        .collect::<std::collections::HashSet<_>>();
    let explorer_reads = calls
        .iter()
        .enumerate()
        .filter(|(index, call)| {
            *index > explorers[1].0
                && *index < confirmation
                && call.name == "read_agent_submissions"
        })
        .collect::<Vec<_>>();
    let explorer_targets = explorer_reads
        .iter()
        .filter_map(|(_, call)| {
            call.arguments
                .get("target")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<std::collections::HashSet<_>>();
    ensure!(
        explorer_agent_ids.is_subset(&explorer_targets),
        "explorer submission reads before confirmation must cover both receipt-bound agentIds"
    );
    let explorer_waits = calls
        .iter()
        .enumerate()
        .filter(|(index, call)| {
            *index > explorers[1].0 && *index < confirmation && call.name == "wait_agents"
        })
        .collect::<Vec<_>>();
    let waited_targets = explorer_waits
        .iter()
        .flat_map(|(_, call)| {
            call.arguments
                .get("targets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
        })
        .collect::<std::collections::HashSet<_>>();
    ensure!(
        explorer_agent_ids.is_subset(&waited_targets),
        "wait_agents targets before confirmation must cover both receipt-bound explorer agentIds"
    );
    let mut terminal_targets = std::collections::HashSet::new();
    for (_, wait) in explorer_waits {
        for agent_id in &explorer_agent_ids {
            if wait_has_terminal_evidence(wait, outputs, agent_id).is_ok() {
                terminal_targets.insert(agent_id.clone());
            }
        }
    }
    ensure!(
        explorer_agent_ids.is_subset(&terminal_targets),
        "same-call-id canonical wait outputs must provide terminal evidence for both explorer agentIds"
    );
    ensure!(
        confirmation < design_write,
        "root design write occurred before plan confirmation"
    );
    let executors = successful_spawns("executor")?;
    let worktrees = successful_spawns("worktree_executor")?;
    ensure!(
        !executors.is_empty() && !worktrees.is_empty(),
        "wire captures contain no implementation profiles"
    );
    let implementation_first = executors
        .iter()
        .chain(worktrees.iter())
        .map(|(index, _)| *index)
        .min()
        .unwrap();
    ensure!(
        design_write < implementation_first,
        "implementation spawn occurred before the confirmed design baseline"
    );
    let implementation_wait = calls
        .iter()
        .enumerate()
        .skip(implementation_first + 1)
        .find(|(_, call)| is_wait(call))
        .map(|(index, _)| index)
        .context("implementation spawns were not followed by a wait/read")?;
    ensure!(
        executors[0].0 + 1 == worktrees[0].0,
        "successful executor and worktree_executor spawns must be adjacent in flattened calls"
    );
    ensure!(
        executors
            .iter()
            .all(|(index, _)| *index < implementation_wait)
            && worktrees
                .iter()
                .all(|(index, _)| *index < implementation_wait),
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
    let reviewer = successful_spawns("reviewer")?;
    ensure!(
        !reviewer.is_empty(),
        "wire captures contain no reviewer spawn"
    );
    ensure!(cleanup > cherry_pick, "cleanup occurred before cherry-pick");
    ensure!(
        reviewer[0].0 > cleanup,
        "reviewer was not spawned after integration and cleanup"
    );
    ensure!(
        calls[reviewer[0].0]
            .arguments
            .get("writablePaths")
            .is_none(),
        "reviewer spawn unexpectedly requested writablePaths"
    );
    Ok(())
}

#[derive(Debug)]
struct ReviewerSubmissionEvidence {
    spawn_index: usize,
    agent_id: String,
    finding_index: Option<usize>,
    approval_index: Option<usize>,
}

fn reviewer_submission_evidence(
    calls: &[WireCall],
    outputs: &[WireOutput],
) -> Result<Vec<ReviewerSubmissionEvidence>> {
    let reviewers = calls
        .iter()
        .enumerate()
        .filter(|(_, call)| {
            call.name == "spawn_agent"
                && call.arguments.get("profileId").and_then(Value::as_str) == Some("reviewer")
        })
        .collect::<Vec<_>>();
    let receipts = bound_receipts(calls, outputs)?;
    reviewers
        .into_iter()
        .map(|(spawn_index, spawn)| {
            let receipt = receipts
                .iter()
                .find(|(candidate, _)| candidate.call_id == spawn.call_id)
                .map(|(_, receipt)| receipt)
                .context("reviewer spawn has no bound receipt")?;
            let agent_id = receipt
                .get("agentId")
                .and_then(Value::as_str)
                .context("reviewer receipt has no agentId")?
                .to_string();
            let targeted_reads = calls
                .iter()
                .enumerate()
                .filter(|(index, call)| {
                    *index > spawn_index
                        && call.name == "read_agent_submissions"
                        && call.arguments.get("target").and_then(Value::as_str)
                            == Some(agent_id.as_str())
                })
                .collect::<Vec<_>>();
            ensure!(
                !targeted_reads.is_empty(),
                "reviewer has no targeted submissions read"
            );
            let mut finding_index = None;
            let mut approval_index = None;
            for (read_index, read) in targeted_reads {
                let Some(read_id) = read.call_id.as_ref() else {
                    continue;
                };
                let Some(output) = outputs
                    .iter()
                    .find(|output| output.call_id.as_ref() == Some(read_id))
                else {
                    continue;
                };
                let page = serde_json::from_str::<Value>(&output.content)
                    .context("reviewer submissions output is not JSON")?;
                let items = page
                    .get("items")
                    .and_then(Value::as_array)
                    .context("reviewer submissions output has no items")?;
                ensure!(
                    page.get("offset").and_then(Value::as_u64).is_some()
                        && page.get("limit").and_then(Value::as_u64).is_some()
                        && page.get("total").and_then(Value::as_u64).is_some()
                        && page.get("hasMore").and_then(Value::as_bool).is_some(),
                    "reviewer submissions output is not a canonical page"
                );
                for item in items {
                    ensure!(
                        item.get("stage").and_then(Value::as_str).is_some()
                            && item.get("summary").and_then(Value::as_str).is_some()
                            && item.get("nextStep").and_then(Value::as_str).is_some()
                            && item.get("createdAt").and_then(Value::as_i64).is_some()
                            && item.get("detail").is_none_or(|detail| detail.is_string()),
                        "reviewer submission item is not canonical"
                    );
                }
                if items.is_empty() || page.get("total").and_then(Value::as_u64) == Some(0) {
                    continue;
                }
                let marker = |name: &str| {
                    items.iter().any(|item| {
                        ["summary", "nextStep", "detail"].iter().any(|field| {
                            item.get(*field)
                                .and_then(Value::as_str)
                                .is_some_and(|text| text.contains(name))
                        })
                    })
                };
                if marker("REVIEWER_FINDING") {
                    finding_index = Some(read_index);
                }
                if marker("REVIEWER_READ_ONLY_APPROVED") && approval_index.is_none() {
                    approval_index = Some(read_index);
                }
            }
            Ok(ReviewerSubmissionEvidence {
                spawn_index,
                agent_id,
                finding_index,
                approval_index,
            })
        })
        .collect()
}

fn ensure_finding_re_review(calls: &[WireCall], outputs: &[WireOutput]) -> Result<()> {
    let evidence = reviewer_submission_evidence(calls, outputs)?;
    let finding = evidence
        .iter()
        .any(|reviewer| reviewer.finding_index.is_some());
    let final_reviewer = evidence.last().context("no reviewer spawn")?;
    ensure!(
        final_reviewer.approval_index.is_some(),
        "last reviewer targeted submission lacks final approval"
    );
    if !finding {
        return Ok(());
    }
    ensure!(
        evidence.len() >= 2,
        "REVIEWER_FINDING requires a second reviewer spawn"
    );
    ensure!(
        evidence
            .windows(2)
            .all(|pair| pair[0].agent_id != pair[1].agent_id),
        "finding re-review requires different reviewer agentIds"
    );
    ensure!(
        evidence[..evidence.len() - 1]
            .iter()
            .any(|reviewer| reviewer.finding_index.is_some()),
        "REVIEWER_FINDING must come from a non-final reviewer submission"
    );
    let (finding_reviewer, finding_index) = evidence[..evidence.len() - 1]
        .iter()
        .filter_map(|reviewer| reviewer.finding_index.map(|index| (reviewer, index)))
        .max_by_key(|(_, index)| *index)
        .context("REVIEWER_FINDING must come from a non-final reviewer submission")?;
    ensure!(
        finding_reviewer.agent_id != final_reviewer.agent_id,
        "finding re-review requires a different final reviewer agentId"
    );
    let final_reviewer_spawn = final_reviewer.spawn_index;
    let implementation_spawn = calls
        .iter()
        .enumerate()
        .find(|(index, call)| {
            *index > finding_index
                && *index < final_reviewer_spawn
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
            && index < final_reviewer_spawn
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
        required.iter().filter(|(p, _, _)| *p == "explorer").count() == 2,
        "wire captures contain exactly two explorer receipts"
    );
    let first_impl = required
        .iter()
        .filter(|(p, _, _)| *p != "explorer")
        .map(|(_, _, i)| *i)
        .min()
        .context("no implementation spawn")?;
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
        .context("no explicit cherry-pick")?;
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
        let result = parse_submission_page(&output.content)?;
        if result.total == 0 {
            let read_index = calls
                .iter()
                .position(|call| call.call_id == read.call_id)
                .context("submissions read is absent from flattened calls")?;
            let boundary = if profile == "explorer" {
                first_impl
            } else {
                cherry_pick
            };
            let terminal_wait = calls.iter().enumerate().find(|(index, call)| {
                *index > spawn_index
                    && *index < read_index
                    && call.name == "wait_agents"
                    && wait_has_terminal_evidence(call, outputs, &agent).is_ok()
            });
            terminal_wait.map(|(index, _)| index).with_context(|| {
                format!("{profile} agent {agent} has no strictly bound terminal wait")
            })?;
            ensure!(
                calls.iter().enumerate().any(|(index, call)| {
                    index > read_index
                        && index < boundary
                        && call.name == "read_agent_session"
                        && call.arguments.get("target").and_then(Value::as_str)
                            == Some(agent.as_str())
                        && session_has_terminal_evidence(call, outputs, &agent).is_ok()
                }),
                "{profile} agent {agent} has no strictly bound terminal session fallback"
            );
        }
        if profile == "explorer" {
            ensure!(
                calls
                    .iter()
                    .position(|call| call.call_id == read.call_id)
                    .unwrap()
                    < first_impl,
                "explorer submissions read occurred after implementation spawn"
            );
        } else {
            ensure!(
                calls
                    .iter()
                    .position(|call| call.call_id == read.call_id)
                    .unwrap()
                    < cherry_pick,
                "implementation submissions read occurred after cherry-pick"
            );
        }
    }
    Ok(())
}

fn wait_has_terminal_evidence(
    wait_call: &WireCall,
    outputs: &[WireOutput],
    agent: &str,
) -> Result<()> {
    ensure!(
        wait_call.name == "wait_agents",
        "terminal evidence is not wait_agents"
    );
    let targets = wait_call
        .arguments
        .get("targets")
        .and_then(Value::as_array)
        .context("wait_agents has no targets")?;
    ensure!(
        targets.iter().any(|target| target.as_str() == Some(agent)),
        "wait_agents targets do not contain {agent}"
    );
    let call_id = wait_call
        .call_id
        .as_ref()
        .context("wait_agents has no call_id")?;
    let output = outputs
        .iter()
        .find(|output| output.call_id.as_ref() == Some(call_id))
        .with_context(|| format!("wait_agents {call_id} has no same-call-id output"))?;
    let result: Value = serde_json::from_str(&output.content)
        .with_context(|| format!("wait_agents {call_id} output is not canonical JSON"))?;
    ensure!(
        result.is_object(),
        "wait_agents output is not a canonical object"
    );
    ensure!(
        result.get("reason").and_then(Value::as_str) == Some("terminal"),
        "wait_agents output is not terminal"
    );
    let messages = result
        .get("messages")
        .and_then(Value::as_array)
        .context("terminal wait output has no messages")?;
    let message = messages
        .iter()
        .find(|message| message.get("agentId").and_then(Value::as_str) == Some(agent))
        .context("terminal wait output has no message bound to agent")?;
    if let Some(state) = message.get("state") {
        ensure!(
            state
                .get("agent")
                .and_then(|agent| agent.get("kind"))
                .and_then(Value::as_str)
                == Some("idle"),
            "terminal wait message agent state is not idle"
        );
        ensure!(
            state
                .get("lastTurnOutcome")
                .and_then(|outcome| outcome.get("outcome"))
                .and_then(|outcome| outcome.get("kind"))
                .and_then(Value::as_str)
                == Some("completed"),
            "terminal wait message has no completed lastTurnOutcome"
        );
    }
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmissionPage {
    items: Vec<Value>,
    offset: u64,
    limit: u64,
    total: u64,
    has_more: bool,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionSnapshot {
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    thread_id: Option<String>,
    messages: Vec<SessionMessage>,
}

#[derive(Debug, serde::Deserialize)]
struct SessionMessage {
    role: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    content: Option<String>,
}

fn parse_submission_page(content: &str) -> Result<SubmissionPage> {
    let page: SubmissionPage =
        serde_json::from_str(content).context("submissions output is not canonical JSON")?;
    ensure!(
        page.total == 0 || !page.items.is_empty(),
        "submissions page total/items are inconsistent"
    );
    ensure!(
        page.total != 0 || page.items.is_empty(),
        "submissions page total/items are inconsistent"
    );
    let _ = (page.offset, page.limit, page.has_more);
    Ok(page)
}

fn session_has_terminal_evidence(
    session_call: &WireCall,
    outputs: &[WireOutput],
    agent: &str,
) -> Result<()> {
    ensure!(
        session_call.name == "read_agent_session"
            && session_call.arguments.get("target").and_then(Value::as_str) == Some(agent),
        "session fallback target is not bound to agent"
    );
    let call_id = session_call
        .call_id
        .as_ref()
        .context("session fallback has no call_id")?;
    let output = outputs
        .iter()
        .find(|output| output.call_id.as_ref() == Some(call_id))
        .context("session fallback has no same-call-id output")?;
    let session: SessionSnapshot =
        serde_json::from_str(&output.content).context("session fallback output is not JSON")?;
    if let Some(session_agent) = session.agent_id.as_deref() {
        ensure!(
            session_agent == agent,
            "session output agentId does not match target"
        );
    }
    if let Some(thread_id) = session.thread_id.as_deref() {
        ensure!(
            thread_id == agent,
            "session output threadId does not match target"
        );
    }
    let final_message = session
        .messages
        .last()
        .filter(|message| message.role == "assistant")
        .context("session fallback has no assistant final message")?;
    let text = final_message
        .text
        .as_deref()
        .or(final_message.content.as_deref())
        .context("session final assistant message is not text")?;
    ensure!(
        !text.trim().is_empty(),
        "session final assistant message is empty"
    );
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
            .is_none_or(|id| seen.insert(id.clone()))
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
    fn live_prompt_contains_deterministic_implementation_contract() {
        let prompt = include_str!("../../../test-fixtures/subagents-live/prompt.md");
        assert!(
            prompt.contains("spawn 调用参数硬门禁")
                && prompt.contains("每一次 `spawn_agent` 都必须在顶层参数中显式传入")
                && prompt.contains("不接受省略后依赖默认值")
                && prompt.contains("不接受只把 `forkTurns:none` 写进 child message")
                && prompt.contains("这两次 explorer `spawn_agent` 的顶层参数都必须显式包含")
                && prompt.contains("只有 executor 额外传 `writablePaths:[\"allowed\"]`")
                && prompt.contains(
                    "reviewer 的 `spawn_agent` 也必须显式传顶层 `\"forkTurns\":\"none\"`"
                ),
            "live prompt must require explicit top-level forkTurns:none for every spawn stage"
        );
        assert!(
            prompt.contains("DIRECTORY_MARKER"),
            "live prompt must require DIRECTORY_MARKER in directory.txt"
        );
        assert!(
            prompt.contains("WORKTREE_RESULT_MARKER"),
            "live prompt must require WORKTREE_RESULT_MARKER in worktree_result.txt"
        );
        assert!(
            prompt.contains("两个 explorer submissions 都读取后")
                && prompt.contains("executor 与 worktree_executor 两个 spawn_agent")
                && prompt
                    .contains("两次 implementation spawn 均完成后才能对任一实现调用 wait/read"),
            "live prompt must require both implementation spawns before wait/read"
        );
        assert!(
            prompt.contains("root 不得为 directory child 产物额外提交后再 spawn worktree")
                && prompt.contains("不得先额外提交 directory child 产物再 spawn worktree"),
            "live prompt must forbid serializing worktree spawn behind a directory commit"
        );
    }

    #[test]
    fn live_prompt_plans_with_bounded_explorers_before_confirmation() {
        let prompt = include_str!("../../../test-fixtures/subagents-live/prompt.md");
        assert!(
            prompt.contains(
                "planning 阶段由 root 调用 list_agent_profiles，随后并行 spawn 两个 explorer"
            ) && prompt.contains("两个 explorer 都进入 terminal")
                && prompt.contains("再调用 request_user_input 请求确认")
                && prompt.contains("确认后 root 才能写入 design/subagents-orchestration.md"),
            "live prompt must explore and synthesize the plan before confirmation and design edits"
        );
        assert!(
            prompt.contains("explorer child 不得调用 list_agent_profiles")
                && prompt.contains("不得调用 skill_view")
                && prompt.contains("不得读取 Studio home 或配置")
                && prompt.contains("不得全仓 rg")
                && prompt.contains("不得扫描 target/ 或 .git/ 内部")
                && prompt.contains("不得运行 cargo test"),
            "live prompt must keep explorer work finite and independent of Studio configuration"
        );
        assert!(
            prompt.contains("探索者一只读核对 Task workflow 与 live artifact")
                && prompt.contains("root 注入的已编译阶段图")
                && prompt.contains("探索者二只读核对 workspace 与 Git lifecycle"),
            "live prompt must keep the explorers semantically distinct while bounding their tools"
        );
        assert!(
            prompt.contains(EXPLORER_STEPS_FIXTURE_SOURCE_V1)
                && prompt.contains(EXPLORER_STEPS_WORKSPACE_GIT_V1)
                && prompt.contains("原样复制以下版本化 canonical block")
                && prompt.contains("不能增删、改写或追加动作")
                && prompt
                    .contains("只允许在完整 canonical block 外包一层 Markdown `text` 展示围栏")
                && prompt.contains("不得使用其它围栏语言、嵌套围栏"),
            "live prompt must require exact versioned explorer steps"
        );
    }

    #[test]
    fn durable_reviewer_verdict_is_prompted_end_to_end() {
        let reviewer = include_str!("../../../code/pl-studio-runtime/src/prompts/reviewer.md");
        let workflow = include_str!(
            "../../../code/pl-studio-runtime/assets/skills/subagent-workflow/SKILL.md"
        );
        let task =
            include_str!("../../../code/pl-studio-runtime/assets/skills/modes/mode.task/SKILL.md");
        let live = include_str!("../../../test-fixtures/subagents-live/prompt.md");

        for (name, prompt) in [
            ("reviewer", reviewer),
            ("subagent-workflow", workflow),
            ("mode.task", task),
            ("live acceptance", live),
        ] {
            assert!(
                prompt.contains("report_progress")
                    && prompt.contains("REVIEWER_FINDING")
                    && prompt.contains("REVIEWER_READ_ONLY_APPROVED"),
                "{name} must require a durable reviewer verdict with fixed markers"
            );
        }
        assert!(
            reviewer.contains("final reply 前必须调用 `report_progress` 提交最终 durable verdict")
                && reviewer.contains("中间 submission 不能替代最终")
                && reviewer.contains("不得修改 workspace、Git")
                && reviewer.contains("不得使用 `exec`"),
            "fixed reviewer prompt must submit a final durable verdict without weakening read-only scope"
        );
        for prompt in [reviewer, workflow, task, live] {
            assert!(
                !prompt.contains("report_progress` exactly once")
                    && !prompt.contains("仅调用一次 `report_progress`")
                    && !prompt.contains("必须调用且仅调用一次 report_progress")
                    && !prompt.contains("并且只能调用一次"),
                "reviewer contracts must allow intermediate progress before the final durable verdict"
            );
        }
        assert!(
            workflow.contains("call `read_agent_submissions` with the reviewer agentId")
                && workflow.contains("A root retelling or `read_agent_session` does not count"),
            "subagent-workflow must require the root to consume canonical durable reviewer evidence"
        );
        assert!(
            task.contains("按 reviewer agentId 调用 `read_agent_submissions`")
                && task.contains("root 转述或 `read_agent_session` 不算"),
            "mode.task must require the root to consume canonical durable reviewer evidence"
        );
        let reviewer_read = live
            .find("按 reviewer agentId 调用 read_agent_submissions")
            .expect("live prompt must require a targeted reviewer submissions read");
        let final_test = live
            .find("targeted read 到该最终 durable verdict 后才执行最终 cargo test")
            .expect("live prompt must gate the final test on the durable verdict");
        assert!(
            reviewer_read < final_test,
            "live prompt must read the reviewer durable verdict before the final test"
        );
    }

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
    fn validate_fixture_accepts_live_marker_outputs_and_writes_receipts() {
        let root = tempfile::tempdir().unwrap();
        let fixture = root.path().join("fixture");
        let artifacts = root.path().join("artifacts");
        prepare_fixture(&fixture).unwrap();
        fs::create_dir_all(&artifacts).unwrap();
        fs::write(
            fixture.join("design/subagents-orchestration.md"),
            "ROOT_DESIGN_MARKER: orchestration contract\n",
        )
        .unwrap();
        fs::write(
            fixture.join("allowed/directory.txt"),
            "DIRECTORY_MARKER: this file was created by the directory executor inside the allowed boundary.\n",
        )
        .unwrap();
        fs::write(
            fixture.join("worktree_result.txt"),
            "WORKTREE_RESULT_MARKER: worktree child committed\n",
        )
        .unwrap();
        run_git(&fixture, &["add", "worktree_result.txt"]).unwrap();
        run_git(
            &fixture,
            &[
                "-c",
                "user.name=Pure Acceptance",
                "-c",
                "user.email=pure-acceptance@example.invalid",
                "commit",
                "-m",
                "feat: worktree executor marker",
            ],
        )
        .unwrap();

        validate_fixture(&fixture, &artifacts).unwrap();

        for name in [
            "final-git-status.txt",
            "final-git-log.txt",
            "final-worktree-list.txt",
            "final-file-diff.json",
        ] {
            assert!(artifacts.join(name).is_file(), "missing receipt {name}");
        }
        let diff: Value =
            serde_json::from_slice(&fs::read(artifacts.join("final-file-diff.json")).unwrap())
                .unwrap();
        let files = diff.get("files").and_then(Value::as_array).unwrap();
        assert_eq!(files.len(), 3);
        let rendered = serde_json::to_string(files).unwrap();
        for marker in [
            "ROOT_DESIGN_MARKER",
            DIRECTORY_MARKER,
            WORKTREE_RESULT_MARKER,
        ] {
            assert!(rendered.contains(marker), "receipt lacks marker {marker}");
        }
    }

    #[test]
    fn validate_fixture_rejects_missing_output_markers() {
        for (relative, content, expected) in [
            (
                "allowed/directory.txt",
                "directory child accepted\n",
                "directory child output is missing or incorrect",
            ),
            (
                "worktree_result.txt",
                "worktree child committed\n",
                "worktree child commit was not integrated",
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            let fixture = root.path().join("fixture");
            let artifacts = root.path().join("artifacts");
            prepare_fixture(&fixture).unwrap();
            fs::create_dir_all(&artifacts).unwrap();
            fs::write(
                fixture.join("design/subagents-orchestration.md"),
                "ROOT_DESIGN_MARKER\n",
            )
            .unwrap();
            fs::write(fixture.join("allowed/directory.txt"), "DIRECTORY_MARKER\n").unwrap();
            fs::write(
                fixture.join("worktree_result.txt"),
                "WORKTREE_RESULT_MARKER\n",
            )
            .unwrap();
            fs::write(fixture.join(relative), content).unwrap();
            run_git(&fixture, &["add", "worktree_result.txt"]).unwrap();
            run_git(
                &fixture,
                &[
                    "-c",
                    "user.name=Pure Acceptance",
                    "-c",
                    "user.email=pure-acceptance@example.invalid",
                    "commit",
                    "-m",
                    "feat: worktree executor marker",
                ],
            )
            .unwrap();

            let error = validate_fixture(&fixture, &artifacts).unwrap_err();
            assert!(error.to_string().contains(expected));
        }
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
    fn bound_receipts_ignores_failed_spawn_output_and_keeps_successful_receipt() {
        let calls = vec![
            WireCall {
                call_id: Some("failed".into()),
                name: "spawn_agent".into(),
                arguments: serde_json::json!({"profileId": "explorer"}),
            },
            WireCall {
                call_id: Some("retry".into()),
                name: "spawn_agent".into(),
                arguments: serde_json::json!({"profileId": "explorer"}),
            },
        ];
        let outputs = vec![
            WireOutput {
                call_id: Some("failed".into()),
                content: "Tool execution error: invalid writablePaths".into(),
            },
            WireOutput {
                call_id: Some("retry".into()),
                content: serde_json::json!({
                    "profileId": "explorer",
                    "agentId": "explorer-retry"
                })
                .to_string(),
            },
        ];

        let receipts = bound_receipts(&calls, &outputs).unwrap();
        assert_eq!(
            calls.len(),
            2,
            "failed call remains in the audit call history"
        );
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].0.call_id.as_deref(), Some("retry"));
        assert_eq!(receipts[0].1["agentId"], "explorer-retry");
    }

    #[test]
    fn bound_receipts_rejects_malformed_json_like_failed_output() {
        let calls = vec![WireCall {
            call_id: Some("malformed".into()),
            name: "spawn_agent".into(),
            arguments: serde_json::json!({"profileId": "explorer"}),
        }];
        for content in [" {\"profileId\":", "[", "\"unterminated", "false trailing"] {
            let outputs = vec![WireOutput {
                call_id: Some("malformed".into()),
                content: content.into(),
            }];
            let error = bound_receipts(&calls, &outputs).unwrap_err();
            assert!(error.to_string().contains("spawn malformed"));
            assert!(error.to_string().contains("looks like JSON"));
        }
    }

    #[test]
    fn bound_receipts_rejects_valid_non_object_and_invalid_canonical_receipts() {
        let calls = vec![WireCall {
            call_id: Some("invalid".into()),
            name: "spawn_agent".into(),
            arguments: serde_json::json!({"profileId": "explorer"}),
        }];
        for content in [
            "null",
            "[]",
            "{\"agentId\":\"explorer-a\"}",
            "{\"profileId\":\"executor\",\"agentId\":\"explorer-a\"}",
            "unrecognized tool output",
        ] {
            let outputs = vec![WireOutput {
                call_id: Some("invalid".into()),
                content: content.into(),
            }];
            assert!(
                bound_receipts(&calls, &outputs).is_err(),
                "accepted {content}"
            );
        }
    }

    #[test]
    fn bound_receipts_rejects_reused_successful_agent_ids_across_profiles() {
        let (calls, mut outputs) = valid_orchestration_calls();
        outputs
            .iter_mut()
            .filter(|output| output.call_id.as_deref() == Some("e2"))
            .for_each(|output| {
                output.content = serde_json::json!({
                    "profileId": "explorer",
                    "agentId": "explorer-a"
                })
                .to_string();
            });
        assert!(bound_receipts(&calls, &outputs).is_err());

        let (calls, mut outputs) = valid_orchestration_calls();
        outputs
            .iter_mut()
            .filter(|output| output.call_id.as_deref() == Some("r1"))
            .for_each(|output| {
                output.content = serde_json::json!({
                    "profileId": "reviewer",
                    "agentId": "executor-a"
                })
                .to_string();
            });
        assert!(bound_receipts(&calls, &outputs).is_err());
    }

    #[test]
    fn failed_spawn_output_alone_cannot_satisfy_required_receipts() {
        let calls = vec![WireCall {
            call_id: Some("failed".into()),
            name: "spawn_agent".into(),
            arguments: serde_json::json!({"profileId": "executor"}),
        }];
        let outputs = vec![WireOutput {
            call_id: Some("failed".into()),
            content: "Tool execution error: invalid writablePaths".into(),
        }];

        let error = ensure_workspace_receipts(&calls, &outputs).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("wire tool results contain no executor workspace receipt")
        );
    }

    #[test]
    fn failed_attempts_are_ignored_but_two_explorers_are_required_consistently() {
        let (mut calls, mut outputs) = valid_orchestration_calls();
        let failures = ["failed-empty", "failed-malicious"];
        for (offset, id) in failures.into_iter().enumerate() {
            calls.insert(
                1 + offset,
                orchestration_call(
                    id,
                    "spawn_agent",
                    serde_json::json!({"profileId":"explorer"}),
                ),
            );
            outputs.push(WireOutput {
                call_id: Some(id.into()),
                content: if offset == 0 {
                    "Tool execution error: capacity unavailable".into()
                } else {
                    "  Tool execution error: invalid writablePaths  ".into()
                },
            });
        }
        let cherry_pick = calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("i1"))
            .unwrap();
        for (id, target) in [("xr1", "executor-a"), ("xr2", "worktree-a")] {
            calls.insert(
                cherry_pick,
                orchestration_call(
                    id,
                    "read_agent_submissions",
                    serde_json::json!({"target":target}),
                ),
            );
            outputs.push(WireOutput {
                call_id: Some(id.into()),
                content: submission_page(vec![serde_json::json!({"id": "submission"})], 1),
            });
        }
        for id in ["er1", "er2"] {
            outputs.push(WireOutput {
                call_id: Some(id.into()),
                content: submission_page(vec![serde_json::json!({"id": "submission"})], 1),
            });
        }
        ensure_orchestration_order(&calls, &outputs).unwrap();
        ensure_submissions(&calls, &outputs).unwrap();

        let mut profile_calls = profile_message_calls();
        let mut profile_outputs = profile_message_outputs(&profile_calls);
        for id in failures {
            let failed = WireCall {
                call_id: Some(id.into()),
                name: "spawn_agent".into(),
                arguments: serde_json::json!({
                    "profileId": "explorer",
                    "forkTurns": "none",
                    "message": "failed attempt"
                }),
            };
            profile_calls.insert(0, failed);
            profile_outputs.push(WireOutput {
                call_id: Some(id.into()),
                content: "Tool execution error: capacity unavailable".into(),
            });
        }
        ensure_profile_messages(&profile_calls, &profile_outputs).unwrap();

        let third = calls[3].clone();
        calls.push(WireCall {
            call_id: Some("explorer-success-3".into()),
            ..third
        });
        outputs.push(spawn_output(
            "explorer-success-3",
            "explorer",
            "explorer-success-3",
        ));
        assert!(ensure_orchestration_order(&calls, &outputs).is_err());
        assert!(ensure_submissions(&calls, &outputs).is_err());

        let mut third_profile = profile_calls[2].clone();
        third_profile.call_id = Some("explorer-success-3".into());
        profile_calls.push(third_profile);
        profile_outputs.push(spawn_output(
            "explorer-success-3",
            "explorer",
            "explorer-success-3",
        ));
        assert!(ensure_profile_messages(&profile_calls, &profile_outputs).is_err());
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
        let durable_verdict = CaptureReceipt {
            path: "reviewer-progress.json".into(),
            actor: "reviewer".into(),
            calls: vec![WireCall {
                call_id: Some("reviewer-verdict".into()),
                name: "report_progress".into(),
                arguments: serde_json::json!({
                    "stage": "verifying",
                    "summary": "REVIEWER_READ_ONLY_APPROVED",
                    "nextStep": "root integrates the durable verdict",
                }),
            }],
        };
        assert!(
            ensure_reviewer_history(&[durable_verdict]).is_ok(),
            "report_progress is the reviewer's only allowed durable collaboration write"
        );
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

    fn root_call(name: &str, arguments: Value) -> WireCall {
        orchestration_call(name, name, arguments)
    }

    fn submission_item(fields: &[(&str, &str)]) -> Value {
        let mut item = serde_json::json!({
            "stage": "readyForReview",
            "summary": "review completed",
            "nextStep": "continue orchestration",
            "createdAt": 1,
        });
        for (field, value) in fields {
            item[*field] = Value::String((*value).to_string());
        }
        item
    }

    fn submission_page(items: Vec<Value>, total: usize) -> String {
        serde_json::json!({
            "items": items,
            "offset": 0,
            "limit": 50,
            "total": total,
            "hasMore": false,
        })
        .to_string()
    }

    fn reviewer_receipt(call_id: &str, agent_id: &str) -> WireOutput {
        WireOutput {
            call_id: Some(call_id.into()),
            content: serde_json::json!({
                "profileId": "reviewer",
                "agentId": agent_id,
            })
            .to_string(),
        }
    }

    fn reviewer_read_output(call_id: Option<&str>, items: Vec<Value>, total: usize) -> WireOutput {
        WireOutput {
            call_id: call_id.map(str::to_string),
            content: submission_page(items, total),
        }
    }

    fn valid_root_history() -> (Vec<CaptureReceipt>, Vec<WireCall>, Vec<WireOutput>) {
        let root_calls = vec![
            orchestration_call(
                "design",
                "write_file",
                serde_json::json!({"path":"design/subagents-orchestration.md"}),
            ),
            orchestration_call(
                "pick",
                "exec",
                serde_json::json!({"command":"git cherry-pick abc"}),
            ),
            orchestration_call(
                "cleanup",
                "close_agent",
                serde_json::json!({"workspaceDisposition":"cleanup"}),
            ),
            orchestration_call(
                "r1",
                "spawn_agent",
                serde_json::json!({"profileId":"reviewer"}),
            ),
            orchestration_call(
                "rr1",
                "read_agent_submissions",
                serde_json::json!({"target":"reviewer-agent"}),
            ),
            orchestration_call(
                "final-test",
                "exec",
                serde_json::json!({"command":"cargo test --workspace"}),
            ),
        ];
        let captures = vec![CaptureReceipt {
            path: "root.json".into(),
            actor: "root".into(),
            calls: root_calls.clone(),
        }];
        let outputs = vec![
            reviewer_receipt("r1", "reviewer-agent"),
            reviewer_read_output(
                Some("rr1"),
                vec![submission_item(&[(
                    "detail",
                    "REVIEWER_READ_ONLY_APPROVED",
                )])],
                1,
            ),
        ];
        (captures, root_calls, outputs)
    }

    #[test]
    fn root_history_aggregates_all_root_captures() {
        let (mut captures, calls, outputs) = valid_root_history();
        let root_calls = captures.pop().unwrap().calls;
        captures = root_calls
            .chunks(2)
            .enumerate()
            .map(|(index, calls)| CaptureReceipt {
                path: format!("root-{index}.json"),
                actor: "root".into(),
                calls: calls.to_vec(),
            })
            .collect();
        ensure_root_history(&captures, &calls, &outputs).unwrap();
    }

    #[test]
    fn root_history_rejects_each_root_only_action_for_every_child_actor() {
        let root_only_calls = [
            root_call(
                "write_file",
                serde_json::json!({"path":"design/subagents-orchestration.md"}),
            ),
            root_call("exec", serde_json::json!({"command":"git cherry-pick abc"})),
            root_call(
                "close_agent",
                serde_json::json!({"workspaceDisposition":"cleanup"}),
            ),
        ];
        for actor in ["explorer", "executor", "worktree_executor", "reviewer"] {
            for forbidden in &root_only_calls {
                let (mut captures, calls, outputs) = valid_root_history();
                captures.push(CaptureReceipt {
                    path: format!("{actor}.json"),
                    actor: actor.into(),
                    calls: vec![forbidden.clone()],
                });
                assert!(
                    ensure_root_history(&captures, &calls, &outputs).is_err(),
                    "{actor} unexpectedly performed {}",
                    forbidden.name
                );
            }
        }
    }

    #[test]
    fn root_history_requires_all_root_owned_actions() {
        for missing_call_id in ["design", "pick", "cleanup"] {
            let (mut captures, calls, outputs) = valid_root_history();
            captures[0]
                .calls
                .retain(|call| call.call_id.as_deref() != Some(missing_call_id));
            assert!(
                ensure_root_history(&captures, &calls, &outputs).is_err(),
                "missing root action {missing_call_id}"
            );
        }
    }

    #[test]
    fn root_history_allows_child_implementation_write_and_pre_review_cargo_test() {
        let (mut captures, mut calls, outputs) = valid_root_history();
        let executor_write = orchestration_call(
            "executor-write",
            "write_file",
            serde_json::json!({"path":"allowed/directory.txt"}),
        );
        let worktree_test = orchestration_call(
            "worktree-test",
            "exec",
            serde_json::json!({"command":"cargo test"}),
        );
        captures.push(CaptureReceipt {
            path: "executor.json".into(),
            actor: "executor".into(),
            calls: vec![executor_write.clone()],
        });
        captures.push(CaptureReceipt {
            path: "worktree.json".into(),
            actor: "worktree_executor".into(),
            calls: vec![worktree_test.clone()],
        });
        let reviewer_spawn = calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("r1"))
            .unwrap();
        calls.splice(
            reviewer_spawn..reviewer_spawn,
            [executor_write, worktree_test],
        );
        ensure_root_history(&captures, &calls, &outputs).unwrap();
    }

    #[test]
    fn root_history_rejects_final_cargo_test_before_reviewer_approval() {
        let (mut captures, mut calls, outputs) = valid_root_history();
        calls.swap(4, 5);
        captures[0].calls.swap(4, 5);
        assert!(ensure_root_history(&captures, &calls, &outputs).is_err());
    }

    #[test]
    fn root_history_rejects_final_cargo_test_owned_only_by_executor() {
        let (mut captures, calls, outputs) = valid_root_history();
        let final_test = captures[0].calls.pop().unwrap();
        captures.push(CaptureReceipt {
            path: "executor.json".into(),
            actor: "executor".into(),
            calls: vec![final_test],
        });
        assert!(ensure_root_history(&captures, &calls, &outputs).is_err());
    }

    #[test]
    fn root_history_accepts_root_final_cargo_test_after_reviewer_approval() {
        let (captures, calls, outputs) = valid_root_history();
        ensure_root_history(&captures, &calls, &outputs).unwrap();
    }

    #[test]
    fn submissions_require_implementation_reads_before_cherry_pick() {
        let mut calls = vec![orchestration_call(
            "profiles",
            "list_agent_profiles",
            serde_json::json!({}),
        )];
        let mut outputs = Vec::new();
        for (index, profile) in ["explorer", "explorer", "executor", "worktree_executor"]
            .into_iter()
            .enumerate()
        {
            let spawn = format!("spawn-{index}");
            let agent = format!("agent-{index}");
            calls.push(orchestration_call(
                &spawn,
                "spawn_agent",
                serde_json::json!({"profileId":profile}),
            ));
            outputs.push(WireOutput {
                call_id: Some(spawn),
                content: serde_json::json!({"profileId":profile,"agentId":agent}).to_string(),
            });
            let read = format!("read-{index}");
            calls.push(orchestration_call(
                &read,
                "read_agent_submissions",
                serde_json::json!({"target":agent}),
            ));
            outputs.push(WireOutput {
                call_id: Some(read),
                content: submission_page(vec![serde_json::json!({"id":"submission"})], 1),
            });
        }
        calls.push(orchestration_call(
            "pick",
            "exec",
            serde_json::json!({"command":"git cherry-pick abc"}),
        ));
        ensure_submissions(&calls, &outputs).unwrap();
        let pick = calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("pick"))
            .unwrap();
        calls.swap(pick, pick - 1);
        assert!(ensure_submissions(&calls, &outputs).is_err());
    }

    #[test]
    fn empty_non_reviewer_pages_accept_bound_terminal_session_fallback() {
        let mut calls = vec![orchestration_call(
            "profiles",
            "list_agent_profiles",
            serde_json::json!({}),
        )];
        let mut outputs = Vec::new();
        for (index, profile) in ["explorer", "explorer", "executor", "worktree_executor"]
            .into_iter()
            .enumerate()
        {
            let spawn = format!("spawn-{index}");
            let agent = format!("agent-{index}");
            calls.push(orchestration_call(
                &spawn,
                "spawn_agent",
                serde_json::json!({"profileId":profile}),
            ));
            outputs.push(WireOutput {
                call_id: Some(spawn),
                content: serde_json::json!({"profileId":profile,"agentId":agent}).to_string(),
            });
            let wait = format!("wait-{index}");
            calls.push(orchestration_call(
                &wait,
                "wait_agents",
                serde_json::json!({"targets":[agent]}),
            ));
            outputs.push(terminal_wait_output(&wait, &agent));
            let read = format!("read-{index}");
            calls.push(orchestration_call(
                &read,
                "read_agent_submissions",
                serde_json::json!({"target":agent}),
            ));
            outputs.push(WireOutput {
                call_id: Some(read),
                content: submission_page(Vec::new(), 0),
            });
            let session = format!("session-{index}");
            calls.push(orchestration_call(
                &session,
                "read_agent_session",
                serde_json::json!({"target":agent}),
            ));
            outputs.push(WireOutput {
                call_id: Some(session),
                content: serde_json::json!({
                    "messages": [
                        {"role":"assistant","text":"<final>terminal child result</final>"}
                    ],
                    "truncated": false
                })
                .to_string(),
            });
        }
        calls.push(orchestration_call(
            "pick",
            "exec",
            serde_json::json!({"command":"git cherry-pick abc"}),
        ));
        ensure_submissions(&calls, &outputs).unwrap();
    }

    #[test]
    fn session_fallback_requires_binding_shape_and_phase_boundary() {
        let mut calls = vec![orchestration_call(
            "profiles",
            "list_agent_profiles",
            serde_json::json!({}),
        )];
        let mut outputs = Vec::new();
        for (index, profile) in ["explorer", "explorer", "executor", "worktree_executor"]
            .into_iter()
            .enumerate()
        {
            let spawn = format!("spawn-{index}");
            let agent = format!("agent-{index}");
            calls.push(orchestration_call(
                &spawn,
                "spawn_agent",
                serde_json::json!({"profileId":profile}),
            ));
            outputs.push(WireOutput {
                call_id: Some(spawn),
                content: serde_json::json!({"profileId":profile,"agentId":agent}).to_string(),
            });
            let wait = format!("wait-{index}");
            calls.push(orchestration_call(
                &wait,
                "wait_agents",
                serde_json::json!({"targets":[agent]}),
            ));
            outputs.push(terminal_wait_output(&wait, &agent));
            let read = format!("read-{index}");
            calls.push(orchestration_call(
                &read,
                "read_agent_submissions",
                serde_json::json!({"target":agent}),
            ));
            outputs.push(WireOutput {
                call_id: Some(read),
                content: submission_page(Vec::new(), 0),
            });
            let session = format!("session-{index}");
            calls.push(orchestration_call(
                &session,
                "read_agent_session",
                serde_json::json!({"target":agent}),
            ));
            outputs.push(WireOutput {
                call_id: Some(session),
                content: serde_json::json!({
                    "messages": [{"role":"assistant","text":"terminal child result"}]
                })
                .to_string(),
            });
        }
        calls.push(orchestration_call(
            "pick",
            "exec",
            serde_json::json!({"command":"git cherry-pick abc"}),
        ));
        let baseline_calls = calls.clone();
        let baseline_outputs = outputs.clone();
        for (call_id, mutate) in [
            ("session-0", 0),
            ("session-0", 1),
            ("session-0", 2),
            ("session-0", 3),
        ] {
            let mut invalid_calls = baseline_calls.clone();
            let mut invalid_outputs = baseline_outputs.clone();
            match mutate {
                0 => {
                    invalid_calls.retain(|call| call.call_id.as_deref() != Some(call_id));
                    invalid_outputs.retain(|output| output.call_id.as_deref() != Some(call_id));
                }
                1 => {
                    invalid_calls
                        .iter_mut()
                        .find(|call| call.call_id.as_deref() == Some(call_id))
                        .unwrap()
                        .arguments = serde_json::json!({"target":"wrong-agent"})
                }
                2 => {
                    invalid_outputs
                        .iter_mut()
                        .find(|output| output.call_id.as_deref() == Some(call_id))
                        .unwrap()
                        .call_id = Some("wrong-call-id".into())
                }
                3 => {
                    invalid_outputs
                        .iter_mut()
                        .find(|output| output.call_id.as_deref() == Some(call_id))
                        .unwrap()
                        .content = "{}".into()
                }
                _ => unreachable!(),
            }
            assert!(
                ensure_submissions(&invalid_calls, &invalid_outputs).is_err(),
                "mutation {mutate} unexpectedly accepted"
            );
        }
        for mutation in ["target", "call_id", "reason", "message_agent"] {
            let mut invalid_calls = baseline_calls.clone();
            let mut invalid_outputs = baseline_outputs.clone();
            if mutation == "target" {
                invalid_calls
                    .iter_mut()
                    .find(|call| call.call_id.as_deref() == Some("wait-0"))
                    .unwrap()
                    .arguments = serde_json::json!({"targets":["wrong-agent"]});
            } else {
                let output = invalid_outputs
                    .iter_mut()
                    .find(|output| output.call_id.as_deref() == Some("wait-0"))
                    .unwrap();
                match mutation {
                    "call_id" => output.call_id = Some("wrong-call-id".into()),
                    "reason" => {
                        output.content = serde_json::json!({
                            "messages": [{"agentId":"agent-0"}],
                            "reason":"progress"
                        })
                        .to_string()
                    }
                    "message_agent" => {
                        output.content = serde_json::json!({
                            "messages": [{"agentId":"wrong-agent"}],
                            "reason":"terminal"
                        })
                        .to_string()
                    }
                    _ => unreachable!(),
                }
            }
            assert!(
                ensure_submissions(&invalid_calls, &invalid_outputs).is_err(),
                "wait mutation {mutation} unexpectedly accepted"
            );
        }
        let mut wait_before_spawn_calls = baseline_calls.clone();
        let wait_index = wait_before_spawn_calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("wait-0"))
            .unwrap();
        let wait = wait_before_spawn_calls.remove(wait_index);
        let spawn_index = wait_before_spawn_calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("spawn-0"))
            .unwrap();
        wait_before_spawn_calls.insert(spawn_index, wait);
        assert!(ensure_submissions(&wait_before_spawn_calls, &baseline_outputs).is_err());

        let mut session_before_read_calls = baseline_calls.clone();
        let session_index = session_before_read_calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("session-2"))
            .unwrap();
        let session = session_before_read_calls.remove(session_index);
        let read_index = session_before_read_calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("read-2"))
            .unwrap();
        session_before_read_calls.insert(read_index, session);
        assert!(ensure_submissions(&session_before_read_calls, &baseline_outputs).is_err());

        let mut late_calls = baseline_calls.clone();
        let wait_index = late_calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("wait-2"))
            .unwrap();
        let wait = late_calls.remove(wait_index);
        let session_index = late_calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("session-2"))
            .unwrap();
        late_calls.insert(session_index + 1, wait);
        assert!(ensure_submissions(&late_calls, &baseline_outputs).is_err());
        let mut late_worktree_calls = baseline_calls.clone();
        let wait_index = late_worktree_calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("wait-3"))
            .unwrap();
        let wait = late_worktree_calls.remove(wait_index);
        let pick_index = late_worktree_calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("pick"))
            .unwrap();
        late_worktree_calls.insert(pick_index + 1, wait);
        assert!(ensure_submissions(&late_worktree_calls, &baseline_outputs).is_err());
        let mut late_calls = baseline_calls.clone();
        let session_index = late_calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("session-0"))
            .unwrap();
        let late_session = late_calls.remove(session_index);
        late_calls.push(late_session);
        assert!(ensure_submissions(&late_calls, &baseline_outputs).is_err());

        let mut late_worktree_session_calls = baseline_calls;
        let session_index = late_worktree_session_calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("session-3"))
            .unwrap();
        let session = late_worktree_session_calls.remove(session_index);
        let pick_index = late_worktree_session_calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("pick"))
            .unwrap();
        late_worktree_session_calls.insert(pick_index + 1, session);
        assert!(ensure_submissions(&late_worktree_session_calls, &baseline_outputs).is_err());
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
        let outputs = profile_message_outputs(&canonical);
        assert!(ensure_profile_messages(&canonical, &outputs).is_err());
    }

    #[test]
    fn profile_message_contract_requires_all_profiles_and_sections() {
        let calls = profile_message_calls();
        let outputs = profile_message_outputs(&calls);
        ensure_profile_messages(&calls, &outputs).unwrap();
    }

    #[test]
    fn profile_messages_ignore_failed_explorer_retry_but_reject_third_success() {
        let mut calls = profile_message_calls();
        let failed = WireCall {
            call_id: Some("explorer-failed".into()),
            name: "spawn_agent".into(),
            arguments: serde_json::json!({
                "profileId": "explorer",
                "forkTurns": "none",
                "message": "failed attempt may not satisfy the child contract"
            }),
        };
        calls.insert(0, failed);
        let mut outputs = profile_message_outputs(&calls[1..]);
        outputs.push(WireOutput {
            call_id: Some("explorer-failed".into()),
            content: "Tool execution error: capacity unavailable".into(),
        });
        ensure_profile_messages(&calls, &outputs).unwrap();

        let third = calls[2].clone();
        calls.push(WireCall {
            call_id: Some("explorer-success-3".into()),
            ..third
        });
        outputs.push(spawn_output(
            "explorer-success-3",
            "explorer",
            "explorer-success-3",
        ));
        assert!(ensure_profile_messages(&calls, &outputs).is_err());
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
        let outputs = profile_message_outputs(&calls);
        assert!(ensure_profile_messages(&calls, &outputs).is_err());

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
        let outputs = profile_message_outputs(&complete);
        ensure_profile_messages(&complete, &outputs).unwrap();
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
            let message = if profile == "explorer" && id == "explorer-1" {
                message.replace(
                    "[[CHILD_CONTRACT:steps]]\ncontent",
                    &format!("[[CHILD_CONTRACT:steps]]\n{EXPLORER_STEPS_FIXTURE_SOURCE_V1}"),
                )
            } else if profile == "explorer" && id == "explorer-2" {
                message
                    .replace(
                        "[[CHILD_CONTRACT:purpose]]\ncontent",
                        "[[CHILD_CONTRACT:purpose]]\nsecond purpose",
                    )
                    .replace(
                        "[[CHILD_CONTRACT:ownership]]\ncontent",
                        "[[CHILD_CONTRACT:ownership]]\nsecond ownership",
                    )
                    .replace(
                        "[[CHILD_CONTRACT:steps]]\ncontent",
                        &format!("[[CHILD_CONTRACT:steps]]\n{EXPLORER_STEPS_WORKSPACE_GIT_V1}"),
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

    fn profile_message_outputs(calls: &[WireCall]) -> Vec<WireOutput> {
        calls
            .iter()
            .filter(|call| call.name == "spawn_agent")
            .map(|call| WireOutput {
                call_id: call.call_id.clone(),
                content: serde_json::json!({
                    "profileId": call.arguments["profileId"],
                    "agentId": call.call_id,
                })
                .to_string(),
            })
            .collect()
    }

    fn orchestration_call(id: &str, name: &str, arguments: Value) -> WireCall {
        WireCall {
            call_id: Some(id.into()),
            name: name.into(),
            arguments,
        }
    }

    fn orchestration_planning_calls() -> Vec<WireCall> {
        vec![
            orchestration_call("profiles", "list_agent_profiles", serde_json::json!({})),
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
            orchestration_call(
                "w1",
                "wait_agents",
                serde_json::json!({"targets":["explorer-a","explorer-b"]}),
            ),
            orchestration_call(
                "w1b",
                "wait_agents",
                serde_json::json!({"targets":["explorer-b"]}),
            ),
            orchestration_call(
                "er1",
                "read_agent_submissions",
                serde_json::json!({"target":"explorer-a"}),
            ),
            orchestration_call(
                "er2",
                "read_agent_submissions",
                serde_json::json!({"target":"explorer-b"}),
            ),
            orchestration_call("confirm", "request_user_input", serde_json::json!({})),
            orchestration_call(
                "design",
                "write_file",
                serde_json::json!({"path":"design/subagents-orchestration.md"}),
            ),
        ]
    }

    fn spawn_output(call_id: &str, profile: &str, agent_id: &str) -> WireOutput {
        WireOutput {
            call_id: Some(call_id.into()),
            content: serde_json::json!({
                "profileId": profile,
                "agentId": agent_id,
            })
            .to_string(),
        }
    }

    fn terminal_wait_output(call_id: &str, agent_id: &str) -> WireOutput {
        WireOutput {
            call_id: Some(call_id.into()),
            content: serde_json::json!({
                "messages": [{
                    "agentId": agent_id,
                    "state": {
                        "agent": {"data": null, "kind": "idle"},
                        "lastTurnOutcome": {"outcome": {"kind": "completed"}},
                    },
                }],
                "reason": "terminal",
            })
            .to_string(),
        }
    }

    #[test]
    fn explorer_messages_reject_third_explorer_and_any_steps_suffix() {
        let mut calls = profile_message_calls();
        let mut third = calls[0].clone();
        third.call_id = Some("explorer-3".into());
        calls.push(third);
        let outputs = profile_message_outputs(&calls);
        assert!(ensure_profile_messages(&calls, &outputs).is_err());

        for extra in [
            "查看 README.md",
            "检查 Cargo.lock",
            "使用 unexpected_tool",
            "禁止调用 list_agent_profiles",
            "改写目标",
        ] {
            let mut calls = profile_message_calls();
            let message = calls[0].arguments["message"].as_str().unwrap();
            calls[0].arguments["message"] =
                Value::String(message.replace("。", &format!("。{extra}")));
            let outputs = profile_message_outputs(&calls);
            assert!(
                ensure_profile_messages(&calls, &outputs).is_err(),
                "accepted `{extra}`"
            );
        }

        let mut calls = profile_message_calls();
        let message = calls[1].arguments["message"].as_str().unwrap();
        calls[1].arguments["message"] = Value::String(message.replace("。", "。读取 Cargo.lock"));
        let outputs = profile_message_outputs(&calls);
        assert!(ensure_profile_messages(&calls, &outputs).is_err());
    }

    #[test]
    fn explorer_messages_accept_crlf_and_outer_whitespace() {
        let mut calls = profile_message_calls();
        for call in calls.iter_mut().take(2) {
            let message = call.arguments["message"]
                .as_str()
                .unwrap()
                .replace('\n', "\r\n");
            call.arguments["message"] = Value::String(format!("  {message}  "));
        }
        let outputs = profile_message_outputs(&calls);
        ensure_profile_messages(&calls, &outputs).unwrap();
    }

    #[test]
    fn explorer_steps_accept_only_one_outer_text_fence() {
        let canonical = EXPLORER_STEPS_FIXTURE_SOURCE_V1;
        assert_eq!(normalize_explorer_steps(canonical), canonical);
        assert_eq!(
            normalize_explorer_steps(&format!("```text\n{canonical}\n```")),
            canonical
        );
        assert_eq!(
            normalize_explorer_steps(&format!("  ```text\r\n{canonical}\r\n```  ")),
            canonical
        );
        let mut calls = profile_message_calls();
        let message = calls[0].arguments["message"].as_str().unwrap();
        calls[0].arguments["message"] =
            Value::String(message.replace(canonical, &format!("```text\n{canonical}\n```")));
        let outputs = profile_message_outputs(&calls);
        ensure_profile_messages(&calls, &outputs).unwrap();

        for invalid in [
            format!("```markdown\n{canonical}\n```"),
            format!("```text\n{canonical}"),
            format!("```text\n{canonical}\n```\nextra"),
            format!("extra\n```text\n{canonical}\n```"),
            format!("```text\n```text\n{canonical}\n```\n```"),
            format!("```text\n{canonical}\n\nextra action\n```"),
        ] {
            assert_ne!(
                normalize_explorer_steps(&invalid),
                canonical,
                "accepted {invalid:?}"
            );
        }
    }

    fn valid_orchestration_calls() -> (Vec<WireCall>, Vec<WireOutput>) {
        let mut calls = orchestration_planning_calls();
        calls.extend([
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
        ]);
        let outputs = vec![
            spawn_output("e1", "explorer", "explorer-a"),
            spawn_output("e2", "explorer", "explorer-b"),
            terminal_wait_output("w1", "explorer-a"),
            terminal_wait_output("w1b", "explorer-b"),
            spawn_output("x1", "executor", "executor-a"),
            spawn_output("x2", "worktree_executor", "worktree-a"),
            spawn_output("r1", "reviewer", "reviewer-a"),
        ];
        (calls, outputs)
    }

    fn swap_calls(calls: &mut [WireCall], left: &str, right: &str) {
        let left = calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some(left))
            .unwrap();
        let right = calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some(right))
            .unwrap();
        calls.swap(left, right);
    }

    #[test]
    fn orchestration_order_accepts_explorer_wait_then_both_implementations() {
        let (calls, outputs) = valid_orchestration_calls();
        ensure_orchestration_order(&calls, &outputs).unwrap();
    }

    #[test]
    fn orchestration_order_rejects_wait_after_only_one_implementation_spawn() {
        let (mut calls, outputs) = valid_orchestration_calls();
        swap_calls(&mut calls, "x2", "w2");
        assert!(ensure_orchestration_order(&calls, &outputs).is_err());
    }

    #[test]
    fn orchestration_order_rejects_ordinary_call_between_explorer_spawns() {
        let (mut calls, outputs) = valid_orchestration_calls();
        calls.insert(
            2,
            orchestration_call("interposed", "git_status", serde_json::json!({})),
        );
        assert!(ensure_orchestration_order(&calls, &outputs).is_err());
    }

    #[test]
    fn orchestration_order_rejects_ordinary_call_between_implementation_spawns() {
        let (mut calls, outputs) = valid_orchestration_calls();
        let worktree = calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("x2"))
            .unwrap();
        calls.insert(
            worktree,
            orchestration_call("interposed", "git_status", serde_json::json!({})),
        );
        assert!(ensure_orchestration_order(&calls, &outputs).is_err());
    }

    #[test]
    fn orchestration_order_rejects_confirmation_before_all_explorer_reads() {
        let (mut calls, outputs) = valid_orchestration_calls();
        swap_calls(&mut calls, "er2", "confirm");
        assert!(ensure_orchestration_order(&calls, &outputs).is_err());
    }

    #[test]
    fn orchestration_order_rejects_submission_reads_for_unbound_targets() {
        let (mut calls, outputs) = valid_orchestration_calls();
        for call in calls
            .iter_mut()
            .filter(|call| call.name == "read_agent_submissions")
        {
            call.arguments["target"] = Value::String(format!(
                "fake-{}",
                call.arguments["target"].as_str().unwrap()
            ));
        }
        assert!(ensure_orchestration_order(&calls, &outputs).is_err());
    }

    #[test]
    fn orchestration_order_rejects_waits_for_unbound_targets() {
        let (mut calls, outputs) = valid_orchestration_calls();
        for call in calls.iter_mut().filter(|call| call.name == "wait_agents") {
            call.arguments["targets"] = serde_json::json!(["fake-a", "fake-b"]);
        }
        assert!(ensure_orchestration_order(&calls, &outputs).is_err());
    }

    #[test]
    fn orchestration_order_requires_terminal_wait_output_for_each_explorer() {
        let (calls, mut outputs) = valid_orchestration_calls();
        let second_wait = outputs
            .iter_mut()
            .find(|output| output.call_id.as_deref() == Some("w1b"))
            .unwrap();
        second_wait.content = serde_json::json!({
            "messages": [{"agentId": "explorer-b"}],
            "reason": "progress",
        })
        .to_string();
        assert!(ensure_orchestration_order(&calls, &outputs).is_err());
    }

    #[test]
    fn orchestration_order_ignores_failed_spawn_receipt() {
        let (mut calls, mut outputs) = valid_orchestration_calls();
        calls.insert(
            1,
            orchestration_call(
                "failed-explorer",
                "spawn_agent",
                serde_json::json!({"profileId":"explorer"}),
            ),
        );
        outputs.push(WireOutput {
            call_id: Some("failed-explorer".into()),
            content: "Tool execution error: capacity unavailable".into(),
        });
        ensure_orchestration_order(&calls, &outputs).unwrap();
    }

    #[test]
    fn orchestration_order_rejects_design_write_before_confirmation() {
        let (mut calls, outputs) = valid_orchestration_calls();
        swap_calls(&mut calls, "confirm", "design");
        assert!(ensure_orchestration_order(&calls, &outputs).is_err());
    }

    #[test]
    fn orchestration_order_rejects_cleanup_before_cherry_pick() {
        let (mut calls, outputs) = valid_orchestration_calls();
        swap_calls(&mut calls, "i1", "c1");
        assert!(ensure_orchestration_order(&calls, &outputs).is_err());
    }

    fn single_reviewer_calls(target: &str) -> Vec<WireCall> {
        vec![
            orchestration_call(
                "r1",
                "spawn_agent",
                serde_json::json!({"profileId":"reviewer"}),
            ),
            orchestration_call(
                "rr1",
                "read_agent_submissions",
                serde_json::json!({"target":target}),
            ),
        ]
    }

    fn single_reviewer_outputs(read: WireOutput) -> Vec<WireOutput> {
        vec![reviewer_receipt("r1", "reviewer-a"), read]
    }

    fn approval_output(call_id: Option<&str>) -> WireOutput {
        reviewer_read_output(
            call_id,
            vec![submission_item(&[(
                "detail",
                "REVIEWER_READ_ONLY_APPROVED",
            )])],
            1,
        )
    }

    fn finding_output(call_id: &str) -> WireOutput {
        reviewer_read_output(
            Some(call_id),
            vec![submission_item(&[("detail", "REVIEWER_FINDING")])],
            1,
        )
    }

    fn markerless_output(call_id: &str) -> WireOutput {
        reviewer_read_output(
            Some(call_id),
            vec![submission_item(&[("detail", "review notes")])],
            1,
        )
    }

    fn executor_receipt(call_id: &str, agent_id: &str) -> WireOutput {
        WireOutput {
            call_id: Some(call_id.into()),
            content: serde_json::json!({
                "profileId": "executor",
                "agentId": agent_id,
            })
            .to_string(),
        }
    }

    fn valid_finding_re_review() -> (Vec<WireCall>, Vec<WireOutput>) {
        let calls = vec![
            orchestration_call(
                "r1",
                "spawn_agent",
                serde_json::json!({"profileId":"reviewer"}),
            ),
            orchestration_call(
                "rr1",
                "read_agent_submissions",
                serde_json::json!({"target":"reviewer-a"}),
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
            orchestration_call(
                "rr2",
                "read_agent_submissions",
                serde_json::json!({"target":"reviewer-b"}),
            ),
        ];
        let outputs = vec![
            reviewer_receipt("r1", "reviewer-a"),
            finding_output("rr1"),
            executor_receipt("x1", "executor-a"),
            reviewer_receipt("r2", "reviewer-b"),
            approval_output(Some("rr2")),
        ];
        (calls, outputs)
    }

    #[test]
    fn reviewer_approval_requires_exact_target_agent_id() {
        let calls = single_reviewer_calls("another-agent");
        let outputs = single_reviewer_outputs(approval_output(Some("rr1")));
        assert!(ensure_finding_re_review(&calls, &outputs).is_err());
    }

    #[test]
    fn reviewer_approval_requires_same_call_id_output() {
        let calls = single_reviewer_calls("reviewer-a");
        let outputs = single_reviewer_outputs(approval_output(Some("another-read")));
        assert!(ensure_finding_re_review(&calls, &outputs).is_err());
    }

    #[test]
    fn reviewer_approval_rejects_output_without_call_id() {
        let calls = single_reviewer_calls("reviewer-a");
        let outputs = single_reviewer_outputs(approval_output(None));
        assert!(ensure_finding_re_review(&calls, &outputs).is_err());
    }

    #[test]
    fn reviewer_unbound_arbitrary_approval_does_not_authorize() {
        let calls = single_reviewer_calls("reviewer-a");
        let mut outputs = single_reviewer_outputs(markerless_output("rr1"));
        outputs.push(approval_output(None));
        assert!(ensure_finding_re_review(&calls, &outputs).is_err());
    }

    #[test]
    fn reviewer_empty_submission_page_does_not_authorize() {
        let calls = single_reviewer_calls("reviewer-a");
        let outputs = single_reviewer_outputs(reviewer_read_output(Some("rr1"), vec![], 0));
        assert!(ensure_finding_re_review(&calls, &outputs).is_err());
    }

    #[test]
    fn reviewer_zero_total_page_with_marker_does_not_authorize() {
        let calls = single_reviewer_calls("reviewer-a");
        let outputs = single_reviewer_outputs(reviewer_read_output(
            Some("rr1"),
            vec![submission_item(&[(
                "detail",
                "REVIEWER_READ_ONLY_APPROVED",
            )])],
            0,
        ));
        assert!(ensure_finding_re_review(&calls, &outputs).is_err());
    }

    #[test]
    fn reviewer_polling_skips_empty_page_until_bound_approval() {
        let mut calls = single_reviewer_calls("reviewer-a");
        calls.push(orchestration_call(
            "rr2",
            "read_agent_submissions",
            serde_json::json!({"target":"reviewer-a"}),
        ));
        let outputs = vec![
            reviewer_receipt("r1", "reviewer-a"),
            reviewer_read_output(Some("rr1"), vec![], 0),
            approval_output(Some("rr2")),
        ];
        ensure_finding_re_review(&calls, &outputs).unwrap();
    }

    #[test]
    fn reviewer_polling_skips_multiple_markerless_pages_until_bound_approval() {
        let mut calls = single_reviewer_calls("reviewer-a");
        calls.push(orchestration_call(
            "rr2",
            "read_agent_submissions",
            serde_json::json!({"target":"reviewer-a"}),
        ));
        calls.push(orchestration_call(
            "rr3",
            "read_agent_submissions",
            serde_json::json!({"target":"reviewer-a"}),
        ));
        let outputs = vec![
            reviewer_receipt("r1", "reviewer-a"),
            markerless_output("rr1"),
            markerless_output("rr2"),
            approval_output(Some("rr3")),
        ];
        ensure_finding_re_review(&calls, &outputs).unwrap();
    }

    #[test]
    fn reviewer_marker_outside_payload_fields_does_not_authorize() {
        let calls = single_reviewer_calls("reviewer-a");
        let outputs = single_reviewer_outputs(reviewer_read_output(
            Some("rr1"),
            vec![submission_item(&[("stage", "REVIEWER_READ_ONLY_APPROVED")])],
            1,
        ));
        assert!(ensure_finding_re_review(&calls, &outputs).is_err());
    }

    #[test]
    fn reviewer_marker_is_accepted_from_each_payload_field() {
        for field in ["summary", "nextStep", "detail"] {
            let calls = single_reviewer_calls("reviewer-a");
            let outputs = single_reviewer_outputs(reviewer_read_output(
                Some("rr1"),
                vec![submission_item(&[(field, "REVIEWER_READ_ONLY_APPROVED")])],
                1,
            ));
            ensure_finding_re_review(&calls, &outputs).unwrap();
        }
    }

    #[test]
    fn finding_re_review_without_finding_allows_one_reviewer() {
        let calls = single_reviewer_calls("reviewer-a");
        let outputs = single_reviewer_outputs(approval_output(Some("rr1")));
        ensure_finding_re_review(&calls, &outputs).unwrap();
    }

    #[test]
    fn finding_re_review_requires_second_reviewer() {
        let calls = single_reviewer_calls("reviewer-a");
        let outputs = single_reviewer_outputs(finding_output("rr1"));
        assert!(ensure_finding_re_review(&calls, &outputs).is_err());
    }

    #[test]
    fn finding_re_review_accepts_complete_rework_sequence() {
        let (calls, outputs) = valid_finding_re_review();
        ensure_finding_re_review(&calls, &outputs).unwrap();
    }

    #[test]
    fn finding_re_review_requires_implementation_after_actual_finding_read() {
        let (mut calls, outputs) = valid_finding_re_review();
        calls.swap(1, 2);
        assert!(ensure_finding_re_review(&calls, &outputs).is_err());
    }

    #[test]
    fn finding_re_review_requires_integration_after_implementation() {
        let (mut calls, outputs) = valid_finding_re_review();
        calls.swap(2, 3);
        assert!(ensure_finding_re_review(&calls, &outputs).is_err());
    }

    #[test]
    fn finding_re_review_uses_last_non_final_finding_as_repair_boundary() {
        let (mut calls, mut outputs) = valid_finding_re_review();
        calls[4].arguments["profileId"] = Value::String("reviewer".into());
        calls[5].arguments["target"] = Value::String("reviewer-b".into());
        outputs.pop();
        outputs.push(finding_output("rr2"));
        calls.push(orchestration_call(
            "r3",
            "spawn_agent",
            serde_json::json!({"profileId":"reviewer"}),
        ));
        calls.push(orchestration_call(
            "rr3",
            "read_agent_submissions",
            serde_json::json!({"target":"reviewer-c"}),
        ));
        outputs.push(reviewer_receipt("r3", "reviewer-c"));
        outputs.push(approval_output(Some("rr3")));
        assert!(ensure_finding_re_review(&calls, &outputs).is_err());
    }

    #[test]
    fn finding_re_review_rejects_first_reviewer_approval_for_final_reviewer() {
        let (mut calls, mut outputs) = valid_finding_re_review();
        calls.insert(
            5,
            orchestration_call(
                "rr1-approval",
                "read_agent_submissions",
                serde_json::json!({"target":"reviewer-a"}),
            ),
        );
        outputs.pop();
        outputs.push(approval_output(Some("rr1-approval")));
        outputs.push(markerless_output("rr2"));
        assert!(ensure_finding_re_review(&calls, &outputs).is_err());
    }

    #[test]
    fn finding_re_review_rejects_unbound_approval_for_final_reviewer() {
        let (calls, mut outputs) = valid_finding_re_review();
        outputs.pop();
        outputs.push(markerless_output("rr2"));
        outputs.push(approval_output(None));
        assert!(ensure_finding_re_review(&calls, &outputs).is_err());
    }

    #[test]
    fn finding_re_review_requires_different_final_reviewer_agent_id() {
        let (mut calls, mut outputs) = valid_finding_re_review();
        calls[5].arguments["target"] = Value::String("reviewer-a".into());
        outputs
            .iter_mut()
            .find(|output| output.call_id.as_deref() == Some("r2"))
            .unwrap()
            .content = serde_json::json!({
            "profileId": "reviewer",
            "agentId": "reviewer-a",
        })
        .to_string();
        assert!(ensure_finding_re_review(&calls, &outputs).is_err());
    }
}
