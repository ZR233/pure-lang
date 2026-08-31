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
    output_markers: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireCall {
    name: String,
    arguments: Value,
}

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
    let mut calls = Vec::new();
    let mut outputs = Vec::new();
    for path in &captures {
        let capture: Value = serde_json::from_slice(&fs::read(path)?)?;
        let body = capture
            .get("wireBody")
            .context("wire capture has no body")?;
        collect_calls(body, &mut calls);
        collect_outputs(body, &mut outputs);
    }
    let ordered_calls = calls.clone();
    ensure_orchestration_order(&ordered_calls)?;
    deduplicate_calls(&mut calls);
    ensure_spawn_calls(&calls)?;
    ensure_workspace_receipts(&outputs)?;
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
    let output = outputs.join("\n");
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

fn ensure_workspace_receipts(outputs: &[String]) -> Result<()> {
    let receipts = outputs
        .iter()
        .filter_map(|output| serde_json::from_str::<Value>(output).ok())
        .collect::<Vec<_>>();
    let directory = receipts
        .iter()
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
    let first_wait = calls
        .iter()
        .position(|call| {
            matches!(
                call.name.as_str(),
                "wait_agents" | "read_agent_session" | "read_agent_submissions"
            )
        })
        .context("wire captures contain no child wait/read operation")?;
    let explorers = spawn_indices("explorer");
    ensure!(
        explorers.len() >= 2,
        "wire captures contain fewer than two explorer spawns"
    );
    ensure!(
        explorers[1] < first_wait,
        "explorer spawns were not both issued before the first wait/read"
    );
    let executors = spawn_indices("executor");
    let worktrees = spawn_indices("worktree_executor");
    ensure!(
        !executors.is_empty() && !worktrees.is_empty(),
        "wire captures contain no implementation profiles"
    );
    let implementation_last = *executors.iter().chain(worktrees.iter()).max().unwrap();
    ensure!(
        calls
            .iter()
            .enumerate()
            .skip(implementation_last + 1)
            .any(|(_, call)| matches!(
                call.name.as_str(),
                "wait_agents" | "read_agent_session" | "read_agent_submissions"
            )),
        "implementation spawns were not issued before waiting"
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
    ensure!(
        reviewer[0] > cherry_pick && reviewer[0] > cleanup,
        "reviewer was not spawned after integration and cleanup"
    );
    ensure!(
        calls[reviewer[0]].arguments.get("writablePaths").is_none(),
        "reviewer spawn unexpectedly requested writablePaths"
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

fn collect_outputs(body: &Value, outputs: &mut Vec<String>) {
    for item in body
        .get("input")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if item.get("type").and_then(Value::as_str) == Some("function_call_output")
            && let Some(output) = item.get("output").and_then(Value::as_str)
        {
            outputs.push(output.to_string());
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
            outputs.push(output.to_string());
        }
    }
}

fn deduplicate_calls(calls: &mut Vec<WireCall>) {
    calls.sort_by(|left, right| {
        (&left.name, left.arguments.to_string()).cmp(&(&right.name, right.arguments.to_string()))
    });
    calls.dedup_by(|left, right| left.name == right.name && left.arguments == right.arguments);
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
                name: "spawn_agent".to_string(),
                arguments: serde_json::json!({
                    "profileId": "executor",
                    "writablePaths": ["allowed"]
                }),
            },
            WireCall {
                name: "spawn_agent".to_string(),
                arguments: serde_json::json!({"profileId": "worktree_executor"}),
            },
        ];
        ensure_spawn_calls(&calls).unwrap();
        ensure_workspace_receipts(&[
            serde_json::json!({
                "profileId": "executor",
                "workspace": {
                    "mode": "directory",
                    "writablePaths": ["/tmp/fixture/allowed"]
                }
            })
            .to_string(),
            serde_json::json!({
                "profileId": "worktree_executor",
                "workspace": {
                    "mode": "worktree",
                    "worktree": {
                        "branch": "pure-agent-child",
                        "baseCommit": "0123456789abcdef0123456789abcdef01234567"
                    }
                }
            })
            .to_string(),
        ])
        .unwrap();
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
}
