use crate::cli::{VerifySubagentsOptions, VerifyWorkflowOptions};
use crate::{paths, process};
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod artifact;
mod resident;
mod subagents;

const LIVE_TEST_NAME: &str = "installed_config_workflow_mode_delivers_rust_project";
const VERIFY_MARKER: &str = "PURE_WORKFLOW_GUI_VERIFY_OK";
const LIVE_CONFIG_SCHEMA_VERSION: i64 = 17;
const TOTAL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const STALL_TIMEOUT_SECONDS: u64 = 10 * 60;
const WORKFLOW_FIXTURE_USAGE_PATH: &str = ".agents/skills/workflow-fixture-rust/.usage.json";
const EXPECTED_DELIVERY_PATHS: &[&str] = &[
    "design/task-workflows.md",
    "src/normalize.rs",
    "src/validate.rs",
    "tests/normalize.rs",
    "tests/validate.rs",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkflowAcceptanceScope {
    Full,
    PlanOnly,
}

impl WorkflowAcceptanceScope {
    fn from_options(options: VerifyWorkflowOptions) -> Result<Self> {
        if options.plan_only {
            ensure!(
                options.gui && !options.headless,
                "verify-workflow --plan-only requires --gui and cannot run with --headless"
            );
            Ok(Self::PlanOnly)
        } else {
            Ok(Self::Full)
        }
    }

    fn driver_value(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::PlanOnly => "plan-only",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkflowFixtureSkillUsage {
    created_by: String,
    views: u64,
    uses: u64,
    patches: u64,
    created_at: i64,
    updated_at: i64,
    last_viewed_at: Option<i64>,
    pinned: bool,
}

pub(crate) fn run(options: VerifyWorkflowOptions) -> Result<()> {
    let deadline = Instant::now()
        + if options.headless {
            Duration::from_secs(45 * 60)
        } else {
            TOTAL_TIMEOUT
        };
    ensure!(
        options.live,
        "verify-workflow requires --live because it uses real credentials, incurs model fees, and never falls back to a scripted provider"
    );
    let scope = WorkflowAcceptanceScope::from_options(options)?;
    let workspace_root = paths::workspace_root()?;
    let surface = match (options.headless, options.gui) {
        (true, false) => "headless",
        (false, true) if scope == WorkflowAcceptanceScope::PlanOnly => "gui-plan",
        (false, true) => "gui",
        _ => bail!("verify-workflow requires exactly one of --headless or --gui"),
    };
    let artifact_dir = workspace_root
        .join("target")
        .join("workflow-live-artifacts")
        .join(format!("{surface}-{}-{}", std::process::id(), unix_nanos()));
    fs::create_dir_all(&artifact_dir)?;
    let prompt_name = match scope {
        WorkflowAcceptanceScope::Full => "prompt.md",
        WorkflowAcceptanceScope::PlanOnly => "plan-only-prompt.md",
    };
    let prompt = workspace_root
        .join("test-fixtures")
        .join("workflow-live")
        .join(prompt_name);
    let prompt_bytes = fs::read(&prompt)
        .with_context(|| format!("failed to read canonical prompt `{}`", prompt.display()))?;
    let prompt_hash = format!("{:x}", Sha256::digest(&prompt_bytes));
    fs::write(artifact_dir.join("fixture-prompt.md"), &prompt_bytes)?;
    fs::write(
        artifact_dir.join("fixture-prompt.sha256"),
        format!("{prompt_hash}\n"),
    )?;
    let simple_prompt = workspace_root
        .join("test-fixtures")
        .join("workflow-live")
        .join("simple-prompt.md");
    let simple_prompt_bytes = fs::read(&simple_prompt).with_context(|| {
        format!(
            "failed to read canonical simple-mode prompt `{}`",
            simple_prompt.display()
        )
    })?;
    let simple_prompt_hash = format!("{:x}", Sha256::digest(&simple_prompt_bytes));
    fs::write(
        artifact_dir.join("simple-fixture-prompt.md"),
        &simple_prompt_bytes,
    )?;
    fs::write(
        artifact_dir.join("simple-fixture-prompt.sha256"),
        format!("{simple_prompt_hash}\n"),
    )?;
    fs::write(
        artifact_dir.join("acceptance-surface.txt"),
        format!(
            "surface={surface}\nscope={}\nscriptedProvider=false\nlive=true\n",
            scope.driver_value()
        ),
    )?;
    println!("Workflow live artifacts: {}", artifact_dir.display());

    let wire_dir = artifact_dir.join("wire");
    fs::create_dir_all(&wire_dir)?;
    let acceptance = if options.headless {
        run_headless(&workspace_root, &artifact_dir, &wire_dir, deadline)
    } else {
        run_gui(
            &workspace_root,
            &artifact_dir,
            &wire_dir,
            &prompt,
            scope,
            deadline,
        )
    };
    if let Err(error) = &acceptance {
        fs::write(
            artifact_dir.join("acceptance-error.txt"),
            format!("{error:#}\n"),
        )?;
    }
    let manifest = match artifact::has_captures(&wire_dir) {
        Ok(false) if acceptance.is_err() => {
            fs::write(
                artifact_dir.join("wire-validation-skipped.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "schemaVersion": 1,
                    "reason": "acceptanceFailedBeforeProviderRequest",
                    "captureCount": 0,
                }))?,
            )?;
            Ok(())
        }
        Ok(_) => artifact::finalize(
            &artifact_dir,
            &wire_dir,
            surface,
            &prompt_hash,
            &simple_prompt_hash,
            scope,
        ),
        Err(error) => Err(error),
    };
    match (acceptance, manifest) {
        (Ok(()), Ok(())) => {
            fs::write(artifact_dir.join("result.txt"), "completed\n")?;
            println!(
                "Workflow live acceptance completed: {}",
                artifact_dir.display()
            );
            Ok(())
        }
        (Err(error), Ok(())) => Err(error.context(format!(
            "Workflow acceptance artifacts were preserved at {}",
            artifact_dir.display()
        ))),
        (Ok(()), Err(error)) => Err(error.context(format!(
            "Workflow wire acceptance failed; artifacts are at {}",
            artifact_dir.display()
        ))),
        (Err(error), Err(manifest_error)) => Err(error.context(format!(
            "wire manifest also failed: {manifest_error:#}; artifacts are at {}",
            artifact_dir.display()
        ))),
    }
}

pub(crate) fn run_subagents(options: VerifySubagentsOptions) -> Result<()> {
    subagents::run(options)
}

fn run_headless(
    workspace_root: &Path,
    artifact_dir: &Path,
    wire_dir: &Path,
    deadline: Instant,
) -> Result<()> {
    let mut command = Command::new("cargo");
    command
        .args([
            "test",
            "-p",
            "pl-studio-runtime",
            "--features",
            "live-tests",
            "--test",
            "workflow_live",
            LIVE_TEST_NAME,
            "--",
            "--ignored",
            "--nocapture",
        ])
        .current_dir(workspace_root)
        .env("PURE_STUDIO_WORKFLOW_ARTIFACT_DIR", artifact_dir)
        .env("PURE_STUDIO_WIRE_CAPTURE_DIR", wire_dir);
    let timeout = deadline.saturating_duration_since(Instant::now());
    ensure!(
        !timeout.is_zero(),
        "workflow headless acceptance exceeded 30 minutes before starting"
    );
    resident::run_logged_with_timeout(
        &mut command,
        "real-model headless workflow acceptance",
        &artifact_dir.join("headless.stdout.log"),
        &artifact_dir.join("headless.stderr.log"),
        timeout,
    )
}

fn run_gui(
    workspace_root: &Path,
    artifact_dir: &Path,
    wire_dir: &Path,
    prompt: &Path,
    scope: WorkflowAcceptanceScope,
    deadline: Instant,
) -> Result<()> {
    let installed_home = current_home()?.join(".pure");
    let installed_config = installed_home.join("config.toml");
    let installed_state_before = user_config_state(&installed_home)?;
    fs::write(
        artifact_dir.join("installed-user-state.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schemaVersion": 1,
            "configPresent": installed_config.is_file(),
            "before": &installed_state_before,
            "after": &installed_state_before,
            "unchanged": true,
        }))?,
    )?;
    ensure!(
        installed_config.is_file(),
        "installed Studio config is required for live GUI acceptance: {}",
        installed_config.display()
    );
    let root = tempfile::Builder::new()
        .prefix(match scope {
            WorkflowAcceptanceScope::Full => "pure-workflow-live-gui-",
            WorkflowAcceptanceScope::PlanOnly => "pure-workflow-plan-live-gui-",
        })
        .tempdir()
        .context("failed to create isolated GUI acceptance root")?;
    let simple_studio_home = root.path().join("studio-home-simple");
    let task_studio_home = root.path().join("studio-home-task");
    let simple_fixture_workspace = root.path().join("workspace-simple");
    let task_fixture_workspace = root.path().join("workspace-task");
    fs::create_dir_all(&task_studio_home)?;
    fs::create_dir_all(&task_fixture_workspace)?;
    if scope == WorkflowAcceptanceScope::Full {
        fs::create_dir_all(&simple_studio_home)?;
        fs::create_dir_all(&simple_fixture_workspace)?;
        write_isolated_live_config(
            &installed_config,
            &simple_studio_home.join("config.toml"),
            &artifact_dir.join("model-routes.json"),
        )?;
    }
    write_isolated_live_config(
        &installed_config,
        &task_studio_home.join("config.toml"),
        &artifact_dir.join("model-routes.json"),
    )?;
    let installed_agents = installed_home.join("agents");
    if installed_agents.is_dir() {
        if scope == WorkflowAcceptanceScope::Full {
            copy_directory(&installed_agents, &simple_studio_home.join("agents"))?;
        }
        copy_directory(&installed_agents, &task_studio_home.join("agents"))?;
    }
    let canonical_workspace = workspace_root
        .join("test-fixtures")
        .join("workflow-live")
        .join("workspace");
    if scope == WorkflowAcceptanceScope::Full {
        copy_directory(&canonical_workspace, &simple_fixture_workspace)?;
    }
    copy_directory(&canonical_workspace, &task_fixture_workspace)?;
    let simple_prompt = workspace_root
        .join("test-fixtures")
        .join("workflow-live")
        .join("simple-prompt.md");

    let acceptance = match scope {
        WorkflowAcceptanceScope::Full => (|| {
            let simple = run_gui_attempt(GuiAttempt {
                workspace_root,
                artifact_dir,
                wire_dir: &wire_dir.join("simple"),
                studio_home: &simple_studio_home,
                fixture_workspace: &simple_fixture_workspace,
                prompt: &simple_prompt,
                mode: "new",
                attempt: 1,
                studio_mode: "mode.simple",
                scope,
                deadline,
            })?;
            ensure!(
                simple.workflow_run_id.is_none(),
                "mode.simple GUI receipt unexpectedly contains a workflow identity: {simple:?}"
            );
            let task = run_gui_attempt(GuiAttempt {
                workspace_root,
                artifact_dir,
                wire_dir: &wire_dir.join("task-new"),
                studio_home: &task_studio_home,
                fixture_workspace: &task_fixture_workspace,
                prompt,
                mode: "new",
                attempt: 2,
                studio_mode: "mode.task",
                scope,
                deadline,
            })?;
            let reopened = run_gui_attempt(GuiAttempt {
                workspace_root,
                artifact_dir,
                wire_dir: &wire_dir.join("task-resume"),
                studio_home: &task_studio_home,
                fixture_workspace: &task_fixture_workspace,
                prompt,
                mode: "resume",
                attempt: 3,
                studio_mode: "mode.task",
                scope,
                deadline,
            })?;
            ensure!(
                task == reopened,
                "GUI reopen selected a different durable workflow: first={task:?}, reopened={reopened:?}"
            );
            fs::write(
                artifact_dir.join("gui-shutdown-reopen.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "schemaVersion": 1,
                    "simple": simple,
                    "first": task,
                    "reopened": reopened,
                    "sameWorkflow": true,
                }))?,
            )?;
            validate_delivered_fixture(
                &canonical_workspace,
                &simple_fixture_workspace,
                &artifact_dir.join("simple"),
            )?;
            validate_delivered_fixture(
                &canonical_workspace,
                &task_fixture_workspace,
                &artifact_dir.join("task"),
            )
        })(),
        WorkflowAcceptanceScope::PlanOnly => (|| {
            let task = run_gui_attempt(GuiAttempt {
                workspace_root,
                artifact_dir,
                wire_dir: &wire_dir.join("task-plan"),
                studio_home: &task_studio_home,
                fixture_workspace: &task_fixture_workspace,
                prompt,
                mode: "new",
                attempt: 1,
                studio_mode: "mode.task",
                scope,
                deadline,
            })?;
            ensure!(
                task.workflow_run_id.is_some(),
                "Plan-only GUI receipt has no workflow identity: {task:?}"
            );
            validate_plan_only_workspace(
                &canonical_workspace,
                &task_fixture_workspace,
                &artifact_dir.join("plan-only"),
            )
        })(),
    };
    let diff_artifact = match scope {
        WorkflowAcceptanceScope::Full => (|| {
            write_workspace_diff(
                &canonical_workspace,
                &simple_fixture_workspace,
                artifact_dir,
                "workspace-file-diff-simple.json",
            )?;
            write_workspace_diff(
                &canonical_workspace,
                &task_fixture_workspace,
                artifact_dir,
                "workspace-file-diff-task.json",
            )
        })(),
        WorkflowAcceptanceScope::PlanOnly => write_workspace_diff(
            &canonical_workspace,
            &task_fixture_workspace,
            artifact_dir,
            "workspace-file-diff-plan-only.json",
        ),
    };
    let installed_state_after = user_config_state(&installed_home);
    let state_check = installed_state_after.and_then(|after| {
        fs::write(
            artifact_dir.join("installed-user-state.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "before": installed_state_before,
                "after": after,
                "unchanged": installed_state_before == after,
            }))?,
        )?;
        ensure!(
            installed_state_before == after,
            "installed ~/.pure/config.toml or Agent files changed during isolated acceptance"
        );
        Ok(())
    });
    merge_acceptance_results(acceptance, diff_artifact, state_check)
}

