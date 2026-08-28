use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use pl_studio_runtime::{
    ConfigStore, InteractionKind, InteractionRequest, InteractionStatus,
    STUDIO_CONFIG_SCHEMA_VERSION, StudioConfig, StudioHostKind, StudioMode, StudioRole,
    StudioRuntime, StudioRuntimeOptions, StudioStore, StudioTaskRuntime, StudioTaskState,
};
use sha2::{Digest, Sha256};

use super::git::git_output;

pub const LIVE_VERIFY_MARKER: &str = "PURE_TASK_FIXTURE_VERIFY_OK";
pub const LIVE_TASK_PROMPT: &str = include_str!("../../../../test-fixtures/task-live/prompt.md");

const POLL_INTERVAL: Duration = Duration::from_millis(100);
const IDLE_FAILURE_GRACE: Duration = Duration::from_secs(10);

pub struct LiveTaskFixture {
    pub runtime: StudioRuntime,
    pub store: StudioStore,
    pub workspace: PathBuf,
    pub studio_home: PathBuf,
    pub artifact_dir: PathBuf,
    pub thread_id: String,
    route_diagnostics: String,
    toolchain_diagnostics: String,
    installed_config: InstalledConfigGuard,
    _root: TempRoot,
}

impl LiveTaskFixture {
    pub async fn new() -> Result<Self> {
        Self::new_with_mode(StudioMode::Task, "Live multi-workstream Rust task", true).await
    }

    /// `require_node` 为 false 时用于不依赖 Node.js 验收的 Simple 模式 live 流程。
    pub async fn new_with_mode(
        mode: StudioMode,
        thread_title: &str,
        require_node: bool,
    ) -> Result<Self> {
        let installed_config = InstalledConfigGuard::load()?;
        let config = &installed_config.config;
        let route_diagnostics = StudioRole::all()
            .into_iter()
            .map(|role| {
                let route = config.resolve_role(role)?;
                Ok(format!(
                    "{}: provider={}, model={}, connection={:?}",
                    role.key(),
                    route.provider_id,
                    route.model.slug,
                    route.model.transport.default_connection_mode
                ))
            })
            .collect::<Result<Vec<_>>>()?
            .join("\n");
        let toolchain_diagnostics = if require_node {
            command_output(None, "cargo", &["--version"])
                .context("Cargo is required before starting the live model test")?
        } else {
            "not required".to_string()
        };

        let root = TempRoot::new("pure-task-live-integration")?;
        let studio_home = root.path.join("home");
        let workspace = root.path.join("workspace");
        tokio::fs::create_dir_all(&studio_home).await?;
        tokio::fs::write(
            studio_home.join("config.toml"),
            &installed_config.runtime_bytes,
        )
        .await?;
        tokio::fs::create_dir_all(&workspace).await?;
        if mode == StudioMode::Task {
            copy_directory(&live_fixture_workspace(), &workspace)?;
        } else {
            tokio::fs::write(
                workspace.join("README.md"),
                "# Live Task Fixture\n\nBuild the requested project in this repository.\n",
            )
            .await?;
            tokio::fs::write(workspace.join(".gitignore"), ".pure/\ntarget/\n").await?;
        }
        git_output(&workspace, &["init", "--initial-branch=main"])?;
        git_output(&workspace, &["add", "."])?;
        git_output(
            &workspace,
            &[
                "-c",
                "user.name=Pure Studio",
                "-c",
                "user.email=pure-studio@local",
                "commit",
                "-m",
                "test: initialize temporary live Task project",
            ],
        )?;

        let runtime = StudioRuntime::with_options(StudioRuntimeOptions {
            studio_home: Some(studio_home.clone()),
            // Live acceptance must exercise the same credential boundary as the
            // native application. The Test host intentionally uses an in-memory
            // credential store and would silently bypass installed system keys.
            host: StudioHostKind::Desktop,
        })
        .await
        .map_err(anyhow::Error::new)?;
        runtime.start_runtime().await?;
        let store = StudioStore::open(studio_home.join("studio/studio.sqlite")).await?;
        let project = runtime.open_project(&workspace).await?;
        let session = runtime.create_thread(&project.id, thread_title).await?;
        runtime.set_thread_mode(&session.id, mode).await?;

        eprintln!("live model routes:\n{route_diagnostics}");
        let artifact_dir = task_artifact_dir()?;
        std::fs::create_dir_all(&artifact_dir).with_context(|| {
            format!(
                "failed to create Task artifact directory `{}`",
                artifact_dir.display()
            )
        })?;
        let prompt = LIVE_TASK_PROMPT.as_bytes();
        std::fs::write(artifact_dir.join("fixture-prompt.md"), prompt)?;
        std::fs::write(
            artifact_dir.join("fixture-prompt.sha256"),
            format!("{:x}\n", Sha256::digest(prompt)),
        )?;
        std::fs::write(
            artifact_dir.join("model-routes.txt"),
            format!("{route_diagnostics}\n"),
        )?;
        eprintln!("toolchain: {toolchain_diagnostics}");
        eprintln!("Task artifacts: {}", artifact_dir.display());

        Ok(Self {
            runtime,
            store,
            workspace: workspace.clone(),
            studio_home,
            artifact_dir,
            thread_id: session.id,
            route_diagnostics,
            toolchain_diagnostics,
            installed_config,
            _root: root,
        })
    }

