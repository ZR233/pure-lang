use crate::cli::VerifyWorkflowOptions;
use crate::{paths, process};
use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod artifact;
mod resident;

const LIVE_TEST_NAME: &str = "installed_config_workflow_mode_delivers_rust_project";
const VERIFY_MARKER: &str = "PURE_WORKFLOW_GUI_VERIFY_OK";
const LIVE_CONFIG_SCHEMA_VERSION: i64 = 16;
const TOTAL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const STALL_TIMEOUT_SECONDS: u64 = 10 * 60;

pub(crate) fn run(options: VerifyWorkflowOptions) -> Result<()> {
    let deadline = Instant::now() + TOTAL_TIMEOUT;
    ensure!(
        options.live,
        "verify-workflow requires --live because it uses real credentials, incurs model fees, and never falls back to a scripted provider"
    );
    let workspace_root = paths::workspace_root()?;
    let surface = if options.headless { "headless" } else { "gui" };
    let artifact_dir = workspace_root
        .join("target")
        .join("workflow-live-artifacts")
        .join(format!("{surface}-{}-{}", std::process::id(), unix_nanos()));
    fs::create_dir_all(&artifact_dir)?;
    let prompt = workspace_root
        .join("test-fixtures")
        .join("workflow-live")
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
    println!("Workflow live artifacts: {}", artifact_dir.display());

    let wire_dir = artifact_dir.join("wire");
    fs::create_dir_all(&wire_dir)?;
    let acceptance = if options.headless {
        run_headless(&workspace_root, &artifact_dir, &wire_dir)
    } else if options.gui {
        run_gui(&workspace_root, &artifact_dir, &wire_dir, &prompt, deadline)
    } else {
        bail!("verify-workflow requires exactly one of --headless or --gui")
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
        Ok(_) => artifact::finalize(&artifact_dir, &wire_dir, surface, &prompt_hash),
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
            "workflow_live",
            LIVE_TEST_NAME,
            "--",
            "--ignored",
            "--nocapture",
        ])
        .current_dir(workspace_root)
        .env("PURE_STUDIO_WORKFLOW_ARTIFACT_DIR", artifact_dir)
        .env("PURE_STUDIO_WIRE_CAPTURE_DIR", wire_dir);
    resident::run_logged(
        &mut command,
        "real-model headless workflow acceptance",
        &artifact_dir.join("headless.stdout.log"),
        &artifact_dir.join("headless.stderr.log"),
    )
}

fn run_gui(
    workspace_root: &Path,
    artifact_dir: &Path,
    wire_dir: &Path,
    prompt: &Path,
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
        .prefix("pure-workflow-live-gui-")
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
    let installed_agents = installed_home.join("agents");
    if installed_agents.is_dir() {
        copy_directory(&installed_agents, &studio_home.join("agents"))?;
    }
    let canonical_workspace = workspace_root
        .join("test-fixtures")
        .join("workflow-live")
        .join("workspace");
    copy_directory(&canonical_workspace, &fixture_workspace)?;

    let acceptance = (|| {
        let first = run_gui_attempt(GuiAttempt {
            workspace_root,
            artifact_dir,
            wire_dir: &wire_dir.join("new"),
            studio_home: &studio_home,
            fixture_workspace: &fixture_workspace,
            prompt,
            mode: "new",
            attempt: 1,
            deadline,
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
            deadline,
        })?;
        ensure!(
            first == reopened,
            "GUI reopen selected a different durable workflow: first={first:?}, reopened={reopened:?}"
        );
        fs::write(
            artifact_dir.join("gui-shutdown-reopen.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "first": first,
                "reopened": reopened,
                "sameWorkflow": true,
            }))?,
        )?;
        validate_delivered_fixture(&canonical_workspace, &fixture_workspace, artifact_dir)
    })();
    let diff_artifact =
        write_workspace_diff(&canonical_workspace, &fixture_workspace, artifact_dir);
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
    deadline: Instant,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct DriverWorkflowIdentity {
    project_id: String,
    thread_id: String,
    workflow_run_id: String,
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
    for path in [
        "Cargo.toml",
        "Cargo.lock",
        "src/lib.rs",
        "src/bin/fixture_verify.rs",
        "README.md",
        "AGENTS.md",
        ".gitignore",
        "docs/product-contract.md",
        "skills/workflow-fixture-rust/SKILL.md",
    ] {
        ensure!(
            fs::read(canonical.join(path))? == fs::read(workspace.join(path))?,
            "GUI workflow modified protected fixture file `{path}`"
        );
    }
    let mut changed = Vec::new();
    collect_relative_files(workspace, workspace, &mut changed)?;
    changed
        .retain(|path| fs::read(canonical.join(path)).ok() != fs::read(workspace.join(path)).ok());
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
        "GUI workflow changed unexpected files: {changed:?}"
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

fn collect_relative_files(root: &Path, current: &Path, output: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.file_name().is_some_and(|name| name == "target") {
            continue;
        }
        if path.is_dir() {
            collect_relative_files(root, &path, output)?;
        } else {
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

fn write_driver_receipt(log: &Path, output: &Path) -> Result<DriverWorkflowIdentity> {
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
    ensure!(
        completed
            .pointer("/workflow/currentRun/currentStageId")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|stage| stage == "completed"),
        "Flutter Driver completed receipt does not contain a terminal workflow"
    );
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
        workflow_run_id: completed
            .pointer("/workflow/currentRun/runId")
            .and_then(serde_json::Value::as_str)
            .context("Flutter Driver receipt has no workflow run id")?
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
        let environment_selector = provider
            .get("bearer_token_env")
            .and_then(toml::Value::as_str)
            .is_some_and(|name| !name.trim().is_empty());
        ensure!(
            !environment_selector,
            "live GUI provider {provider_id} must resolve credentials only from the system credential store"
        );
        // SystemCredentialStore 是 Studio runtime 的产品边界；xtask 不读取或复制
        // 系统密钥。真实 provider request 是最终的 credential resolution 门禁。
        let credential_source = "systemCredentialStore";
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

fn write_workspace_diff(canonical: &Path, workspace: &Path, artifacts: &Path) -> Result<()> {
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
        artifacts.join("workspace-file-diff.json"),
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

    #[test]
    fn live_config_copy_upgrades_only_the_immediately_previous_schema() {
        let mut previous = toml::toml! { schema_version = 15 };
        upgrade_live_config_copy(&mut previous).unwrap();
        assert_eq!(
            previous
                .get("schema_version")
                .and_then(toml::Value::as_integer),
            Some(LIVE_CONFIG_SCHEMA_VERSION)
        );

        let mut stale = toml::toml! { schema_version = 14 };
        let error = upgrade_live_config_copy(&mut stale).unwrap_err();
        assert!(error.to_string().contains("cannot be upgraded"));
    }
}