struct GuiAttempt<'a> {
    workspace_root: &'a Path,
    artifact_dir: &'a Path,
    wire_dir: &'a Path,
    studio_home: &'a Path,
    fixture_workspace: &'a Path,
    prompt: &'a Path,
    mode: &'static str,
    attempt: u32,
    studio_mode: &'static str,
    scope: WorkflowAcceptanceScope,
    deadline: Instant,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct DriverWorkflowIdentity {
    project_id: String,
    thread_id: String,
    title: String,
    workflow_run_id: Option<String>,
}

fn run_gui_attempt(attempt: GuiAttempt<'_>) -> Result<DriverWorkflowIdentity> {
    let remaining = attempt.deadline.saturating_duration_since(Instant::now());
    ensure!(
        !remaining.is_zero(),
        "workflow GUI acceptance exceeded 30 minutes"
    );
    fs::create_dir_all(attempt.wire_dir)?;
    let prefix = format!("gui-attempt-{}", attempt.attempt);
    let mut command = Command::new("cargo");
    command
        .args(["xtask", "run-gui", "--driver", "--log-level", "debug"])
        .current_dir(attempt.workspace_root)
        .env("PURE_STUDIO_HOME", attempt.studio_home)
        .env("PURE_STUDIO_WIRE_CAPTURE_DIR", attempt.wire_dir)
        .env(
            "PURE_STUDIO_NATIVE_LIFECYCLE_LOG",
            attempt.artifact_dir.join(format!("{prefix}-native.log")),
        );
    let mut gui = resident::ResidentProcess::start(
        &mut command,
        &attempt
            .artifact_dir
            .join(format!("{prefix}-gui.stdout.log")),
        &attempt
            .artifact_dir
            .join(format!("{prefix}-gui.stderr.log")),
    )?;
    let acceptance = (|| {
        let vm_service = gui.wait_for_vm_service(remaining)?;
        fs::write(
            attempt
                .artifact_dir
                .join(format!("{prefix}-vm-service.txt")),
            format!("{vm_service}\n"),
        )?;
        let app_dir = paths::studio_app_dir(attempt.workspace_root);
        let driver_remaining = attempt.deadline.saturating_duration_since(Instant::now());
        ensure!(
            !driver_remaining.is_zero(),
            "workflow GUI acceptance exceeded 30 minutes before Driver started"
        );
        let args = driver_args(&attempt, &vm_service, driver_remaining.as_secs().max(1));
        let display = process::display_command("dart", &args);
        let mut driver = process::path_command("dart", &args);
        driver.current_dir(app_dir);
        let driver_stdout = attempt
            .artifact_dir
            .join(format!("{prefix}-driver.stdout.log"));
        resident::run_logged_with_timeout(
            &mut driver,
            &display,
            &driver_stdout,
            &attempt
                .artifact_dir
                .join(format!("{prefix}-driver.stderr.log")),
            driver_remaining,
        )?;
        write_driver_receipt(
            &driver_stdout,
            &attempt
                .artifact_dir
                .join(format!("{prefix}-workflow-receipt.json")),
            attempt.studio_mode,
            attempt.scope,
        )
    })();
    let process_tree = gui.write_process_tree(
        &attempt
            .artifact_dir
            .join(format!("{prefix}-last-process-tree.txt")),
    );
    let cleanup = gui.stop();
    merge_attempt_results(acceptance, process_tree, cleanup)
}

