//! Real native-GUI acceptance for directory and worktree child Agents.

use super::{
    copy_directory, current_home, resident, unix_nanos, user_config_state,
    write_isolated_live_config,
};
use crate::cli::VerifySubagentsOptions;
use crate::{paths, process};
use anyhow::{Context, Result, bail, ensure};
use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const TOTAL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
// One HTTP attempt has a five-minute client deadline. The acceptance needs
// enough time to record that boundary, but must not spend the full retry
// budget when the provider never opens a stream.
const STALL_TIMEOUT_SECONDS: u64 = 6 * 60;
const SENTINEL: &str = "PURE_SUBAGENTS_LIVE_OK";
const DIRECTORY_MARKER: &str = "DIRECTORY_MARKER";
const WORKTREE_RESULT_MARKER: &str = "WORKTREE_RESULT_MARKER";
const SSH_SERVER_ENV: &str = "PURE_SUBAGENTS_SSH_SERVER";
const SSH_USERNAME_ENV: &str = "PURE_SUBAGENTS_SSH_USERNAME";
const SSH_PASSWORD_ENV: &str = "PURE_SUBAGENTS_SSH_PASSWORD";
const SSH_PORT_ENV: &str = "PURE_SUBAGENTS_SSH_PORT";
const SSH_ACCEPTANCE_NAME: &str = "Pure SSH Acceptance";
#[derive(Debug, Clone)]
struct Route {
    provider: String,
    model: String,
}

#[derive(Clone)]
struct SshAcceptance {
    host: String,
    username: String,
    password: String,
    port: u16,
}

struct RemoteFixture {
    path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireReceipt {
    schema_version: u32,
    capture_count: usize,
    attempt_count: usize,
    expected_rejection_count: usize,
    unexpected_failure_count: usize,
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
    let ssh = read_ssh_acceptance()?;
    let root = tempfile::Builder::new()
        .prefix("pure-subagents-live-gui-")
        .tempdir()
        .context("failed to create isolated subagents acceptance root")?;
    let studio_home = root.path().join("studio-home");
    fs::create_dir_all(&studio_home)?;
    let local_fixture = root.path().join("workspace");
    let remote_fixture = match ssh.as_ref() {
        Some(ssh) => Some(prepare_remote_fixture(ssh)?),
        None => {
            prepare_fixture(&local_fixture)?;
            None
        }
    };
    let fixture = remote_fixture.as_ref().map_or_else(
        || local_fixture.clone(),
        |fixture| PathBuf::from(&fixture.path),
    );
    let isolated_config = studio_home.join("config.toml");
    write_isolated_live_config(
        &installed_config,
        &isolated_config,
        &artifact_dir.join("model-routes.json"),
    )?;
    configure_live_acceptance(&isolated_config)?;
    fs::write(
        artifact_dir.join("permission-mode.txt"),
        "permissionMode=full-access\nprofileWorkspacePolicies=preserved\n",
    )?;
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
        format!(
            "surface=gui\nscriptedProvider=false\nlive=true\nfixture={}\ntransport={}\n",
            if remote_fixture.is_some() {
                "isolatedRemoteGit"
            } else {
                "isolatedGit"
            },
            if remote_fixture.is_some() {
                "ssh"
            } else {
                "local"
            }
        ),
    )?;

    let mut acceptance = run_gui(GuiRun {
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
        ssh: ssh.as_ref(),
        remote_fixture: remote_fixture.as_ref(),
        deadline,
    })
    .and_then(|_| validate_wire(&wire_dir, &artifact_dir, ssh.is_some()))
    .and_then(|_| match (&ssh, &remote_fixture) {
        (Some(ssh), Some(fixture)) => validate_remote_fixture(ssh, fixture, &artifact_dir),
        (None, None) => validate_fixture(&fixture, &artifact_dir),
        _ => unreachable!("SSH acceptance and remote fixture are created together"),
    });
    if let (Some(ssh), Some(fixture)) = (&ssh, &remote_fixture)
        && let Err(cleanup_error) = cleanup_remote_fixture(ssh, fixture, &artifact_dir)
    {
        acceptance = Err(match acceptance {
            Ok(()) => cleanup_error,
            Err(error) => error.context(format!(
                "remote fixture cleanup also failed: {cleanup_error:#}"
            )),
        });
    }
    if let Some(ssh) = &ssh
        && let Err(leak_error) = verify_ssh_credential_storage(ssh, &studio_home, &artifact_dir)
    {
        acceptance = Err(match acceptance {
            Ok(()) => leak_error,
            Err(error) => {
                error.context(format!("secret leakage check also failed: {leak_error:#}"))
            }
        });
    }

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
    ssh: Option<&'a SshAcceptance>,
    remote_fixture: Option<&'a RemoteFixture>,
    deadline: Instant,
}

fn run_gui(run: GuiRun<'_>) -> Result<()> {
    let mut command = Command::new("cargo");
    command
        .args(["xtask", "run-gui", "--driver", "--log-level", "debug"])
        .current_dir(run.workspace_root)
        .env_remove(SSH_SERVER_ENV)
        .env_remove(SSH_USERNAME_ENV)
        .env_remove(SSH_PASSWORD_ENV)
        .env_remove(SSH_PORT_ENV)
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
        if let Some(ssh) = run.ssh {
            driver.env(SSH_PASSWORD_ENV, &ssh.password);
        } else {
            driver.env_remove(SSH_PASSWORD_ENV);
        }
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
            run.fixture.to_string_lossy().as_ref(),
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
    if let (Some(ssh), Some(fixture)) = (run.ssh, run.remote_fixture) {
        let port = ssh.port.to_string();
        for (name, value) in [
            ("--ssh-host", ssh.host.as_str()),
            ("--ssh-username", ssh.username.as_str()),
            ("--ssh-port", port.as_str()),
            ("--ssh-workspace", fixture.path.as_str()),
        ] {
            args.push(OsString::from(name));
            args.push(OsString::from(value));
        }
    }
    args
}