    pub async fn wait_for_plan_confirmation(&self) -> Result<InteractionRequest> {
        let mut idle_since = None;
        loop {
            let thread = self.runtime.thread_snapshot(&self.thread_id).await?;
            let pending = thread
                .interactions
                .into_iter()
                .filter(|interaction| interaction.status() == InteractionStatus::Pending)
                .collect::<Vec<_>>();
            if let Some(unexpected) = pending
                .iter()
                .find(|interaction| interaction.kind() != InteractionKind::PlanConfirmation)
            {
                bail!(
                    "unexpected interaction before plan confirmation: {:?}\n{}",
                    unexpected.kind(),
                    self.diagnostics().await
                );
            }
            if let Some(confirmation) = pending
                .into_iter()
                .find(|interaction| interaction.kind() == InteractionKind::PlanConfirmation)
            {
                return Ok(confirmation);
            }
            if let Some(task) = self.runtime.thread_task_view(&self.thread_id).await?
                && is_live_failure(&task.state)
            {
                bail!(
                    "Task entered blocked or terminal state `{}` before plan confirmation\n{}",
                    task_state_name(&task.state),
                    self.diagnostics().await
                );
            }

            if self
                .runtime
                .runtime_snapshot()
                .await?
                .active_turns
                .is_empty()
            {
                let idle_started = idle_since.get_or_insert_with(Instant::now);
                if idle_started.elapsed() >= IDLE_FAILURE_GRACE {
                    bail!(
                        "planner stopped without requesting plan confirmation\n{}",
                        self.diagnostics().await
                    );
                }
            } else {
                idle_since = None;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    pub async fn wait_for_completed_task(&self) -> Result<StudioTaskRuntime> {
        let mut idle_since = None;
        loop {
            let thread = self.runtime.thread_snapshot(&self.thread_id).await?;
            let pending = thread
                .interactions
                .into_iter()
                .filter(|interaction| interaction.status() == InteractionStatus::Pending)
                .collect::<Vec<_>>();
            if let Some(interaction) = pending.first() {
                bail!(
                    "unexpected pending interaction while Task was running: {:?}\n{}",
                    interaction.kind(),
                    self.diagnostics().await
                );
            }

            let task = self.runtime.thread_task_view(&self.thread_id).await?;
            if let Some(task) = &task {
                if matches!(&task.state, StudioTaskState::Completed(_)) {
                    return Ok(task.clone());
                }
                if is_live_failure(&task.state) {
                    bail!(
                        "Task entered blocked or terminal state `{}`: {}\n{}",
                        task_state_name(&task.state),
                        task_state_message(&task.state).unwrap_or("no status message"),
                        self.diagnostics().await
                    );
                }
            }
            if self
                .runtime
                .runtime_snapshot()
                .await?
                .active_turns
                .is_empty()
            {
                let idle_started = idle_since.get_or_insert_with(Instant::now);
                if idle_started.elapsed() >= IDLE_FAILURE_GRACE {
                    bail!(
                        "Task stopped making progress with no active turn\n{}",
                        self.diagnostics().await
                    );
                }
            } else {
                idle_since = None;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    pub async fn wait_for_no_active_turns(&self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if self
                .runtime
                .runtime_snapshot()
                .await?
                .active_turns
                .is_empty()
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!(
                    "Studio runtime retained an active turn\n{}",
                    self.diagnostics().await
                );
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    pub async fn successful_executor_scope_hints(&self) -> Result<Vec<Vec<String>>> {
        let snapshot = self.runtime.thread_snapshot(&self.thread_id).await?;
        snapshot
            .items
            .into_iter()
            .filter_map(|item| {
                let tool = item.tool()?;
                if tool.invocation().name() != "task_spawn_executor"
                    || !matches!(tool.state(), pl_protocol::ThreadToolState::Succeeded(_))
                {
                    return None;
                }
                Some((
                    tool.invocation().arguments().to_owned(),
                    tool.terminal_output()?.result().to_owned(),
                ))
            })
            .filter_map(|(arguments, result)| {
                match successful_executor_scope_hints(&arguments, &result) {
                    Ok(Some(scope_hints)) => Some(Ok(scope_hints)),
                    Ok(None) => None,
                    Err(error) => Some(Err(error)),
                }
            })
            .collect()
    }

    pub async fn successful_task_record_merge_arguments(&self) -> Result<Vec<serde_json::Value>> {
        let snapshot = self.runtime.thread_snapshot(&self.thread_id).await?;
        snapshot
            .items
            .into_iter()
            .filter_map(|item| {
                let tool = item.tool()?;
                if tool.invocation().name() != "task_record_merge"
                    || !matches!(tool.state(), pl_protocol::ThreadToolState::Succeeded(_))
                {
                    return None;
                }
                Some((
                    tool.invocation().arguments().to_owned(),
                    tool.terminal_output()?.result().to_owned(),
                ))
            })
            .map(|(arguments, result)| {
                let result: serde_json::Value = serde_json::from_str(&result)
                    .context("successful task_record_merge result is not JSON")?;
                if result
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .is_none()
                {
                    bail!("task_record_merge result does not contain a merge record id");
                }
                serde_json::from_str(&arguments).context("task_record_merge arguments are not JSON")
            })
            .collect()
    }

    pub async fn diagnostics(&self) -> String {
        let task = self
            .runtime
            .thread_task_view(&self.thread_id)
            .await
            .map(|task| format!("{task:#?}"))
            .unwrap_or_else(|error| format!("task projection failed: {error:#}"));
        let snapshot = self.runtime.thread_snapshot(&self.thread_id).await;
        let interactions = snapshot
            .as_ref()
            .map(|snapshot| {
                let pending = snapshot
                    .interactions
                    .iter()
                    .filter(|interaction| interaction.status() == InteractionStatus::Pending)
                    .collect::<Vec<_>>();
                format!("{pending:#?}")
            })
            .unwrap_or_else(|error| format!("hot interaction projection failed: {error:#}"));
        let snapshot_summary = snapshot
            .as_ref()
            .map(|snapshot| {
                format!(
                    "revision={} active_turn={:#?}",
                    snapshot.revision, snapshot.active_turn
                )
            })
            .unwrap_or_else(|error| format!("thread snapshot query failed: {error:#}"));
        let items = snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .items
                    .iter()
                    .rev()
                    .take(20)
                    .map(compact_item_diagnostic)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_else(|error| format!("thread item query failed: {error:#}"));
        let runtime = format!("{:#?}", self.runtime.runtime_snapshot().await);
        let git = if self.workspace.join(".git").exists() {
            git_output(&self.workspace, &["status", "--porcelain"])
                .unwrap_or_else(|error| format!("git status failed: {error:#}"))
        } else {
            "repository not initialized".to_string()
        };
        format!(
            "model routes:\n{}\ntoolchain: {}\ntask projection:\n{task}\n\
             pending interactions:\n{interactions}\nruntime snapshot:\n{runtime}\n\
             thread snapshot:\n{snapshot_summary}\nrecent items:\n{items}\ngit status:\n{git}",
            self.route_diagnostics, self.toolchain_diagnostics
        )
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.runtime.shutdown_runtime().await?;
        Ok(())
    }

    pub fn assert_config_unchanged(&self) -> Result<()> {
        self.installed_config.assert_unchanged()
    }
}

fn successful_executor_scope_hints(arguments: &str, result: &str) -> Result<Option<Vec<String>>> {
    let Ok(result) = serde_json::from_str::<serde_json::Value>(result) else {
        return Ok(None);
    };
    if result
        .get("agentId")
        .and_then(serde_json::Value::as_str)
        .is_none()
    {
        return Ok(None);
    }
    let arguments: serde_json::Value = serde_json::from_str(arguments)
        .context("successful task_spawn_executor arguments are not JSON")?;
    let Some(scope_hints) = arguments
        .get("scope")
        .and_then(|scope| scope.get("scopeHints"))
    else {
        return Ok(Some(Vec::new()));
    };
    let scope_hints = scope_hints
        .as_array()
        .context("task_spawn_executor scopeHints is not an array")?
        .iter()
        .map(|path| {
            path.as_str()
                .map(ToOwned::to_owned)
                .context("task_spawn_executor scopeHints contains a non-string value")
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(scope_hints))
}

struct InstalledConfigGuard {
    path: PathBuf,
    original_bytes: Option<Vec<u8>>,
    runtime_bytes: Vec<u8>,
    config: StudioConfig,
}

impl InstalledConfigGuard {
    fn load() -> Result<Self> {
        let store = ConfigStore::default_app()?;
        let path = store.paths().config_file().to_path_buf();
        let (mut config, original_bytes) = if store.config_exists() {
            let bytes = std::fs::read(&path)
                .with_context(|| format!("failed to read installed config `{}`", path.display()))?;
            let config = load_isolated_installed_config(&path, &bytes)?;
            (config, Some(bytes))
        } else {
            let mut config = StudioConfig::default_config();
            let default_provider = config
                .models
                .providers
                .iter_mut()
                .find_map(|(id, provider)| (id.as_str() == "deepseek").then_some(provider))
                .context("default Studio config has no deepseek provider")?;
            default_provider.bearer_token_env = Some("DEEPSEEK_API_KEY".to_string());
            if !config
                .models
                .providers
                .values()
                .any(|provider| provider.resolved_bearer_token().is_some())
            {
                bail!(
                    "installed Studio config is missing at `{}` and the default provider environment credential is unavailable",
                    path.display()
                );
            }
            (config, None)
        };
        validate_real_task_routes(&config)?;
        append_acceptance_context(
            &mut config.instructions.developer,
            "TASK_LIVE_GLOBAL_DEVELOPER_CONTEXT: preserve the complete Task acceptance contract.",
        );
        append_acceptance_context(
            &mut config.instructions.user,
            "TASK_LIVE_GLOBAL_USER_CONTEXT: execute the canonical fixture prompt exactly.",
        );
        let mut persisted_config = config.clone();
        for provider in persisted_config.models.providers.values_mut() {
            provider.bearer_token = None;
        }
        let runtime_bytes = toml::to_string_pretty(&persisted_config)
            .context("failed to serialize isolated live Studio config")?
            .into_bytes();
        Ok(Self {
            path,
            original_bytes,
            runtime_bytes,
            config,
        })
    }

    fn assert_unchanged(&self) -> Result<()> {
        match &self.original_bytes {
            Some(original) => {
                let current = std::fs::read(&self.path).with_context(|| {
                    format!(
                        "failed to reread installed config `{}`",
                        self.path.display()
                    )
                })?;
                if &current != original {
                    bail!(
                        "live Task test modified installed Studio config `{}`",
                        self.path.display()
                    );
                }
            }
            None if self.path.exists() => {
                bail!(
                    "live Task test unexpectedly created installed Studio config `{}`",
                    self.path.display()
                );
            }
            None => {}
        }
        Ok(())
    }
}

fn load_isolated_installed_config(path: &Path, bytes: &[u8]) -> Result<StudioConfig> {
    let source = std::str::from_utf8(bytes)
        .with_context(|| format!("installed Studio config `{}` is not UTF-8", path.display()))?;
    let mut document = source
        .parse::<toml::Table>()
        .with_context(|| format!("installed Studio config `{}` is invalid", path.display()))?;
    let schema_version = document
        .get("schema_version")
        .and_then(toml::Value::as_integer)
        .context("installed Studio config has no integer schema_version")?;
    let current_schema = i64::from(STUDIO_CONFIG_SCHEMA_VERSION);
    if schema_version != current_schema {
        anyhow::ensure!(
            schema_version.checked_add(1) == Some(current_schema),
            "installed Studio config schema {schema_version} cannot be upgraded in the live acceptance copy to schema {current_schema}"
        );
        document.insert(
            "schema_version".to_string(),
            toml::Value::Integer(current_schema),
        );
    }

    let isolated = tempfile::tempdir().context("failed to create config validation directory")?;
    std::fs::write(
        isolated.path().join("config.toml"),
        toml::to_string_pretty(&document)?,
    )?;
    ConfigStore::for_studio_home(isolated.path())
        .load()
        .with_context(|| format!("installed Studio config `{}` is invalid", path.display()))
}

fn validate_real_task_routes(config: &StudioConfig) -> Result<()> {
    for role in [
        StudioRole::Planner,
        StudioRole::Executor,
        StudioRole::Reviewer,
    ] {
        let route = config.resolve_role(role)?;
        let base_url = route.endpoint.base_url.to_ascii_lowercase();
        if ["localhost", "127.0.0.1", "[::1]", "0.0.0.0"]
            .iter()
            .any(|host| base_url.contains(host))
        {
            bail!(
                "live Task route {} points to a local/scripted endpoint `{}`",
                role.key(),
                route.endpoint.base_url
            );
        }
        if route.endpoint.bearer_token.is_none() {
            bail!(
                "live Task route {} ({}/{}) has no resolved API credential",
                role.key(),
                route.provider_id,
                route.model.slug
            );
        }
    }
    Ok(())
}

fn append_acceptance_context(target: &mut String, marker: &str) {
    if !target.trim().is_empty() {
        target.push_str("\n\n");
    }
    target.push_str(marker);
}

impl Drop for InstalledConfigGuard {
    fn drop(&mut self) {
        let unchanged = match &self.original_bytes {
            Some(original) => std::fs::read(&self.path)
                .map(|current| current == *original)
                .unwrap_or(false),
            None => !self.path.exists(),
        };
        if !unchanged {
            eprintln!(
                "ERROR: installed Studio config changed during live Task test: {}",
                self.path.display()
            );
            if !std::thread::panicking() {
                panic!("installed Studio config changed during live Task test");
            }
        }
    }
}

fn live_fixture_workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test-fixtures/task-live/workspace")
}

fn task_artifact_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("PURE_STUDIO_TASK_ARTIFACT_DIR") {
        return Ok(PathBuf::from(path));
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("target/task-live-artifacts")
        .join(format!("headless-{}-{stamp}", std::process::id())))
}

fn copy_directory(source: &Path, target: &Path) -> Result<()> {
    for entry in std::fs::read_dir(source)
        .with_context(|| format!("failed to read fixture directory `{}`", source.display()))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&target_path)?;
            copy_directory(&source_path, &target_path)?;
        } else {
            std::fs::copy(&source_path, &target_path).with_context(|| {
                format!(
                    "failed to copy fixture file `{}` to `{}`",
                    source_path.display(),
                    target_path.display()
                )
            })?;
        }
    }
    Ok(())
}

struct TempRoot {
    path: PathBuf,
}

impl TempRoot {
    fn new(label: &str) -> Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{label}-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&path)
            .with_context(|| format!("failed to create temporary root `{}`", path.display()))?;
        Ok(Self { path })
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        if self.path.starts_with(std::env::temp_dir()) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn task_state_name(state: &StudioTaskState) -> &'static str {
    match state {
        StudioTaskState::Planning(_) => "planning",
        StudioTaskState::PendingConfirmation(_) => "pendingConfirmation",
        StudioTaskState::EditingDocuments(_) => "editingDocuments",
        StudioTaskState::Working(_) => "working",
        StudioTaskState::Reviewing(_) => "reviewing",
        StudioTaskState::Completed(_) => "completed",
    }
}

fn task_state_message(state: &StudioTaskState) -> Option<&str> {
    match state {
        StudioTaskState::Planning(state) => Some(&state.request),
        StudioTaskState::Working(state) => Some(&state.document_edit_summary),
        StudioTaskState::Reviewing(state) => Some(&state.target.reviewed_head),
        StudioTaskState::Completed(state) => match &state.outcome {
            pl_studio_runtime::StudioTaskOutcome::Succeeded { summary, .. }
            | pl_studio_runtime::StudioTaskOutcome::Failed { summary, .. } => Some(summary),
        },
        StudioTaskState::PendingConfirmation(_) | StudioTaskState::EditingDocuments(_) => None,
    }
}

fn is_live_failure(state: &StudioTaskState) -> bool {
    matches!(
        state,
        StudioTaskState::Completed(pl_studio_runtime::StudioCompletedTaskState {
            outcome: pl_studio_runtime::StudioTaskOutcome::Failed { .. }
        })
    )
}

fn compact_item_diagnostic(item: &pl_protocol::ThreadItem) -> String {
    let content = if let Some(tool) = item.tool() {
        format!(
            "tool={} arguments={} result={}",
            tool.invocation().name(),
            truncate_diagnostic(tool.invocation().arguments(), 800),
            truncate_diagnostic(
                tool.terminal_output()
                    .map(pl_protocol::ThreadToolOutput::result)
                    .unwrap_or(""),
                1_200,
            )
        )
    } else if let Some(text) = item.text() {
        format!(
            "text channel={:?} value={}",
            text.channel(),
            truncate_diagnostic(text.text(), 1_200)
        )
    } else {
        truncate_diagnostic(&format!("{:?}", item.state()), 1_200)
    };
    format!(
        "item={} turn={} kind={:?} failure={:?} {content}",
        item.id,
        item.turn_id,
        item.kind(),
        item.failure()
    )
}

fn truncate_diagnostic(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let prefix = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

pub fn command_output(cwd: Option<&Path>, program: &str, args: &[&str]) -> Result<String> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command
        .output()
        .with_context(|| format!("failed to execute `{program}`"))?;
    if !output.status.success() {
        bail!(
            "`{program} {}` failed with {}\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::successful_executor_scope_hints;

    #[test]
    fn rejected_executor_spawn_is_not_collected_as_a_success() {
        assert_eq!(
            successful_executor_scope_hints("{}", "executor blueprint is invalid").unwrap(),
            None
        );
        assert_eq!(
            successful_executor_scope_hints(
                r#"{"scope":{"scopeHints":["game-core.mjs"]}}"#,
                r#"{"agentId":"executor-1"}"#,
            )
            .unwrap(),
            Some(vec!["game-core.mjs".to_string()])
        );
    }
}