fn driver_args(
    attempt: &GuiAttempt<'_>,
    vm_service: &str,
    workflow_timeout_seconds: u64,
) -> Vec<OsString> {
    let prefix = format!("gui-attempt-{}", attempt.attempt);
    let mut args = Vec::new();
    for value in [
        "run",
        "test_driver/workflow_acceptance_driver.dart",
        "--mode",
        attempt.mode,
        "--vm-service-url",
        vm_service,
        "--studio-mode",
        attempt.studio_mode,
        "--workspace",
    ] {
        args.push(OsString::from(value));
    }
    args.push(attempt.fixture_workspace.as_os_str().to_owned());
    args.push(OsString::from("--snapshot-output"));
    args.push(
        attempt
            .artifact_dir
            .join(format!("{prefix}-snapshots.jsonl"))
            .into_os_string(),
    );
    args.push(OsString::from("--workflow-timeout-seconds"));
    args.push(OsString::from(workflow_timeout_seconds.to_string()));
    args.push(OsString::from("--stall-timeout-seconds"));
    args.push(OsString::from(STALL_TIMEOUT_SECONDS.to_string()));
    args.push(OsString::from("--attempt"));
    args.push(OsString::from(if attempt.attempt == 1 { "1" } else { "2" }));
    args.push(OsString::from("--acceptance-scope"));
    args.push(OsString::from(attempt.scope.driver_value()));
    args.push(OsString::from("--shutdown-after-completion"));
    args.push(OsString::from("true"));
    if attempt.mode == "new" {
        args.extend([
            OsString::from("--prompt-file"),
            attempt.prompt.as_os_str().to_owned(),
        ]);
    }
    args
}