fn write_driver_receipt(log: &Path, output: &Path, expected_workspace: &str) -> Result<()> {
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
    validate_driver_snapshot(&completed, expected_workspace)?;
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

fn validate_driver_snapshot(completed: &Value, expected_workspace: &str) -> Result<()> {
    ensure!(
        completed
            .get("project")
            .and_then(|project| project.get("path"))
            .and_then(Value::as_str)
            == Some(expected_workspace),
        "Flutter Driver terminal project is not the expected workspace"
    );
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
    run_git(path, &["config", "user.name", "Pure Acceptance"])?;
    run_git(
        path,
        &["config", "user.email", "pure-acceptance@example.invalid"],
    )?;
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

fn read_ssh_acceptance() -> Result<Option<SshAcceptance>> {
    let configured = [
        SSH_SERVER_ENV,
        SSH_USERNAME_ENV,
        SSH_PASSWORD_ENV,
        SSH_PORT_ENV,
    ]
    .into_iter()
    .any(|key| std::env::var_os(key).is_some());
    if !configured {
        return Ok(None);
    }
    ensure!(
        cfg!(unix),
        "SSH subagents acceptance currently requires a Unix Askpass host"
    );
    let required = |key: &str| -> Result<String> {
        std::env::var(key)
            .with_context(|| format!("{key} is required when SSH subagents acceptance is enabled"))
            .and_then(|value| {
                ensure!(!value.is_empty(), "{key} must not be empty");
                Ok(value)
            })
    };
    let port = match std::env::var(SSH_PORT_ENV) {
        Ok(value) => value
            .parse::<u16>()
            .with_context(|| format!("{SSH_PORT_ENV} must be a valid non-zero port"))?,
        Err(std::env::VarError::NotPresent) => 22,
        Err(error) => return Err(error).context(format!("failed to read {SSH_PORT_ENV}")),
    };
    ensure!(port > 0, "{SSH_PORT_ENV} must be a valid non-zero port");
    Ok(Some(SshAcceptance {
        host: required(SSH_SERVER_ENV)?,
        username: required(SSH_USERNAME_ENV)?,
        password: required(SSH_PASSWORD_ENV)?,
        port,
    }))
}

fn prepare_remote_fixture(ssh: &SshAcceptance) -> Result<RemoteFixture> {
    let script = r#"set -eu
fixture=$(mktemp -d "${HOME%/}/pure-subagents-live.XXXXXX")
cleanup() { rm -rf -- "$fixture"; }
trap cleanup EXIT
mkdir -p "$fixture/src" "$fixture/design" "$fixture/allowed" "$fixture/forbidden"
printf '%s\n' '[package]' 'name = "subagents-live-fixture"' 'version = "0.1.0"' 'edition = "2024"' > "$fixture/Cargo.toml"
printf '%s\n' 'pub fn fixture_ready() -> bool { true }' '' '#[cfg(test)]' 'mod tests {' '    #[test]' '    fn fixture_is_ready() { assert!(super::fixture_ready()); }' '}' > "$fixture/src/lib.rs"
: > "$fixture/allowed/.gitkeep"
: > "$fixture/forbidden/.gitkeep"
printf '%s\n' '/target/' '/.pure/' > "$fixture/.gitignore"
git -C "$fixture" init -q -b main
git -C "$fixture" config user.name 'Pure Acceptance'
git -C "$fixture" config user.email 'pure-acceptance@example.invalid'
git -C "$fixture" add .
git -C "$fixture" -c user.name='Pure Acceptance' -c user.email='pure-acceptance@example.invalid' commit -q -m 'test: initialize subagents fixture'
trap - EXIT
printf '%s\n' "$fixture"
"#;
    let output = run_ssh_script(ssh, script)?;
    ensure_ssh_success(ssh, &output, "prepare remote subagents fixture")?;
    let path = String::from_utf8(output.stdout)?
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .context("remote fixture preparation returned no canonical path")?
        .trim()
        .to_string();
    validate_remote_fixture_path(&path)?;
    Ok(RemoteFixture { path })
}

fn validate_remote_fixture(
    ssh: &SshAcceptance,
    fixture: &RemoteFixture,
    artifacts: &Path,
) -> Result<()> {
    validate_remote_fixture_path(&fixture.path)?;
    let path = shell_quote(&fixture.path);
    let script = format!(
        r#"set -eu
fixture={path}
PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
export PATH
test -d "$fixture/.git"
grep -Fq 'ROOT_DESIGN_MARKER' "$fixture/design/subagents-orchestration.md"
grep -Fq 'DIRECTORY_MARKER' "$fixture/allowed/directory.txt"
test ! -e "$fixture/forbidden/denied.txt"
grep -Fq 'WORKTREE_RESULT_MARKER' "$fixture/worktree_result.txt"
test -n "$(git -C "$fixture" log --format='%H' -- worktree_result.txt)"
test -z "$(git -C "$fixture" branch --format='%(refname:short)' | grep '^pure-agent-' || true)"
test "$(git -C "$fixture" worktree list --porcelain | grep -c '^worktree ')" -eq 1
cargo test --manifest-path "$fixture/Cargo.toml"
printf '%s\n' 'REMOTE_SUBAGENTS_FIXTURE_OK' '--- git status ---'
git -C "$fixture" status --short
printf '%s\n' '--- git log ---'
git -C "$fixture" log --format='%H%x09%s'
printf '%s\n' '--- worktrees ---'
git -C "$fixture" worktree list --porcelain
printf '%s\n' '--- design ---'
cat "$fixture/design/subagents-orchestration.md"
printf '%s\n' '--- directory ---'
cat "$fixture/allowed/directory.txt"
printf '%s\n' '--- worktree ---'
cat "$fixture/worktree_result.txt"
"#
    );
    let output = run_ssh_script(ssh, &script)?;
    fs::write(
        artifacts.join("remote-fixture-validation.stdout.log"),
        &output.stdout,
    )?;
    fs::write(
        artifacts.join("remote-fixture-validation.stderr.log"),
        &output.stderr,
    )?;
    ensure_ssh_success(ssh, &output, "validate remote subagents fixture")?;
    let rendered = String::from_utf8(output.stdout)?;
    ensure!(
        rendered.contains("REMOTE_SUBAGENTS_FIXTURE_OK"),
        "remote fixture validation returned no completion marker"
    );
    fs::write(
        artifacts.join("remote-final-file-diff.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schemaVersion": 1,
            "workspace": fixture.path,
            "receipt": rendered,
            "rejectedPathPresent": false,
        }))?,
    )?;
    Ok(())
}

fn cleanup_remote_fixture(
    ssh: &SshAcceptance,
    fixture: &RemoteFixture,
    artifacts: &Path,
) -> Result<()> {
    validate_remote_fixture_path(&fixture.path)?;
    let path = shell_quote(&fixture.path);
    let script = format!(
        r#"set -eu
fixture={path}
if test -e "$fixture"; then
  test "$(git -C "$fixture" worktree list --porcelain | grep -c '^worktree ')" -eq 1
  rm -rf -- "$fixture"
fi
test ! -e "$fixture"
printf '%s\n' 'REMOTE_SUBAGENTS_FIXTURE_CLEANED'
"#
    );
    let output = run_ssh_script(ssh, &script)?;
    fs::write(
        artifacts.join("remote-fixture-cleanup.stdout.log"),
        &output.stdout,
    )?;
    fs::write(
        artifacts.join("remote-fixture-cleanup.stderr.log"),
        &output.stderr,
    )?;
    ensure_ssh_success(ssh, &output, "cleanup remote subagents fixture")?;
    ensure!(
        String::from_utf8(output.stdout)?.contains("REMOTE_SUBAGENTS_FIXTURE_CLEANED"),
        "remote fixture cleanup returned no completion marker"
    );
    Ok(())
}

