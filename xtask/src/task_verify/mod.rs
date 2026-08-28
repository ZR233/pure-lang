use crate::cli::VerifyTaskOptions;
use crate::{paths, process};
use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

mod artifact;
mod resident;

const LIVE_TEST_NAME: &str =
    "installed_config_task_mode_delivers_two_rust_workstreams_and_recovers";
const VERIFY_MARKER: &str = "PURE_TASK_FIXTURE_VERIFY_OK";
const LIVE_CONFIG_SCHEMA_VERSION: i64 = 15;

pub(crate) fn run(options: VerifyTaskOptions) -> Result<()> {
    ensure!(
        options.live,
        "verify-task requires --live because it uses real credentials, incurs model fees, and never falls back to a scripted provider"
    );
    let workspace_root = paths::workspace_root()?;
    let surface = if options.headless { "headless" } else { "gui" };
    let artifact_dir = workspace_root
        .join("target")
        .join("task-live-artifacts")
        .join(format!("{surface}-{}-{}", std::process::id(), unix_nanos()));
    fs::create_dir_all(&artifact_dir)?;
    let prompt = workspace_root
        .join("test-fixtures")
        .join("task-live")
        .join("prompt.md");
    let prompt_bytes = fs::read(&prompt)
        .with_context(|| format!("failed to read canonical prompt `{}`", prompt.display()))?;
    let prompt_hash = format!("{:x}", Sha256::digest(&prompt_bytes));
    fs::write(artifact_dir.join("fixture-prompt.md"), &prompt_bytes)?;
    fs::write(
        artifact_dir.join("fixture-prompt.sha256"),
        format!("{prompt_hash}\n"),
    )?;
    fs::write(
        artifact_dir.join("acceptance-surface.txt"),
        format!("surface={surface}\nscriptedProvider=false\nlive=true\n"),
    )?;
    println!("Task live artifacts: {}", artifact_dir.display());

    let wire_dir = artifact_dir.join("wire");
    fs::create_dir_all(&wire_dir)?;
    let acceptance = if options.headless {
        run_headless(&workspace_root, &artifact_dir, &wire_dir)
    } else if options.gui {
        run_gui(&workspace_root, &artifact_dir, &wire_dir, &prompt)
    } else {
        bail!("verify-task requires exactly one of --headless or --gui")
    };
    let manifest = artifact::finalize(&artifact_dir, &wire_dir, surface, &prompt_hash);
    match (acceptance, manifest) {
        (Ok(()), Ok(())) => {
            fs::write(artifact_dir.join("result.txt"), "completed\n")?;
            println!("Task live acceptance completed: {}", artifact_dir.display());
            Ok(())
        }
        (Err(error), Ok(())) => Err(error.context(format!(
            "Task acceptance artifacts were preserved at {}",
            artifact_dir.display()
        ))),
        (Ok(()), Err(error)) => Err(error.context(format!(
            "Task wire acceptance failed; artifacts are at {}",
            artifact_dir.display()
        ))),
        (Err(error), Err(manifest_error)) => Err(error.context(format!(
            "wire manifest also failed: {manifest_error:#}; artifacts are at {}",
            artifact_dir.display()
        ))),
    }
}

fn run_headless(workspace_root: &Path, artifact_dir: &Path, wire_dir: &Path) -> Result<()> {
    let mut command = Command::new("cargo");
    command
        .args([
            "test",
            "-p",
            "pl-studio-runtime",
            "--features",
            "live-tests",
            "--test",
            "task_live",
            LIVE_TEST_NAME,
            "--",
            "--ignored",
            "--nocapture",
        ])
        .current_dir(workspace_root)
        .env("PURE_STUDIO_TASK_ARTIFACT_DIR", artifact_dir)
        .env("PURE_STUDIO_WIRE_CAPTURE_DIR", wire_dir);
    resident::run_logged(
        &mut command,
        "real-model headless Task acceptance",
        &artifact_dir.join("headless.stdout.log"),
        &artifact_dir.join("headless.stderr.log"),
    )
}