fn validate_delivered_fixture(canonical: &Path, workspace: &Path, artifacts: &Path) -> Result<()> {
    fs::create_dir_all(artifacts)?;
    for path in [
        "Cargo.toml",
        "Cargo.lock",
        "src/lib.rs",
        "src/bin/fixture_verify.rs",
        "README.md",
        "AGENTS.md",
        ".gitignore",
        "docs/product-contract.md",
        ".agents/skills/workflow-fixture-rust/SKILL.md",
    ] {
        ensure!(
            fs::read(canonical.join(path))? == fs::read(workspace.join(path))?,
            "GUI workflow modified protected fixture file `{path}`"
        );
    }
    validate_fixture_skill_usage(workspace)?;
    let mut changed = Vec::new();
    collect_relative_files(workspace, workspace, &mut changed)?;
    changed
        .retain(|path| fs::read(canonical.join(path)).ok() != fs::read(workspace.join(path)).ok());
    changed.sort_unstable();
    validate_delivery_changes(&changed)?;

    let mut tests = Command::new("cargo");
    tests.args(["test"]).current_dir(workspace);
    resident::run_logged(
        &mut tests,
        "GUI fixture cargo test",
        &artifacts.join("fixture-cargo-test.stdout.log"),
        &artifacts.join("fixture-cargo-test.stderr.log"),
    )?;
    let mut verifier = Command::new("cargo");
    verifier
        .args(["run", "--quiet", "--bin", "fixture_verify"])
        .current_dir(workspace);
    let verifier_stdout = artifacts.join("fixture-verifier.stdout.log");
    resident::run_logged(
        &mut verifier,
        "GUI fixture deterministic verifier",
        &verifier_stdout,
        &artifacts.join("fixture-verifier.stderr.log"),
    )?;
    ensure!(
        fs::read_to_string(&verifier_stdout)?
            .lines()
            .any(|line| line.trim() == VERIFY_MARKER),
        "fixture verifier did not print `{VERIFY_MARKER}`"
    );
    ensure!(
        !workspace.join(".git").exists(),
        "workflow fixture must not initialize Git"
    );
    ensure!(
        !workspace.join(".pure").exists(),
        "workflow fixture must not create .pure state"
    );
    fs::write(artifacts.join("workspace-git-check.txt"), "git=false\n")?;
    Ok(())
}

fn validate_plan_only_workspace(
    canonical: &Path,
    workspace: &Path,
    artifacts: &Path,
) -> Result<()> {
    fs::create_dir_all(artifacts)?;
    let mut canonical_files = Vec::new();
    let mut workspace_files = Vec::new();
    collect_relative_files(canonical, canonical, &mut canonical_files)?;
    collect_relative_files(workspace, workspace, &mut workspace_files)?;
    let files = canonical_files
        .into_iter()
        .chain(workspace_files)
        .filter(|path| path != WORKFLOW_FIXTURE_USAGE_PATH)
        .collect::<BTreeSet<_>>();
    for path in files {
        ensure!(
            fs::read(canonical.join(&path)).ok() == fs::read(workspace.join(&path)).ok(),
            "Plan-only GUI acceptance modified project file `{path}`"
        );
    }
    if workspace.join(WORKFLOW_FIXTURE_USAGE_PATH).is_file() {
        validate_fixture_skill_usage(workspace)?;
    }
    ensure!(
        !workspace.join(".git").exists(),
        "Plan-only workflow fixture must not initialize Git"
    );
    ensure!(
        !workspace.join(".pure").exists(),
        "Plan-only workflow fixture must not create .pure state"
    );
    fs::write(
        artifacts.join("workspace-check.txt"),
        "projectFilesUnchanged=true\ngit=false\npureState=false\n",
    )?;
    Ok(())
}