fn run_ssh_script(ssh: &SshAcceptance, script: &str) -> Result<std::process::Output> {
    let askpass_root = tempfile::Builder::new()
        .prefix("pure-subagents-askpass-")
        .tempdir()
        .context("failed to create SSH acceptance Askpass directory")?;
    let askpass = askpass_root.path().join("askpass.sh");
    fs::write(
        &askpass,
        "#!/bin/sh\nprintf '%s\\n' \"$PURE_SUBAGENTS_SSH_PASSWORD\"\n",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&askpass, fs::Permissions::from_mode(0o700))?;
    }
    let target = format!("{}@{}", ssh.username, ssh.host);
    let port = ssh.port.to_string();
    let mut command = Command::new("ssh");
    command
        .args([
            "-T",
            "-x",
            "-o",
            "BatchMode=no",
            "-o",
            "NumberOfPasswordPrompts=1",
            "-o",
            "PreferredAuthentications=password,keyboard-interactive",
            "-p",
            &port,
            &target,
            "sh",
            "-s",
        ])
        .env("SSH_ASKPASS", &askpass)
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env("DISPLAY", "pure-studio")
        .env(SSH_PASSWORD_ENV, &ssh.password)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().context("failed to start system OpenSSH")?;
    child
        .stdin
        .take()
        .context("system OpenSSH has no stdin")?
        .write_all(script.as_bytes())?;
    child
        .wait_with_output()
        .context("failed to wait for system OpenSSH")
}

fn ensure_ssh_success(
    ssh: &SshAcceptance,
    output: &std::process::Output,
    operation: &str,
) -> Result<()> {
    ensure!(
        output.status.success(),
        "{operation} failed: {}",
        redact_secret(&String::from_utf8_lossy(&output.stderr), &ssh.password)
    );
    Ok(())
}

fn redact_secret(value: &str, secret: &str) -> String {
    value.replace(secret, "[REDACTED]")
}

fn validate_remote_fixture_path(path: &str) -> Result<String> {
    ensure!(path.starts_with('/'), "remote fixture path is not absolute");
    ensure!(
        path.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/._-".contains(&byte)),
        "remote fixture path contains unsafe characters"
    );
    ensure!(
        !path.split('/').any(|component| component == ".."),
        "remote fixture path escapes its parent"
    );
    let leaf = path
        .rsplit('/')
        .next()
        .filter(|leaf| leaf.starts_with("pure-subagents-live."))
        .context("remote fixture path is not Pure-owned")?;
    Ok(leaf.to_string())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn ensure_secret_absent<'a>(
    roots: impl IntoIterator<Item = &'a Path>,
    secret: &[u8],
) -> Result<()> {
    ensure!(
        !secret.is_empty(),
        "SSH acceptance secret must not be empty"
    );
    for root in roots {
        scan_secret(root, secret)?;
    }
    Ok(())
}

fn verify_ssh_credential_storage(
    ssh: &SshAcceptance,
    studio_home: &Path,
    artifact_dir: &Path,
) -> Result<()> {
    let collision_with = [
        ("username", ssh.username.as_str()),
        ("host", ssh.host.as_str()),
        ("serverName", SSH_ACCEPTANCE_NAME),
    ]
    .into_iter()
    .filter_map(|(field, value)| (ssh.password == value).then_some(field))
    .collect::<Vec<_>>();
    let raw_byte_scan = collision_with.is_empty();
    if raw_byte_scan {
        ensure_secret_absent([artifact_dir, studio_home], ssh.password.as_bytes())?;
    }

    let database_path = studio_home.join("studio").join("studio.sqlite");
    let database_url = sqlite_read_only_url(&database_path);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to create SQLite credential audit runtime")?;
    let (columns, username, auth_json) = runtime.block_on(async {
        let database = Database::connect(&database_url)
            .await
            .context("failed to open isolated Studio database for credential audit")?;
        let columns = database
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                "PRAGMA table_info(ssh_servers)".to_string(),
            ))
            .await?
            .into_iter()
            .map(|row| row.try_get::<String>("", "name"))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let rows = database
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT username, auth_json FROM ssh_servers ORDER BY id".to_string(),
            ))
            .await?;
        ensure!(
            rows.len() == 1,
            "expected exactly one isolated SSH server row"
        );
        let username = rows[0].try_get::<String>("", "username")?;
        let auth_json = rows[0].try_get::<String>("", "auth_json")?;
        database.close().await?;
        Ok::<_, anyhow::Error>((columns, username, auth_json))
    })?;
    let expected_columns = [
        "id",
        "name",
        "host",
        "port",
        "username",
        "auth_json",
        "created_at",
        "updated_at",
    ];
    ensure!(
        columns.iter().map(String::as_str).eq(expected_columns),
        "ssh_servers contains an unexpected persistence column: {columns:?}"
    );
    ensure!(
        username == ssh.username,
        "isolated SSH server username does not match the visible GUI input"
    );
    let auth: Value = serde_json::from_str(&auth_json)?;
    ensure!(
        auth == serde_json::json!({"kind": "password"}),
        "ssh_servers auth_json contains data beyond the password authentication kind"
    );
    fs::write(
        artifact_dir.join("ssh-credential-storage.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schemaVersion": 1,
            "passed": true,
            "rawByteScan": raw_byte_scan,
            "rawByteScanSkippedBecauseSecretMatches": collision_with,
            "typedColumns": columns,
            "persistedAuth": auth,
        }))?,
    )?;
    Ok(())
}

