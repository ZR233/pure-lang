use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use pl_studio_runtime::{
    ConfigStore, InteractionKind, InteractionRequest, StudioHostKind, StudioMode, StudioRole,
    StudioRuntime, StudioRuntimeOptions, StudioStore, StudioTaskRuntime, StudioTaskState,
};

use super::git::git_output;

pub const LIVE_VERIFY_MARKER: &str = "PURE_SHOOTER_VERIFY_OK";

const POLL_INTERVAL: Duration = Duration::from_millis(100);
const IDLE_FAILURE_GRACE: Duration = Duration::from_secs(10);

pub struct LiveTaskFixture {
    pub runtime: StudioRuntime,
    pub store: StudioStore,
    pub workspace: PathBuf,
    pub thread_id: String,
    project_id: String,
    route_diagnostics: String,
    node_version: String,
    installed_config: InstalledConfigGuard,
    _root: TempRoot,
}

impl LiveTaskFixture {
    pub async fn new() -> Result<Self> {
        let installed_config = InstalledConfigGuard::load()?;
        let config = installed_config.store.load().with_context(|| {
            format!(
                "installed Studio config `{}` is invalid",
                installed_config.path.display()
            )
        })?;
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
        let node_version = command_output(None, "node", &["--version"])
            .context("Node.js is required before starting the live model test")?;

        let root = TempRoot::new("pure-task-live-integration")?;
        let studio_home = root.path.join("home");
        let workspace = root.path.join("workspace");
        tokio::fs::create_dir_all(&studio_home).await?;
        tokio::fs::write(
            studio_home.join("config.toml"),
            &installed_config.original_bytes,
        )
        .await?;
        tokio::fs::create_dir_all(&workspace).await?;
        tokio::fs::write(
            workspace.join("README.md"),
            "# Live Task Fixture\n\nBuild the requested project in this repository.\n",
        )
        .await?;

        let runtime = StudioRuntime::with_options(StudioRuntimeOptions {
            studio_home: Some(studio_home.clone()),
            host: StudioHostKind::Test,
        })
        .await
        .map_err(anyhow::Error::new)?;
        runtime.start_runtime().await?;
        let store = StudioStore::open(studio_home.join("studio/studio.sqlite")).await?;
        let project = runtime.open_project(&workspace).await?;
        let session = runtime
            .create_thread(&project.id, "Live headless shooter task")
            .await?;
        runtime
            .set_thread_mode(&session.id, StudioMode::Task)
            .await?;

        eprintln!("live Task model routes:\n{route_diagnostics}");
        eprintln!("Node.js: {node_version}");

        Ok(Self {
            runtime,
            store,
            workspace,
            thread_id: session.id,
            project_id: project.id,
            route_diagnostics,
            node_version,
            installed_config,
            _root: root,
        })
    }