fn validate_delivery_changes(changed: &[String]) -> Result<()> {
    let mut delivery_changes = changed
        .iter()
        .map(String::as_str)
        .filter(|path| *path != WORKFLOW_FIXTURE_USAGE_PATH)
        .collect::<Vec<_>>();
    delivery_changes.sort_unstable();
    let mut expected = EXPECTED_DELIVERY_PATHS.to_vec();
    expected.sort_unstable();
    ensure!(
        delivery_changes == expected,
        "GUI workflow changed unexpected files: {changed:?}"
    );
    Ok(())
}

fn validate_fixture_skill_usage(workspace: &Path) -> Result<()> {
    let path = workspace.join(WORKFLOW_FIXTURE_USAGE_PATH);
    let usage = serde_json::from_slice::<WorkflowFixtureSkillUsage>(
        &fs::read(&path).with_context(|| {
            format!(
                "workflow fixture Skill was not activated: {}",
                path.display()
            )
        })?,
    )
    .with_context(|| format!("invalid workflow fixture Skill usage: {}", path.display()))?;
    ensure!(
        usage.created_by == "agent"
            && usage.views > 0
            && usage.views == usage.uses
            && usage.patches == 0
            && usage.created_at > 0
            && usage.updated_at >= usage.created_at
            && usage.last_viewed_at.is_some_and(|last_viewed_at| {
                last_viewed_at >= usage.created_at && last_viewed_at <= usage.updated_at
            })
            && !usage.pinned,
        "workflow fixture Skill usage does not describe read-only agent activation: {usage:?}"
    );
    Ok(())
}