fn sqlite_read_only_url(path: &Path) -> String {
    let path = path.to_string_lossy();
    let path = path
        .strip_prefix(r"\\?\UNC\")
        .map(|path| format!("//{path}"))
        .or_else(|| path.strip_prefix(r"\\?\").map(ToOwned::to_owned))
        .unwrap_or_else(|| path.into_owned())
        .replace('\\', "/");
    format!("sqlite://{path}?mode=ro")
}

fn scan_secret(path: &Path, secret: &[u8]) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    ensure!(
        !contains_bytes(path.as_os_str().to_string_lossy().as_bytes(), secret),
        "SSH acceptance secret leaked into path {}",
        path.display()
    );
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            scan_secret(&entry?.path(), secret)?;
        }
    } else if metadata.is_file() {
        ensure!(
            !contains_bytes(&fs::read(path)?, secret),
            "SSH acceptance secret leaked into {}",
            path.display()
        );
    }
    Ok(())
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|candidate| candidate == needle)
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
    let worktree_history = run_git(
        fixture,
        &["log", "--format=%H", "--", "worktree_result.txt"],
    )?;
    ensure!(
        worktree_history.lines().any(|line| !line.trim().is_empty()),
        "worktree_result.txt has no integrated Git history"
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

fn configure_live_acceptance(path: &Path) -> Result<()> {
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
    let runtime = config
        .entry("runtime")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .context("isolated live config runtime must be a table")?;
    runtime.insert(
        "permission_mode".to_string(),
        toml::Value::String("full-access".to_string()),
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

fn validate_wire(wire_dir: &Path, artifacts: &Path, ssh: bool) -> Result<()> {
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
        let mut capture_calls = Vec::new();
        collect_calls(body, &mut capture_calls);
        let actor = classify_capture(&capture_calls)?;
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
    let expected_rejection_count = ensure_no_unexpected_tool_failures(&calls, &outputs)?;
    ensure_spawn_calls(&calls)?;
    ensure_workspace_receipts(&calls, &outputs)?;
    ensure_profile_messages(&calls, &outputs)?;
    ensure_reviewer_history(&capture_receipts)?;
    ensure_root_history(&capture_receipts, &calls, &outputs)?;
    ensure_submissions(&calls, &outputs)?;
    ensure_finding_re_review(&calls, &outputs)?;
    ensure_orchestration_order(&calls, &outputs)?;
    if ssh {
        ensure_ssh_exec_contract(&capture_receipts)?;
    }
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
                    .is_some_and(|command| is_git_cherry_pick_command(command, None))
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
    let mut output_markers = vec![
        "directoryRejection",
        "directoryWorkspaceReceipt",
        "worktreeWorkspaceReceipt",
        "explicitCherryPick",
        "explicitCleanup",
        "childDurableDelivery",
    ];
    if ssh {
        output_markers.push("sshRelativeExec");
    }
    fs::write(
        artifacts.join("subagents-wire-receipt.json"),
        serde_json::to_vec_pretty(&WireReceipt {
            schema_version: 2,
            capture_count: captures.len(),
            attempt_count: calls.len(),
            expected_rejection_count,
            unexpected_failure_count: 0,
            calls,
            captures: capture_receipts,
            output_markers,
        })?,
    )?;
    Ok(())
}

fn shell_segments(command: &str) -> impl Iterator<Item = &str> {
    command
        .split([';', '\n', '|'])
        .flat_map(|segment| segment.split("&&"))
}

fn shell_token(token: &str) -> &str {
    token.trim_matches(['\'', '"'])
}

fn is_git_cherry_pick_command(command: &str, expected_commit: Option<&str>) -> bool {
    shell_segments(command).any(|segment| {
        let tokens = segment
            .split_ascii_whitespace()
            .map(shell_token)
            .collect::<Vec<_>>();
        let Some(git) = tokens.iter().position(|token| *token == "git") else {
            return false;
        };
        if tokens[..git]
            .iter()
            .any(|token| !token.contains('=') && *token != "env")
        {
            return false;
        }
        let Some(cherry_pick) = tokens[git + 1..]
            .iter()
            .position(|token| *token == "cherry-pick")
            .map(|index| index + git + 1)
        else {
            return false;
        };
        match expected_commit {
            Some(commit) => tokens[cherry_pick + 1..].contains(&commit),
            None => tokens[cherry_pick + 1..]
                .iter()
                .any(|token| !token.starts_with('-')),
        }
    })
}

fn is_cargo_test_command(command: &str) -> bool {
    shell_segments(command).any(|segment| {
        let tokens = segment
            .split_ascii_whitespace()
            .map(shell_token)
            .collect::<Vec<_>>();
        tokens.first() == Some(&"cargo") && tokens[1..].contains(&"test")
    })
}

fn ensure_ssh_exec_contract(captures: &[CaptureReceipt]) -> Result<()> {
    let mut first_seen = std::collections::HashSet::new();
    let mut child_exec_first_appearances = Vec::new();
    for (capture_index, capture) in captures.iter().enumerate() {
        for call in &capture.calls {
            if call.name != "exec" {
                continue;
            }
            let cwd = call.arguments.get("cwd").and_then(Value::as_str);
            if let Some(cwd) = cwd {
                ensure!(
                    !cwd.starts_with('/')
                        && !cwd.starts_with('\\')
                        && !cwd.split(['/', '\\']).any(|part| part == "..")
                        && !cwd.contains(':'),
                    "SSH exec.cwd is not workspace-relative: {cwd}"
                );
            }
            let Some(call_id) = call.call_id.as_deref() else {
                continue;
            };
            if capture.actor != "root" && first_seen.insert(call_id.to_string()) {
                child_exec_first_appearances.push(capture_index);
            }
        }
    }
    ensure!(
        child_exec_first_appearances.len() >= 2,
        "SSH child acceptance contains fewer than two distinct exec calls"
    );
    ensure!(
        child_exec_first_appearances
            .windows(2)
            .any(|pair| pair[0] != pair[1]),
        "SSH child exec calls were not observed across separate inference captures"
    );
    Ok(())
}

fn classify_capture(calls: &[WireCall]) -> Result<String> {
    let is_root = calls
        .iter()
        .any(|call| matches!(call.name.as_str(), "list_agent_profiles" | "submit_plan"));
    let is_reviewer = calls.iter().any(|call| {
        call.name == "report_progress"
            && ["summary", "nextStep", "detail"]
                .into_iter()
                .filter_map(|field| call.arguments.get(field).and_then(Value::as_str))
                .any(|text| {
                    text.contains("REVIEWER_FINDING")
                        || text.contains("REVIEWER_READ_ONLY_APPROVED")
                })
    });
    ensure!(
        !(is_root && is_reviewer),
        "wire capture contains both root orchestration and reviewer verdict calls"
    );
    Ok(if is_root {
        "root"
    } else if is_reviewer {
        "reviewer"
    } else {
        "child"
    }
    .to_string())
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
            ensure!(
                !message.trim().is_empty(),
                "{profile} spawn has an empty task message"
            );
        }
    }
    let explorers = spawns
        .iter()
        .filter(|call| call.arguments.get("profileId").and_then(Value::as_str) == Some("explorer"))
        .collect::<Vec<_>>();
    ensure!(
        explorers.len() >= 2,
        "wire captures must contain at least two explorer spawn messages"
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
            .is_some_and(|command| is_git_cherry_pick_command(
                command, None
            ))),
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
                            .is_some_and(|command| is_git_cherry_pick_command(command, None)))
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
                    .is_some_and(is_cargo_test_command)
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
                    .is_some_and(is_cargo_test_command)
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

fn ensure_no_unexpected_tool_failures(calls: &[WireCall], outputs: &[WireOutput]) -> Result<usize> {
    let mut expected_rejections = 0;
    for output in outputs.iter().filter(|output| tool_output_failed(output)) {
        let call_id = output
            .call_id
            .as_deref()
            .context("tool failure has no call_id")?;
        let call = calls
            .iter()
            .find(|call| call.call_id.as_deref() == Some(call_id))
            .with_context(|| format!("tool failure {call_id} has no bound call"))?;
        let is_expected_directory_rejection = call.name == "write_file"
            && call.arguments.get("path").and_then(Value::as_str) == Some("forbidden/denied.txt")
            && output
                .content
                .contains("outside the directory Agent writablePaths boundary");
        ensure!(
            is_expected_directory_rejection,
            "unexpected first-call tool failure {call_id} ({}): {}",
            call.name,
            output.content.trim()
        );
        expected_rejections += 1;
    }
    ensure!(
        expected_rejections > 0,
        "wire captures contain no expected forbidden directory rejection"
    );
    Ok(expected_rejections)
}

fn tool_output_failed(output: &WireOutput) -> bool {
    let content = output.content.trim();
    if content.starts_with("Tool execution error:") {
        return true;
    }
    let Ok(value) = serde_json::from_str::<Value>(content) else {
        return false;
    };
    value.get("accepted").and_then(Value::as_bool) == Some(false)
        || value
            .pointer("/state/data/result/kind")
            .and_then(Value::as_str)
            == Some("failed")
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
        let content = output.content.trim();
        let receipt: Value = match serde_json::from_str(content) {
            Ok(receipt) => receipt,
            Err(error) if looks_like_json_output(content) => {
                bail!(
                    "spawn {id} output looks like JSON but is malformed (bound_receipts): {error}"
                )
            }
            Err(_) if content.starts_with("Tool execution error:") => {
                bail!("spawn {id} failed before a canonical receipt: {content}")
            }
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
        .position(|call| call.name == "submit_plan")
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
        explorers.len() >= 2,
        "wire captures contain fewer than two successful explorer spawns"
    );
    ensure!(
        profiles < explorers[0].0,
        "root Profile query must precede explorer spawns"
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
        executors[0].0 < implementation_wait && worktrees[0].0 < implementation_wait,
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
                    .is_some_and(|command| is_git_cherry_pick_command(command, None))
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
    let implementation_spawns = calls
        .iter()
        .enumerate()
        .filter(|(index, call)| {
            *index > finding_index
                && *index < final_reviewer_spawn
                && call.name == "spawn_agent"
                && matches!(
                    call.arguments.get("profileId").and_then(Value::as_str),
                    Some("executor") | Some("worktree_executor")
                )
        })
        .collect::<Vec<_>>();
    ensure!(
        !implementation_spawns.is_empty(),
        "REVIEWER_FINDING requires a new implementation spawn"
    );
    let receipts = bound_receipts(calls, outputs)?;
    for (spawn_index, spawn) in implementation_spawns.into_iter().filter(|(_, call)| {
        call.arguments.get("profileId").and_then(Value::as_str) == Some("worktree_executor")
    }) {
        let receipt = receipts
            .iter()
            .find(|(candidate, _)| candidate.call_id == spawn.call_id)
            .map(|(_, receipt)| receipt)
            .context("rework worktree spawn has no canonical receipt")?;
        let agent = receipt
            .get("agentId")
            .and_then(Value::as_str)
            .context("rework worktree receipt has no agentId")?;
        let (read_index, read) = calls
            .iter()
            .enumerate()
            .find(|(index, call)| {
                *index > spawn_index
                    && *index < final_reviewer_spawn
                    && call.name == "read_agent_submissions"
                    && call.arguments.get("target").and_then(Value::as_str) == Some(agent)
            })
            .with_context(|| {
                format!("rework worktree agent {agent} has no durable delivery read")
            })?;
        let read_id = read
            .call_id
            .as_ref()
            .context("rework worktree durable delivery read has no call_id")?;
        let page = outputs
            .iter()
            .find(|output| output.call_id.as_ref() == Some(read_id))
            .context("rework worktree durable delivery read has no bound output")?;
        let commit = worktree_submission_commit(&parse_submission_page(&page.content)?.items)?;
        let cherry_pick = calls
            .iter()
            .enumerate()
            .find(|(index, call)| {
                *index > read_index
                    && *index < final_reviewer_spawn
                    && call.name == "exec"
                    && call
                        .arguments
                        .get("command")
                        .and_then(Value::as_str)
                        .is_some_and(|command| is_git_cherry_pick_command(command, Some(&commit)))
            })
            .map(|(index, _)| index)
            .with_context(|| format!("rework worktree agent {agent} was not integrated"))?;
        ensure!(
            calls.iter().enumerate().any(|(index, call)| {
                index > cherry_pick
                    && index < final_reviewer_spawn
                    && call.name == "close_agent"
                    && call.arguments.get("target").and_then(Value::as_str) == Some(agent)
                    && call
                        .arguments
                        .get("workspaceDisposition")
                        .and_then(Value::as_str)
                        == Some("cleanup")
            }),
            "rework worktree agent {agent} was not cleaned after integration"
        );
    }
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
        "wire captures contain fewer than two explorer receipts"
    );
    let first_impl = required
        .iter()
        .filter(|(p, _, _)| *p != "explorer")
        .map(|(_, _, i)| *i)
        .min()
        .context("no implementation spawn")?;
    for (profile, agent, spawn_index) in required {
        let (read_index, read) = calls
            .iter()
            .enumerate()
            .find(|(i, call)| {
                *i > spawn_index
                    && call.name == "read_agent_submissions"
                    && call.arguments.get("target").and_then(Value::as_str) == Some(agent.as_str())
            })
            .with_context(|| format!("{profile} agent {agent} has no targeted submissions read"))?;
        calls
            .iter()
            .enumerate()
            .find(|(index, call)| {
                *index > spawn_index
                    && *index < read_index
                    && call.name == "wait_agents"
                    && wait_has_terminal_evidence(call, outputs, &agent).is_ok()
            })
            .with_context(|| {
                format!(
                    "{profile} agent {agent} has no receipt-bound terminal wait before submissions read"
                )
            })?;
        let read_id = read
            .call_id
            .as_ref()
            .context("read_agent_submissions has no call_id")?;
        let output = outputs
            .iter()
            .find(|output| output.call_id.as_ref() == Some(read_id))
            .context("submissions read has no bound output")?;
        let result = parse_submission_page(&output.content)?;
        ensure!(
            result.total > 0 && !result.items.is_empty(),
            "{profile} agent {agent} returned an empty durable submission page"
        );
        let rendered = serde_json::to_string(&result.items)?;
        ensure!(
            rendered.contains("CHILD_DELIVERY_READY"),
            "{profile} agent {agent} submission lacks CHILD_DELIVERY_READY"
        );
        if profile == "worktree_executor" {
            ensure!(
                rendered.contains("WORKTREE_COMMIT_READY"),
                "worktree_executor agent {agent} submission lacks WORKTREE_COMMIT_READY"
            );
        }
        if profile == "explorer" {
            ensure!(
                read_index < first_impl,
                "explorer submissions read occurred after implementation spawn"
            );
        } else {
            let reviewer_spawn = calls
                .iter()
                .enumerate()
                .find(|(index, call)| {
                    *index > spawn_index
                        && call.name == "spawn_agent"
                        && call.arguments.get("profileId").and_then(Value::as_str)
                            == Some("reviewer")
                })
                .map(|(index, _)| index)
                .with_context(|| {
                    format!("{profile} agent {agent} is not followed by a reviewer spawn")
                })?;
            ensure!(
                read_index < reviewer_spawn,
                "{profile} agent {agent} submission was not read before review"
            );
            if profile == "worktree_executor" {
                let commit = worktree_submission_commit(&result.items)?;
                let cherry_pick = calls
                    .iter()
                    .enumerate()
                    .find(|(index, call)| {
                        *index > read_index
                            && *index < reviewer_spawn
                            && call.name == "exec"
                            && call
                                .arguments
                                .get("command")
                                .and_then(Value::as_str)
                                .is_some_and(|command| {
                                    is_git_cherry_pick_command(command, Some(&commit))
                                })
                    })
                    .map(|(index, _)| index)
                    .with_context(|| {
                        format!("worktree_executor agent {agent} was not explicitly integrated")
                    })?;
                ensure!(
                    calls.iter().enumerate().any(|(index, call)| {
                        index > cherry_pick
                            && index < reviewer_spawn
                            && call.name == "close_agent"
                            && call.arguments.get("target").and_then(Value::as_str)
                                == Some(agent.as_str())
                            && call
                                .arguments
                                .get("workspaceDisposition")
                                .and_then(Value::as_str)
                                == Some("cleanup")
                    }),
                    "worktree_executor agent {agent} was not cleaned after integration"
                );
            }
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

fn worktree_submission_commit(items: &[Value]) -> Result<String> {
    let mut commits = std::collections::HashSet::new();
    for item in items {
        for text in ["summary", "nextStep", "detail"]
            .into_iter()
            .filter_map(|field| item.get(field).and_then(Value::as_str))
            .filter(|text| text.contains("WORKTREE_COMMIT_READY"))
        {
            for suffix in text.split("commit=").skip(1) {
                let commit = suffix
                    .chars()
                    .take_while(char::is_ascii_hexdigit)
                    .collect::<String>();
                if commit.len() == 40 {
                    commits.insert(commit.to_ascii_lowercase());
                }
            }
        }
    }
    ensure!(
        commits.len() == 1,
        "worktree durable delivery must identify exactly one `commit=<40-character hexadecimal commit>`"
    );
    commits
        .into_iter()
        .next()
        .context("validated worktree durable delivery commit is missing")
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

    const FIRST_WORKTREE_COMMIT: &str = "1111111111111111111111111111111111111111";
    const SECOND_WORKTREE_COMMIT: &str = "2222222222222222222222222222222222222222";
    const REWORK_WORKTREE_COMMIT: &str = "3333333333333333333333333333333333333333";

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
                "test: integrate worktree result",
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
                    "test: integrate worktree result",
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
        let allowed_path = std::env::temp_dir().join("fixture").join("allowed");
        let receipts = [
            ("e", "explorer", serde_json::json!({"mode":"unrestricted"})),
            ("x", "executor", serde_json::json!({"mode":"directory","writablePaths":[allowed_path]})),
            ("w", "worktree_executor", serde_json::json!({"mode":"worktree","worktree":{"branch":"pure-agent-child","baseCommit":"0123456789abcdef0123456789abcdef01234567"}})),
            ("r", "reviewer", serde_json::json!({"mode":"unrestricted"})),
        ].into_iter().map(|(id, profile, workspace)| WireOutput { call_id: Some(id.into()), content: serde_json::json!({"profileId":profile,"agentId":id,"workspace":workspace}).to_string() }).collect::<Vec<_>>();
        ensure_workspace_receipts(&calls, &receipts).unwrap();
    }

    #[test]
    fn bound_receipts_rejects_failed_spawn_before_retry() {
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

        let error = bound_receipts(&calls, &outputs).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("failed before a canonical receipt")
        );
    }

    #[test]
    fn tool_failures_allow_only_the_expected_directory_rejection() {
        let calls = ["denied-first", "denied-after-review"]
            .into_iter()
            .map(|call_id| {
                orchestration_call(
                    call_id,
                    "write_file",
                    serde_json::json!({"path":"forbidden/denied.txt"}),
                )
            })
            .collect::<Vec<_>>();
        let outputs = ["denied-first", "denied-after-review"]
            .into_iter()
            .map(|call_id| WireOutput {
                call_id: Some(call_id.into()),
                content:
                    "Tool execution error: path is outside the directory Agent writablePaths boundary"
                        .into(),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            ensure_no_unexpected_tool_failures(&calls, &outputs).unwrap(),
            2
        );

        for (name, path, error_text) in [
            (
                "spawn_agent",
                "",
                "Tool execution error: invalid fork_turns",
            ),
            (
                "write_file",
                "allowed/result.txt",
                "Tool execution error: permission denied",
            ),
        ] {
            let calls = vec![orchestration_call(
                "failed",
                name,
                serde_json::json!({"path":path}),
            )];
            let outputs = vec![WireOutput {
                call_id: Some("failed".into()),
                content: error_text.into(),
            }];
            let error = ensure_no_unexpected_tool_failures(&calls, &outputs).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("unexpected first-call tool failure")
            );
        }

        for (name, output) in [
            (
                "workflow_state",
                serde_json::json!({"accepted":false,"code":"invalidDefinition"}).to_string(),
            ),
            (
                "exec",
                serde_json::json!({
                    "state": {"kind":"final","data":{"result":{"kind":"failed"}}}
                })
                .to_string(),
            ),
        ] {
            let calls = vec![orchestration_call("failed", name, serde_json::json!({}))];
            let outputs = vec![WireOutput {
                call_id: Some("failed".into()),
                content: output,
            }];
            assert!(ensure_no_unexpected_tool_failures(&calls, &outputs).is_err());
        }
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
    fn isolated_live_config_uses_full_access_and_disables_live_profiles() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config.toml");
        fs::write(&config, "disabled_system_agents = []\n").unwrap();
        configure_live_acceptance(&config).unwrap();
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
        assert_eq!(
            table
                .get("runtime")
                .and_then(toml::Value::as_table)
                .and_then(|runtime| runtime.get("permission_mode"))
                .and_then(toml::Value::as_str),
            Some("full-access")
        );
    }

    #[test]
    fn capture_classification_uses_tool_behavior_instead_of_prompt_prose() {
        let root = vec![orchestration_call(
            "plan",
            "submit_plan",
            serde_json::json!({"plan":"Inspect, implement, integrate, review."}),
        )];
        assert_eq!(classify_capture(&root).unwrap(), "root");
        let reviewer = vec![orchestration_call(
            "progress",
            "report_progress",
            serde_json::json!({"detail":"REVIEWER_READ_ONLY_APPROVED"}),
        )];
        assert_eq!(classify_capture(&reviewer).unwrap(), "reviewer");
        let mut ambiguous = root;
        ambiguous.extend(reviewer);
        assert!(classify_capture(&ambiguous).is_err());
        assert_eq!(classify_capture(&[]).unwrap(), "child");
    }

    #[test]
    fn command_checks_require_real_git_and_cargo_invocations() {
        assert!(is_git_cherry_pick_command(
            "cd fixture && git cherry-pick 1111111111111111111111111111111111111111",
            Some(FIRST_WORKTREE_COMMIT)
        ));
        assert!(!is_git_cherry_pick_command(
            "echo git cherry-pick 1111111111111111111111111111111111111111",
            Some(FIRST_WORKTREE_COMMIT)
        ));
        assert!(is_cargo_test_command("cargo test --workspace"));
        assert!(!is_cargo_test_command("echo cargo test --workspace"));
    }

    #[test]
    fn worktree_commit_parser_ignores_other_reported_revisions() {
        let item = submission_item(&[(
            "detail",
            "WORKTREE_COMMIT_READY commit=1111111111111111111111111111111111111111 base=2222222222222222222222222222222222222222",
        )]);
        assert_eq!(
            worktree_submission_commit(&[item]).unwrap(),
            FIRST_WORKTREE_COMMIT
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
    fn submissions_require_implementation_reads_before_cherry_pick() {
        let (mut calls, outputs) = valid_nonreviewer_submission_flow();
        ensure_submissions(&calls, &outputs).unwrap();
        let pick = calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("pick"))
            .unwrap();
        calls.swap(pick, pick - 1);
        assert!(ensure_submissions(&calls, &outputs).is_err());
    }

    #[test]
    fn empty_non_reviewer_pages_are_rejected_even_with_session_diagnostics() {
        let (mut calls, mut outputs) = valid_nonreviewer_submission_flow();
        outputs
            .iter_mut()
            .find(|output| output.call_id.as_deref() == Some("read-0"))
            .unwrap()
            .content = submission_page(Vec::new(), 0);
        let read = calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("read-0"))
            .unwrap();
        calls.insert(
            read + 1,
            orchestration_call(
                "session-0",
                "read_agent_session",
                serde_json::json!({"target":"agent-0"}),
            ),
        );
        outputs.push(WireOutput {
            call_id: Some("session-0".into()),
            content: serde_json::json!({
                "messages": [{"role":"assistant","text":"terminal child result"}]
            })
            .to_string(),
        });
        let error = ensure_submissions(&calls, &outputs).unwrap_err();
        assert!(error.to_string().contains("empty durable submission page"));
    }

    #[test]
    fn submissions_require_receipt_bound_terminal_wait_before_read() {
        let (calls, mut outputs) = valid_nonreviewer_submission_flow();
        outputs
            .iter_mut()
            .find(|output| output.call_id.as_deref() == Some("wait-0"))
            .unwrap()
            .content = serde_json::json!({
            "messages": [{"agentId":"agent-0"}],
            "reason":"progress"
        })
        .to_string();
        assert!(ensure_submissions(&calls, &outputs).is_err());

        let (mut calls, outputs) = valid_nonreviewer_submission_flow();
        let wait = calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("wait-0"))
            .unwrap();
        let wait_call = calls.remove(wait);
        let read = calls
            .iter()
            .position(|call| call.call_id.as_deref() == Some("read-0"))
            .unwrap();
        calls.insert(read + 1, wait_call);
        assert!(ensure_submissions(&calls, &outputs).is_err());
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
    fn profile_messages_require_frozen_context_and_nonempty_tasks() {
        let calls = profile_message_calls();
        let outputs = profile_message_outputs(&calls);
        ensure_profile_messages(&calls, &outputs).unwrap();

        let mut invalid = calls.clone();
        invalid[0].arguments["forkTurns"] = Value::String("all".into());
        assert!(ensure_profile_messages(&invalid, &outputs).is_err());

        let mut invalid = calls.clone();
        invalid[2].arguments["message"] = Value::String(String::new());
        assert!(ensure_profile_messages(&invalid, &outputs).is_err());
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
            let message = match id {
                "explorer-1" => "Inspect fixture source and report durable evidence.",
                "explorer-2" => "Inspect workspace Git metadata and report durable evidence.",
                "executor-1" => "Implement the directory-scoped fixture task.",
                "worktree-1" => "Implement and commit the isolated worktree task.",
                "reviewer-1" => "Review the integrated fixture without mutations.",
                _ => unreachable!(),
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
            orchestration_call("between-explorers", "git_status", serde_json::json!({})),
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
            orchestration_call("confirm", "submit_plan", serde_json::json!({})),
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

    fn valid_nonreviewer_submission_flow() -> (Vec<WireCall>, Vec<WireOutput>) {
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
            outputs.push(spawn_output(&spawn, profile, &agent));
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
            let detail = if profile == "worktree_executor" {
                "CHILD_DELIVERY_READY WORKTREE_COMMIT_READY commit=1111111111111111111111111111111111111111"
            } else {
                "CHILD_DELIVERY_READY"
            };
            outputs.push(WireOutput {
                call_id: Some(read),
                content: submission_page(
                    vec![serde_json::json!({
                        "stage":"readyForCompletion",
                        "summary":"CHILD_DELIVERY_READY",
                        "nextStep":"root reads receipt",
                        "detail":detail,
                        "createdAt":1
                    })],
                    1,
                ),
            });
        }
        calls.push(orchestration_call(
            "pick",
            "exec",
            serde_json::json!({"command":format!("git cherry-pick {FIRST_WORKTREE_COMMIT}")}),
        ));
        calls.push(orchestration_call(
            "cleanup",
            "close_agent",
            serde_json::json!({
                "target":"agent-3",
                "workspaceDisposition":"cleanup"
            }),
        ));
        calls.push(orchestration_call(
            "review",
            "spawn_agent",
            serde_json::json!({"profileId":"reviewer"}),
        ));
        outputs.push(spawn_output("review", "reviewer", "reviewer-a"));
        for (profile, suffix) in [("executor", "4"), ("worktree_executor", "5")] {
            let spawn = format!("spawn-{suffix}");
            let agent = format!("agent-{suffix}");
            calls.push(orchestration_call(
                &spawn,
                "spawn_agent",
                serde_json::json!({"profileId":profile}),
            ));
            outputs.push(spawn_output(&spawn, profile, &agent));
            let wait = format!("wait-{suffix}");
            calls.push(orchestration_call(
                &wait,
                "wait_agents",
                serde_json::json!({"targets":[agent]}),
            ));
            outputs.push(terminal_wait_output(&wait, &agent));
            let read = format!("read-{suffix}");
            calls.push(orchestration_call(
                &read,
                "read_agent_submissions",
                serde_json::json!({"target":agent}),
            ));
            let detail = if profile == "worktree_executor" {
                "CHILD_DELIVERY_READY WORKTREE_COMMIT_READY commit=2222222222222222222222222222222222222222"
            } else {
                "CHILD_DELIVERY_READY"
            };
            outputs.push(WireOutput {
                call_id: Some(read),
                content: submission_page(vec![submission_item(&[("detail", detail)])], 1),
            });
        }
        calls.push(orchestration_call(
            "pick-2",
            "exec",
            serde_json::json!({"command":format!("git cherry-pick {SECOND_WORKTREE_COMMIT}")}),
        ));
        calls.push(orchestration_call(
            "cleanup-2",
            "close_agent",
            serde_json::json!({
                "target":"agent-5",
                "workspaceDisposition":"cleanup"
            }),
        ));
        calls.push(orchestration_call(
            "review-2",
            "spawn_agent",
            serde_json::json!({"profileId":"reviewer"}),
        ));
        outputs.push(spawn_output("review-2", "reviewer", "reviewer-b"));
        (calls, outputs)
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
                "between-implementations",
                "git_status",
                serde_json::json!({}),
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
                serde_json::json!({"command":format!("git cherry-pick {REWORK_WORKTREE_COMMIT}")}),
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
    fn orchestration_order_accepts_non_adjacent_spawns_before_waits() {
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
    fn orchestration_order_rejects_confirmation_before_all_explorer_reads() {
        let (mut calls, outputs) = valid_orchestration_calls();
        swap_calls(&mut calls, "er2", "confirm");
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
                serde_json::json!({"profileId":"worktree_executor"}),
            ),
            orchestration_call(
                "xr1",
                "read_agent_submissions",
                serde_json::json!({"target":"worktree-a"}),
            ),
            orchestration_call(
                "i1",
                "exec",
                serde_json::json!({"command":format!("git cherry-pick {REWORK_WORKTREE_COMMIT}")}),
            ),
            orchestration_call(
                "c1",
                "close_agent",
                serde_json::json!({
                    "target":"worktree-a",
                    "workspaceDisposition":"cleanup"
                }),
            ),
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
            spawn_output("x1", "worktree_executor", "worktree-a"),
            reviewer_read_output(
                Some("xr1"),
                vec![submission_item(&[(
                    "detail",
                    "CHILD_DELIVERY_READY WORKTREE_COMMIT_READY commit=3333333333333333333333333333333333333333",
                )])],
                1,
            ),
            reviewer_receipt("r2", "reviewer-b"),
            approval_output(Some("rr2")),
        ];
        (calls, outputs)
    }

    #[test]
    fn reviewer_approval_requires_receipt_bound_target_and_output() {
        let invalid = [
            (
                single_reviewer_calls("another-agent"),
                single_reviewer_outputs(approval_output(Some("rr1"))),
            ),
            (
                single_reviewer_calls("reviewer-a"),
                single_reviewer_outputs(approval_output(Some("another-read"))),
            ),
            (
                single_reviewer_calls("reviewer-a"),
                single_reviewer_outputs(approval_output(None)),
            ),
        ];
        for (calls, outputs) in invalid {
            assert!(ensure_finding_re_review(&calls, &outputs).is_err());
        }
    }

    #[test]
    fn reviewer_empty_submission_does_not_authorize() {
        let calls = single_reviewer_calls("reviewer-a");
        for output in [
            reviewer_read_output(Some("rr1"), vec![], 0),
            reviewer_read_output(
                Some("rr1"),
                vec![submission_item(&[(
                    "detail",
                    "REVIEWER_READ_ONLY_APPROVED",
                )])],
                0,
            ),
        ] {
            let outputs = single_reviewer_outputs(output);
            assert!(ensure_finding_re_review(&calls, &outputs).is_err());
        }
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
    fn finding_re_review_requires_worktree_integration_and_cleanup() {
        for missing in ["i1", "c1"] {
            let (mut calls, outputs) = valid_finding_re_review();
            calls.retain(|call| call.call_id.as_deref() != Some(missing));
            assert!(ensure_finding_re_review(&calls, &outputs).is_err());
        }
        let (mut calls, outputs) = valid_finding_re_review();
        calls
            .iter_mut()
            .find(|call| call.call_id.as_deref() == Some("i1"))
            .unwrap()
            .arguments["command"] =
            Value::String("git cherry-pick 4444444444444444444444444444444444444444".into());
        assert!(ensure_finding_re_review(&calls, &outputs).is_err());
    }

    #[test]
    fn finding_re_review_requires_different_final_reviewer_agent_id() {
        let (mut calls, mut outputs) = valid_finding_re_review();
        calls
            .iter_mut()
            .find(|call| call.call_id.as_deref() == Some("rr2"))
            .unwrap()
            .arguments["target"] = Value::String("reviewer-a".into());
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