    pub async fn wait_for_plan_confirmation(&self) -> Result<InteractionRequest> {
        let mut idle_since = None;
        loop {
            let pending = self
                .store
                .list_pending_interactions(&self.thread_id)
                .await?;
            if let Some(unexpected) = pending
                .iter()
                .find(|interaction| interaction.kind != InteractionKind::PlanConfirmation)
            {
                bail!(
                    "unexpected interaction before plan confirmation: {:?}\n{}",
                    unexpected.kind,
                    self.diagnostics().await
                );
            }
            if let Some(confirmation) = pending
                .into_iter()
                .find(|interaction| interaction.kind == InteractionKind::PlanConfirmation)
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
            let pending = self
                .store
                .list_pending_interactions(&self.thread_id)
                .await?;
            if let Some(interaction) = pending.first() {
                bail!(
                    "unexpected pending interaction while Task was running: {:?}\n{}",
                    interaction.kind,
                    self.diagnostics().await
                );
            }

            if let Some(task) = self.runtime.thread_task_view(&self.thread_id).await? {
                if matches!(&task.state, StudioTaskState::Completed(_)) {
                    return Ok(task);
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
            let child_is_active =
                self.store
                    .list_threads(&self.project_id)
                    .await?
                    .iter()
                    .any(|thread| {
                        thread.root_thread_id == self.thread_id
                            && thread.parent_thread_id.is_some()
                            && matches!(thread.status.as_str(), "queued" | "running" | "waiting")
                    });

            if self
                .runtime
                .runtime_snapshot()
                .await?
                .active_turns
                .is_empty()
                && !child_is_active
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
            .filter_map(|item| match item.content {
                pl_protocol::ThreadItemContent::ToolCall { tool }
                    if tool.name == "task_spawn_executor" =>
                {
                    tool.result.map(|result| (tool.arguments, result))
                }
                _ => None,
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
            .filter_map(|item| match item.content {
                pl_protocol::ThreadItemContent::ToolCall { tool }
                    if tool.name == "task_record_merge" =>
                {
                    tool.result.map(|result| (tool.arguments, result))
                }
                _ => None,
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
        let interactions = self
            .store
            .list_pending_interactions(&self.thread_id)
            .await
            .map(|interactions| format!("{interactions:#?}"))
            .unwrap_or_else(|error| format!("pending interaction query failed: {error:#}"));
        let snapshot = self.runtime.thread_snapshot(&self.thread_id).await;
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
            "model routes:\n{}\nNode.js: {}\ntask projection:\n{task}\n\
             pending interactions:\n{interactions}\nruntime snapshot:\n{runtime}\n\
             thread snapshot:\n{snapshot_summary}\nrecent items:\n{items}\ngit status:\n{git}",
            self.route_diagnostics, self.node_version
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
    store: ConfigStore,
    path: PathBuf,
    original_bytes: Vec<u8>,
}

impl InstalledConfigGuard {
    fn load() -> Result<Self> {
        let store = ConfigStore::default_app()?;
        if !store.config_exists() {
            bail!(
                "installed Studio config is missing at `{}`",
                store.paths().config_file().display()
            );
        }
        let path = store.paths().config_file().to_path_buf();
        let original_bytes = std::fs::read(&path)
            .with_context(|| format!("failed to read installed config `{}`", path.display()))?;
        Ok(Self {
            store,
            path,
            original_bytes,
        })
    }

    fn assert_unchanged(&self) -> Result<()> {
        let current = std::fs::read(&self.path).with_context(|| {
            format!(
                "failed to reread installed config `{}`",
                self.path.display()
            )
        })?;
        if current != self.original_bytes {
            bail!(
                "live Task test modified installed Studio config `{}`",
                self.path.display()
            );
        }
        Ok(())
    }
}

impl Drop for InstalledConfigGuard {
    fn drop(&mut self) {
        let unchanged = std::fs::read(&self.path)
            .map(|current| current == self.original_bytes)
            .unwrap_or(false);
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
        StudioTaskState::DesignUpdating(_) => "designUpdating",
        StudioTaskState::Implementing(_) => "implementing",
        StudioTaskState::Merging(_) => "merging",
        StudioTaskState::Reviewing(_) => "reviewing",
        StudioTaskState::Reworking(_) => "reworking",
        StudioTaskState::Stopping(_) => "stopping",
        StudioTaskState::Blocked(_) => "blocked",
        StudioTaskState::Completed(_) => "completed",
        StudioTaskState::Failed(_) => "failed",
        StudioTaskState::Cancelled(_) => "cancelled",
    }
}

fn task_state_message(state: &StudioTaskState) -> Option<&str> {
    match state {
        StudioTaskState::Merging(state) => state.status_message.as_deref(),
        StudioTaskState::Reviewing(state) => state.status_message.as_deref(),
        StudioTaskState::Reworking(state) => Some(&state.status_message),
        StudioTaskState::Stopping(state) => Some(&state.status_message),
        StudioTaskState::Blocked(state) => Some(&state.message),
        StudioTaskState::Failed(state) => Some(&state.message),
        StudioTaskState::Cancelled(state) => Some(&state.message),
        StudioTaskState::DesignUpdating(_)
        | StudioTaskState::Implementing(_)
        | StudioTaskState::Completed(_) => None,
    }
}

fn is_live_failure(state: &StudioTaskState) -> bool {
    matches!(
        state,
        StudioTaskState::Blocked(_) | StudioTaskState::Failed(_) | StudioTaskState::Cancelled(_)
    )
}

fn compact_item_diagnostic(item: &pl_protocol::ThreadItem) -> String {
    let content = match &item.content {
        pl_protocol::ThreadItemContent::ToolCall { tool } => format!(
            "tool={} arguments={} result={}",
            tool.name,
            truncate_diagnostic(&tool.arguments, 800),
            truncate_diagnostic(tool.result.as_deref().unwrap_or(""), 1_200)
        ),
        pl_protocol::ThreadItemContent::AgentMessage { channel, text } => {
            format!(
                "text channel={channel:?} value={}",
                truncate_diagnostic(text, 1_200)
            )
        }
        content => truncate_diagnostic(&format!("{content:?}"), 1_200),
    };
    format!(
        "item={} turn={} status={:?} error={:?} {content}",
        item.id, item.turn_id, item.status, item.error
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

pub fn normalized_text(path: &Path) -> Result<String> {
    std::fs::read_to_string(path)
        .with_context(|| format!("failed to read `{}`", path.display()))
        .map(|content| content.replace("\r\n", "\n"))
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