fn collect_relative_files(root: &Path, current: &Path, output: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.file_name().is_some_and(|name| name == "target") {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            bail!(
                "fixture traversal refuses symbolic link `{}`",
                path.display()
            );
        }
        if file_type.is_dir() {
            collect_relative_files(root, &path, output)?;
        } else if file_type.is_file() {
            output.push(
                path.strip_prefix(root)
                    .expect("walk path must be below root")
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    Ok(())
}

fn write_driver_receipt(
    log: &Path,
    output: &Path,
    studio_mode: &str,
    scope: WorkflowAcceptanceScope,
) -> Result<DriverWorkflowIdentity> {
    let mut completed = None;
    let mut shutdown = None;
    for line in fs::read_to_string(log)?.lines() {
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if record.get("result").and_then(serde_json::Value::as_str) == Some("completed") {
            completed = Some(record.clone());
        }
        if record.get("event").and_then(serde_json::Value::as_str)
            == Some("studioShutdownCompleted")
        {
            shutdown = Some(record);
        }
    }
    let completed = completed.context("Flutter Driver emitted no completed workflow receipt")?;
    let shutdown = shutdown.context("Flutter Driver emitted no durable shutdown receipt")?;
    let complete = completed
        .pointer("/workspace/timeline")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|item| {
            item.get("tools")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
        })
        .find(|tool| {
            tool.get("name").and_then(serde_json::Value::as_str) == Some("complete")
                && tool.get("status").and_then(serde_json::Value::as_str) == Some("succeeded")
        })
        .cloned();
    match scope {
        WorkflowAcceptanceScope::Full if studio_mode == "mode.task" => {
            ensure!(
                complete.is_some(),
                "Flutter Driver emitted no successful complete tool receipt"
            );
            ensure!(
                completed
                    .pointer("/workflow/currentRun/currentStateId")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|stage| stage == "completed"),
                "Flutter Driver completed receipt does not contain a terminal workflow"
            );
        }
        WorkflowAcceptanceScope::Full => {
            ensure!(
                complete.is_some(),
                "Flutter Driver emitted no successful complete tool receipt"
            );
            ensure!(
                completed
                    .get("workflow")
                    .is_some_and(serde_json::Value::is_null),
                "Flutter Driver simple receipt unexpectedly contains a workflow"
            );
        }
        WorkflowAcceptanceScope::PlanOnly => {
            ensure!(
                studio_mode == "mode.task",
                "Plan-only acceptance must use mode.task"
            );
            ensure!(
                completed
                    .get("acceptanceScope")
                    .and_then(serde_json::Value::as_str)
                    == Some("plan-only"),
                "Flutter Driver receipt does not identify the Plan-only scope"
            );
            ensure!(
                completed
                    .get("planState")
                    .and_then(serde_json::Value::as_str)
                    == Some("approved"),
                "Flutter Driver receipt does not contain an approved canonical Plan"
            );
            ensure!(
                completed
                    .pointer("/workflow/currentRun/currentStateId")
                    .and_then(serde_json::Value::as_str)
                    == Some("planning")
                    && completed
                        .pointer("/workflow/currentRun/terminal")
                        .and_then(serde_json::Value::as_bool)
                        == Some(false),
                "Plan-only receipt left planning or reached a terminal workflow"
            );
            ensure!(
                complete.is_none(),
                "Plan-only acceptance unexpectedly called complete"
            );
        }
    }
    let workflow_run_id = completed
        .pointer("/workflow/currentRun/runId")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    if studio_mode == "mode.task" {
        ensure!(
            workflow_run_id.is_some(),
            "Flutter Driver task receipt has no workflow run id"
        );
    }
    let identity = DriverWorkflowIdentity {
        project_id: completed
            .pointer("/workspace/projectId")
            .and_then(serde_json::Value::as_str)
            .context("Flutter Driver receipt has no Project id")?
            .to_string(),
        thread_id: completed
            .pointer("/workspace/threadId")
            .and_then(serde_json::Value::as_str)
            .context("Flutter Driver receipt has no Thread id")?
            .to_string(),
        title: completed
            .pointer("/workspace/title")
            .and_then(serde_json::Value::as_str)
            .context("Flutter Driver receipt has no Thread title")?
            .to_string(),
        workflow_run_id,
    };
    fs::write(
        output,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schemaVersion": 1,
            "identity": identity,
            "scope": scope.driver_value(),
            "completed": completed,
            "complete": complete,
            "shutdown": shutdown,
        }))?,
    )?;
    Ok(identity)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LiveRouteManifest {
    schema_version: u32,
    routes: Vec<LiveRouteEntry>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LiveRouteEntry {
    role: String,
    provider: String,
    model: String,
    base_url: String,
    credential_source: String,
    credential_resolution: String,
}

fn write_isolated_live_config(
    source: &Path,
    destination: &Path,
    route_manifest: &Path,
) -> Result<()> {
    let source_text = fs::read_to_string(source).with_context(|| {
        format!(
            "failed to read installed Studio config `{}`",
            source.display()
        )
    })?;
    let mut config = source_text
        .parse::<toml::Table>()
        .with_context(|| format!("installed Studio config `{}` is invalid", source.display()))?;
    upgrade_live_config_copy(&mut config)?;
    let routes = validate_live_routes(&config)?;
    fs::write(route_manifest, serde_json::to_vec_pretty(&routes)?)?;
    let instructions = config
        .entry("instructions")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .context("installed Studio config instructions is not a table")?;
    append_toml_context(
        instructions,
        "developer",
        "WORKFLOW_LIVE_GLOBAL_DEVELOPER_CONTEXT: preserve the complete workflow acceptance contract.",
    )?;
    append_toml_context(
        instructions,
        "user",
        "WORKFLOW_LIVE_GLOBAL_USER_CONTEXT: execute the canonical fixture prompt exactly.",
    )?;
    fs::write(destination, toml::to_string_pretty(&config)?).with_context(|| {
        format!(
            "failed to write isolated config `{}`",
            destination.display()
        )
    })
}

fn upgrade_live_config_copy(config: &mut toml::Table) -> Result<()> {
    let schema_version = config
        .get("schema_version")
        .and_then(toml::Value::as_integer)
        .context("installed Studio config has no integer schema_version")?;
    if schema_version == LIVE_CONFIG_SCHEMA_VERSION {
        return Ok(());
    }
    ensure!(
        matches!(schema_version, 15 | 16),
        "installed Studio config schema {schema_version} cannot be upgraded in the live acceptance copy to schema {LIVE_CONFIG_SCHEMA_VERSION}"
    );
    let routes = config
        .get_mut("models")
        .and_then(toml::Value::as_table_mut)
        .and_then(|models| models.get_mut("routes"))
        .and_then(toml::Value::as_table_mut)
        .context("installed Studio config has no models.routes table")?;
    if !routes.contains_key("worktree_executor") {
        let executor = routes
            .get("executor")
            .cloned()
            .context("installed Studio config has no executor route to migrate")?;
        routes.insert("worktree_executor".to_string(), executor);
    }
    config.insert(
        "schema_version".to_string(),
        toml::Value::Integer(LIVE_CONFIG_SCHEMA_VERSION),
    );
    Ok(())
}

fn validate_live_routes(config: &toml::Table) -> Result<LiveRouteManifest> {
    let models = config
        .get("models")
        .and_then(toml::Value::as_table)
        .context("installed Studio config has no models table")?;
    let providers = models
        .get("providers")
        .and_then(toml::Value::as_table)
        .context("installed Studio config has no models.providers table")?;
    let routes = models
        .get("routes")
        .and_then(toml::Value::as_table)
        .context("installed Studio config has no models.routes table")?;
    let mut manifest = Vec::new();
    for role in [
        "explorer",
        "planner",
        "executor",
        "worktree_executor",
        "reviewer",
    ] {
        let route = routes
            .get(role)
            .and_then(toml::Value::as_table)
            .with_context(|| format!("installed Studio config has no {role} model route"))?;
        let provider_id = route
            .get("provider")
            .and_then(toml::Value::as_str)
            .with_context(|| format!("{role} model route has no provider"))?;
        let model = route
            .get("model")
            .and_then(toml::Value::as_str)
            .with_context(|| format!("{role} model route has no model"))?;
        let provider = providers
            .get(provider_id)
            .and_then(toml::Value::as_table)
            .with_context(|| format!("{role} route references missing provider {provider_id}"))?;
        let base_url = provider
            .get("base_url")
            .and_then(toml::Value::as_str)
            .with_context(|| format!("provider {provider_id} has no base_url"))?;
        let lower = base_url.to_ascii_lowercase();
        ensure!(
            !["localhost", "127.0.0.1", "[::1]", "0.0.0.0"]
                .iter()
                .any(|host| lower.contains(host)),
            "live GUI route {role} points to local/scripted endpoint `{base_url}`"
        );
        let inline = provider
            .get("bearer_token")
            .and_then(toml::Value::as_str)
            .is_some_and(|token| !token.trim().is_empty());
        ensure!(
            !inline,
            "live GUI provider {provider_id} uses a forbidden inline bearer token"
        );
        let environment_selector = provider
            .get("bearer_token_env")
            .and_then(toml::Value::as_str)
            .map(str::trim);
        ensure!(
            environment_selector.is_none_or(|name| !name.is_empty()),
            "live GUI provider {provider_id} has an empty bearer_token_env"
        );
        // SystemCredentialStore 和环境变量 fallback 都是 Studio runtime 的产品边界；
        // xtask 不读取或复制密钥。真实 provider request 是最终的 credential 门禁。
        let credential_source = if environment_selector.is_some() {
            "systemCredentialStoreOrEnvironmentFallback"
        } else {
            "systemCredentialStore"
        };
        let credential_resolution = "deferredToStudioRuntime";
        manifest.push(LiveRouteEntry {
            role: role.to_string(),
            provider: provider_id.to_string(),
            model: model.to_string(),
            base_url: base_url.to_string(),
            credential_source: credential_source.to_string(),
            credential_resolution: credential_resolution.to_string(),
        });
    }
    Ok(LiveRouteManifest {
        schema_version: 1,
        routes: manifest,
    })
}

fn append_toml_context(table: &mut toml::Table, key: &str, marker: &str) -> Result<()> {
    let existing = table
        .get(key)
        .map(|value| {
            value
                .as_str()
                .with_context(|| format!("instructions.{key} is not a string"))
        })
        .transpose()?
        .unwrap_or_default();
    let value = if existing.trim().is_empty() {
        marker.to_string()
    } else {
        format!("{existing}\n\n{marker}")
    };
    table.insert(key.to_string(), toml::Value::String(value));
    Ok(())
}

fn copy_directory(source: &Path, target: &Path) -> Result<()> {
    for entry in fs::read_dir(source)
        .with_context(|| format!("failed to read fixture directory `{}`", source.display()))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&target_path)?;
            copy_directory(&source_path, &target_path)?;
        } else {
            fs::copy(&source_path, &target_path)?;
        }
    }
    Ok(())
}