fn run_gui(
    workspace_root: &Path,
    artifact_dir: &Path,
    wire_dir: &Path,
    prompt: &Path,
) -> Result<()> {
    let installed_config = current_home()?.join(".pure").join("config.toml");
    ensure!(
        installed_config.is_file(),
        "installed Studio config is required for live GUI acceptance: {}",
        installed_config.display()
    );
    let root = tempfile::Builder::new()
        .prefix("pure-task-live-gui-")
        .tempdir()
        .context("failed to create isolated GUI acceptance root")?;
    let studio_home = root.path().join("studio-home");
    let fixture_workspace = root.path().join("workspace");
    fs::create_dir_all(&studio_home)?;
    fs::create_dir_all(&fixture_workspace)?;
    write_isolated_live_config(
        &installed_config,
        &studio_home.join("config.toml"),
        &artifact_dir.join("model-routes.json"),
    )?;
    let canonical_workspace = workspace_root
        .join("test-fixtures")
        .join("task-live")
        .join("workspace");
    copy_directory(&canonical_workspace, &fixture_workspace)?;
    git_checked(&fixture_workspace, &["init", "--initial-branch=main"])?;
    git_checked(&fixture_workspace, &["add", "."])?;
    git_checked(
        &fixture_workspace,
        &[
            "-c",
            "user.name=Pure Studio",
            "-c",
            "user.email=pure-studio@local",
            "commit",
            "-m",
            "test: initialize GUI live Task fixture",
        ],
    )?;
    let initial_head = git_output(&fixture_workspace, &["rev-parse", "HEAD"])?;

    let first = run_gui_attempt(GuiAttempt {
        workspace_root,
        artifact_dir,
        wire_dir: &wire_dir.join("new"),
        studio_home: &studio_home,
        fixture_workspace: &fixture_workspace,
        prompt,
        mode: "new",
        attempt: 1,
    })?;
    let reopened = run_gui_attempt(GuiAttempt {
        workspace_root,
        artifact_dir,
        wire_dir: &wire_dir.join("resume"),
        studio_home: &studio_home,
        fixture_workspace: &fixture_workspace,
        prompt,
        mode: "resume",
        attempt: 2,
    })?;
    ensure!(
        first == reopened,
        "GUI reopen selected a different durable Task: first={first:?}, reopened={reopened:?}"
    );
    fs::write(
        artifact_dir.join("gui-shutdown-reopen.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schemaVersion": 1,
            "first": first,
            "reopened": reopened,
            "sameTask": true,
        }))?,
    )?;

    validate_delivered_fixture(
        &canonical_workspace,
        &fixture_workspace,
        artifact_dir,
        &initial_head,
    )
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
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct DriverTaskIdentity {
    project_id: String,
    thread_id: String,
    task_run_id: String,
}

fn run_gui_attempt(attempt: GuiAttempt<'_>) -> Result<DriverTaskIdentity> {
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
        let vm_service = gui.wait_for_vm_service(std::time::Duration::from_secs(30 * 60))?;
        fs::write(
            attempt
                .artifact_dir
                .join(format!("{prefix}-vm-service.txt")),
            format!("{vm_service}\n"),
        )?;
        let app_dir = paths::studio_app_dir(attempt.workspace_root);
        let args = driver_args(&attempt, &vm_service);
        let display = process::display_command("dart", &args);
        let mut driver = process::path_command("dart", &args);
        driver.current_dir(app_dir);
        let driver_stdout = attempt
            .artifact_dir
            .join(format!("{prefix}-driver.stdout.log"));
        resident::run_logged(
            &mut driver,
            &display,
            &driver_stdout,
            &attempt
                .artifact_dir
                .join(format!("{prefix}-driver.stderr.log")),
        )?;
        write_driver_receipt(
            &driver_stdout,
            &attempt
                .artifact_dir
                .join(format!("{prefix}-task-receipt.json")),
        )
    })();
    let cleanup = gui.stop();
    match (acceptance, cleanup) {
        (Ok(identity), Ok(())) => Ok(identity),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup_error)) => {
            Err(error.context(format!("GUI cleanup also failed: {cleanup_error:#}")))
        }
    }
}