fn user_config_state(studio_home: &Path) -> Result<String> {
    let mut files = Vec::new();
    let config = studio_home.join("config.toml");
    if config.is_file() {
        files.push(config);
    }
    let agents = studio_home.join("agents");
    if agents.is_dir() {
        collect_owned_toml_files(&agents, &mut files)?;
    }
    files.sort_unstable();
    let mut hasher = Sha256::new();
    for path in files {
        let relative = path
            .strip_prefix(studio_home)
            .with_context(|| format!("user config path escaped Studio home: {}", path.display()))?;
        let bytes = fs::read(&path)?;
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update([0]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn collect_owned_toml_files(directory: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_owned_toml_files(&path, output)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "toml")
        {
            output.push(path);
        }
    }
    Ok(())
}

fn write_workspace_diff(
    canonical: &Path,
    workspace: &Path,
    artifacts: &Path,
    output_name: &str,
) -> Result<()> {
    let mut canonical_files = Vec::new();
    let mut workspace_files = Vec::new();
    collect_relative_files(canonical, canonical, &mut canonical_files)?;
    collect_relative_files(workspace, workspace, &mut workspace_files)?;
    let paths = canonical_files
        .into_iter()
        .chain(workspace_files)
        .collect::<BTreeSet<_>>();
    let changed = paths
        .into_iter()
        .filter_map(|relative| {
            let before = fs::read(canonical.join(&relative)).ok();
            let after = fs::read(workspace.join(&relative)).ok();
            (before != after).then(|| {
                serde_json::json!({
                    "path": relative,
                    "beforeSha256": before.map(|bytes| format!("{:x}", Sha256::digest(bytes))),
                    "afterSha256": after.map(|bytes| format!("{:x}", Sha256::digest(bytes))),
                })
            })
        })
        .collect::<Vec<_>>();
    fs::write(
        artifacts.join(output_name),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schemaVersion": 1,
            "changed": changed,
        }))?,
    )?;
    Ok(())
}

fn merge_acceptance_results(
    acceptance: Result<()>,
    diff_artifact: Result<()>,
    state_check: Result<()>,
) -> Result<()> {
    match acceptance {
        Err(error) => {
            let mut diagnostics = Vec::new();
            if let Err(extra) = diff_artifact {
                diagnostics.push(format!("workspace diff artifact failed: {extra:#}"));
            }
            if let Err(extra) = state_check {
                diagnostics.push(format!("installed user state check failed: {extra:#}"));
            }
            if diagnostics.is_empty() {
                Err(error)
            } else {
                Err(error.context(diagnostics.join("; ")))
            }
        }
        Ok(()) => {
            diff_artifact?;
            state_check
        }
    }
}

fn merge_attempt_results<T>(
    acceptance: Result<T>,
    process_tree: Result<()>,
    cleanup: Result<()>,
) -> Result<T> {
    match acceptance {
        Err(error) => {
            let mut diagnostics = Vec::new();
            if let Err(extra) = process_tree {
                diagnostics.push(format!("process tree artifact failed: {extra:#}"));
            }
            if let Err(extra) = cleanup {
                diagnostics.push(format!("GUI cleanup also failed: {extra:#}"));
            }
            if diagnostics.is_empty() {
                Err(error)
            } else {
                Err(error.context(diagnostics.join("; ")))
            }
        }
        Ok(value) => {
            process_tree?;
            cleanup?;
            Ok(value)
        }
    }
}

fn current_home() -> Result<PathBuf> {
    #[cfg(windows)]
    const HOME_VARS: &[&str] = &["USERPROFILE", "HOME"];
    #[cfg(not(windows))]
    const HOME_VARS: &[&str] = &["HOME", "USERPROFILE"];
    HOME_VARS
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .find(|path| !path.as_os_str().is_empty())
        .context("could not resolve current user home directory")
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn fixture_walk_rejects_directory_symlinks() {
        use std::os::unix::fs::symlink;

        let workspace = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        fs::write(external.path().join("outside.txt"), "outside").unwrap();
        symlink(external.path(), workspace.path().join("escape")).unwrap();

        let mut files = Vec::new();
        let error = collect_relative_files(workspace.path(), workspace.path(), &mut files)
            .expect_err("fixture traversal must not follow directory symlinks");
        assert!(error.to_string().contains("symbolic link"));
        assert!(files.is_empty());
    }

    #[test]
    fn live_config_copy_upgrades_only_the_immediately_previous_schema() {
        for schema_version in [15, 16] {
            let mut previous = toml::toml! {
                schema_version = schema_version
                [models.routes.executor]
                provider = "provider"
                model = "model"
            };
            upgrade_live_config_copy(&mut previous).unwrap();
            assert_eq!(
                previous
                    .get("schema_version")
                    .and_then(toml::Value::as_integer),
                Some(LIVE_CONFIG_SCHEMA_VERSION)
            );
            assert!(
                previous
                    .get("models")
                    .and_then(toml::Value::as_table)
                    .and_then(|models| models.get("routes"))
                    .and_then(toml::Value::as_table)
                    .is_some_and(|routes| routes.contains_key("worktree_executor"))
            );
        }

        let mut stale = toml::toml! {
            schema_version = 14
            [models.routes.executor]
            provider = "provider"
            model = "model"
        };
        let error = upgrade_live_config_copy(&mut stale).unwrap_err();
        assert!(error.to_string().contains("cannot be upgraded"));
    }

    #[test]
    fn live_routes_allow_runtime_environment_credential_fallback() {
        let config = toml::toml! {
            [models.providers.openai]
            base_url = "https://example.com"
            bearer_token_env = "OPENAI_API_KEY"

            [models.routes.explorer]
            provider = "openai"
            model = "model"
            [models.routes.planner]
            provider = "openai"
            model = "model"
            [models.routes.executor]
            provider = "openai"
            model = "model"
            [models.routes.worktree_executor]
            provider = "openai"
            model = "model"
            [models.routes.reviewer]
            provider = "openai"
            model = "model"
        };

        let manifest = validate_live_routes(&config).expect("runtime fallback is supported");

        assert!(manifest.routes.iter().all(|route| {
            route.credential_source == "systemCredentialStoreOrEnvironmentFallback"
                && route.credential_resolution == "deferredToStudioRuntime"
        }));
    }

    #[test]
    fn runtime_skill_usage_is_not_counted_as_a_delivery_change() {
        let mut changed = EXPECTED_DELIVERY_PATHS
            .iter()
            .map(|path| (*path).to_string())
            .collect::<Vec<_>>();
        changed.push(WORKFLOW_FIXTURE_USAGE_PATH.to_string());
        changed.sort_unstable();

        validate_delivery_changes(&changed)
            .expect("project Skill usage is runtime metadata, not agent delivery output");
    }

    #[test]
    fn only_the_fixture_skill_usage_sidecar_is_excluded() {
        let mut changed = EXPECTED_DELIVERY_PATHS
            .iter()
            .map(|path| (*path).to_string())
            .collect::<Vec<_>>();
        changed.push(".agents/skills/another-skill/.usage.json".to_string());
        changed.sort_unstable();

        assert!(validate_delivery_changes(&changed).is_err());
    }

    #[test]
    fn plan_only_workspace_allows_only_fixture_skill_usage_metadata() {
        let canonical = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let artifacts = tempfile::tempdir().unwrap();
        fs::create_dir_all(canonical.path().join("src")).unwrap();
        fs::create_dir_all(workspace.path().join("src")).unwrap();
        fs::write(canonical.path().join("src/lib.rs"), "pub fn value() {}\n").unwrap();
        fs::write(workspace.path().join("src/lib.rs"), "pub fn value() {}\n").unwrap();
        let usage = workspace.path().join(WORKFLOW_FIXTURE_USAGE_PATH);
        fs::create_dir_all(usage.parent().unwrap()).unwrap();
        fs::write(
            usage,
            serde_json::to_vec_pretty(&serde_json::json!({
                "createdBy": "agent",
                "views": 1,
                "uses": 1,
                "patches": 0,
                "createdAt": 10,
                "updatedAt": 11,
                "lastViewedAt": 11,
                "pinned": false
            }))
            .unwrap(),
        )
        .unwrap();

        validate_plan_only_workspace(canonical.path(), workspace.path(), artifacts.path())
            .expect("the known Skill usage sidecar is runtime metadata");
    }

    #[test]
    fn plan_only_workspace_rejects_project_file_changes() {
        let canonical = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let artifacts = tempfile::tempdir().unwrap();
        fs::create_dir_all(canonical.path().join("src")).unwrap();
        fs::create_dir_all(workspace.path().join("src")).unwrap();
        fs::write(canonical.path().join("src/lib.rs"), "pub fn value() {}\n").unwrap();
        fs::write(workspace.path().join("src/lib.rs"), "pub fn changed() {}\n").unwrap();

        let error =
            validate_plan_only_workspace(canonical.path(), workspace.path(), artifacts.path())
                .expect_err("Plan-only acceptance must reject implementation changes");
        assert!(
            error
                .to_string()
                .contains("modified project file `src/lib.rs`")
        );
    }

    #[test]
    fn fixture_skill_usage_proves_read_only_agent_activation() {
        let workspace = tempfile::tempdir().unwrap();
        let usage_path = workspace.path().join(WORKFLOW_FIXTURE_USAGE_PATH);
        fs::create_dir_all(usage_path.parent().unwrap()).unwrap();
        fs::write(
            &usage_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "createdBy": "agent",
                "views": 1,
                "uses": 1,
                "patches": 0,
                "createdAt": 10,
                "updatedAt": 11,
                "lastViewedAt": 11,
                "pinned": false
            }))
            .unwrap(),
        )
        .unwrap();

        validate_fixture_skill_usage(workspace.path()).unwrap();
    }
}