fn driver_args(attempt: &GuiAttempt<'_>, vm_service: &str) -> Vec<OsString> {
    let prefix = format!("gui-attempt-{}", attempt.attempt);
    let mut args = Vec::new();
    for value in [
        "run",
        "test_driver/task_acceptance_driver.dart",
        "--mode",
        attempt.mode,
        "--vm-service-url",
        vm_service,
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
    args.push(OsString::from("--progress-state-output"));
    args.push(
        attempt
            .artifact_dir
            .join(format!("{prefix}-progress.json"))
            .into_os_string(),
    );
    for value in [
        "--plan-timeout-seconds",
        "600",
        "--task-timeout-seconds",
        "1800",
        "--stall-timeout-seconds",
        "600",
        "--attempt",
        if attempt.attempt == 1 { "1" } else { "2" },
        "--recovery-count",
        "0",
        "--recovery-mode",
        "auto",
        "--expected-task-outcome",
        "succeeded",
        "--expect-budget-recovery",
        "false",
        "--shutdown-after-completion",
        "true",
    ] {
        args.push(OsString::from(value));
    }
    if attempt.mode == "new" {
        args.extend([
            OsString::from("--prompt-file"),
            attempt.prompt.as_os_str().to_owned(),
        ]);
    }
    args
}

fn validate_delivered_fixture(
    canonical: &Path,
    workspace: &Path,
    artifacts: &Path,
    initial_head: &str,
) -> Result<()> {
    for path in [
        "Cargo.toml",
        "Cargo.lock",
        "src/lib.rs",
        "src/bin/fixture_verify.rs",
        "README.md",
        "AGENTS.md",
        ".gitignore",
        "docs/product-contract.md",
        "skills/task-fixture-rust/SKILL.md",
    ] {
        ensure!(
            fs::read(canonical.join(path))? == fs::read(workspace.join(path))?,
            "GUI Task modified protected fixture file `{path}`"
        );
    }
    let changed = git_output(
        workspace,
        &["diff", "--name-only", &format!("{initial_head}..HEAD")],
    )?;
    let mut changed = changed.lines().collect::<Vec<_>>();
    changed.sort_unstable();
    let mut expected = vec![
        "design/task-workflows.md",
        "src/normalize.rs",
        "src/validate.rs",
        "tests/normalize.rs",
        "tests/validate.rs",
    ];
    expected.sort_unstable();
    ensure!(
        changed == expected,
        "GUI Task changed unexpected files: {changed:?}"
    );

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
    let status = git_output(workspace, &["status", "--porcelain=v1"])?;
    fs::write(artifacts.join("git-status.txt"), &status)?;
    ensure!(status.is_empty(), "GUI fixture Git tree is dirty: {status}");
    fs::write(
        artifacts.join("git-head.txt"),
        format!("{}\n", git_output(workspace, &["rev-parse", "HEAD"])?),
    )?;
    fs::write(
        artifacts.join("git-log.txt"),
        git_output(workspace, &["log", "--oneline", "--decorate", "--all"])?,
    )?;
    fs::write(
        artifacts.join("git-diff-stat.txt"),
        git_output(workspace, &["diff", "--stat", initial_head, "HEAD"])?,
    )?;
    fs::write(
        artifacts.join("git-diff.patch"),
        git_output(workspace, &["diff", "--binary", initial_head, "HEAD"])?,
    )?;
    Ok(())
}

fn write_driver_receipt(log: &Path, output: &Path) -> Result<DriverTaskIdentity> {
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
    let completed = completed.context("Flutter Driver emitted no completed Task receipt")?;
    let shutdown = shutdown.context("Flutter Driver emitted no durable shutdown receipt")?;
    ensure!(
        completed
            .pointer("/task/phase")
            .and_then(serde_json::Value::as_str)
            == Some("completed"),
        "Flutter Driver completed receipt does not contain a terminal Task"
    );
    let identity = DriverTaskIdentity {
        project_id: completed
            .pointer("/project/id")
            .and_then(serde_json::Value::as_str)
            .context("Flutter Driver receipt has no Project id")?
            .to_string(),
        thread_id: completed
            .pointer("/workspace/threadId")
            .and_then(serde_json::Value::as_str)
            .context("Flutter Driver receipt has no Thread id")?
            .to_string(),
        task_run_id: completed
            .pointer("/task/runId")
            .and_then(serde_json::Value::as_str)
            .context("Flutter Driver receipt has no TaskRun id")?
            .to_string(),
    };
    fs::write(
        output,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schemaVersion": 1,
            "identity": identity,
            "completed": completed,
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
        "TASK_LIVE_GLOBAL_DEVELOPER_CONTEXT: preserve the complete Task acceptance contract.",
    )?;
    append_toml_context(
        instructions,
        "user",
        "TASK_LIVE_GLOBAL_USER_CONTEXT: execute the canonical fixture prompt exactly.",
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
        schema_version.checked_add(1) == Some(LIVE_CONFIG_SCHEMA_VERSION),
        "installed Studio config schema {schema_version} cannot be upgraded in the live acceptance copy to schema {LIVE_CONFIG_SCHEMA_VERSION}"
    );
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
    for role in ["planner", "executor", "reviewer"] {
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
        let from_environment = provider
            .get("bearer_token_env")
            .and_then(toml::Value::as_str)
            .and_then(std::env::var_os)
            .is_some_and(|token| !token.is_empty());
        let (credential_source, credential_resolution) = if from_environment {
            ("environment", "resolvedByXtask")
        } else {
            // SystemCredentialStore 是 Studio runtime 的产品边界；xtask 不读取或复制
            // 系统密钥。真实 provider request 是最终的 credential resolution 门禁。
            ("systemCredentialStore", "deferredToStudioRuntime")
        };
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

fn git_checked(workspace: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn git_output(workspace: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
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

    #[test]
    fn live_config_copy_upgrades_only_the_immediately_previous_schema() {
        let mut previous = toml::toml! { schema_version = 14 };
        upgrade_live_config_copy(&mut previous).unwrap();
        assert_eq!(
            previous
                .get("schema_version")
                .and_then(toml::Value::as_integer),
            Some(LIVE_CONFIG_SCHEMA_VERSION)
        );

        let mut stale = toml::toml! { schema_version = 13 };
        let error = upgrade_live_config_copy(&mut stale).unwrap_err();
        assert!(error.to_string().contains("cannot be upgraded"));
    }
}
