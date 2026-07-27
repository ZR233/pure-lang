use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use pl_studio_runtime::{
    ConfigStore, InteractionKind, InteractionRequest, StudioMode, StudioRole, StudioRuntime,
    StudioStore, StudioTaskRuntime,
};

use super::git::git_output;

pub const LIVE_VERIFY_MARKER: &str = "PURE_SHOOTER_VERIFY_OK";

const POLL_INTERVAL: Duration = Duration::from_millis(100);
const IDLE_FAILURE_GRACE: Duration = Duration::from_secs(10);

pub struct LiveTaskFixture {
    pub runtime: StudioRuntime,
    pub store: StudioStore,
    pub workspace: PathBuf,
    pub session_id: String,
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
                    route.provider_info.connection_mode
                ))
            })
            .collect::<Result<Vec<_>>>()?
            .join("\n");
        let node_version = command_output(None, "node", &["--version"])
            .context("Node.js is required before starting the live model test")?;

        let root = TempRoot::new("pure-task-live-integration")?;
        let workspace = root.path.join("workspace");
        tokio::fs::create_dir_all(&workspace).await?;
        tokio::fs::write(
            workspace.join("README.md"),
            "# Live Task Fixture\n\nBuild the requested project in this repository.\n",
        )
        .await?;

        let store = StudioStore::open_memory().await?;
        let runtime = StudioRuntime::new(store.clone(), installed_config.store.clone());
        let project = runtime.open_project(&workspace).await?;
        let session = runtime
            .create_session(&project.id, "Live headless shooter task")
            .await?;
        runtime
            .set_session_mode(&session.id, StudioMode::Task)
            .await?;
        runtime.start_runtime().await?;

        eprintln!("live Task model routes:\n{route_diagnostics}");
        eprintln!("Node.js: {node_version}");

        Ok(Self {
            runtime,
            store,
            workspace,
            session_id: session.id,
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
                .list_pending_interactions(&self.session_id)
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
            if let Some(task) = self.runtime.session_task_view(&self.session_id).await?
                && is_failed_phase(&task.phase)
            {
                bail!(
                    "Task entered terminal phase `{}` before plan confirmation\n{}",
                    task.phase,
                    self.diagnostics().await
                );
            }

            if self.runtime.runtime_snapshot().active_turns.is_empty() {
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
                .list_pending_interactions(&self.session_id)
                .await?;
            if let Some(interaction) = pending.first() {
                bail!(
                    "unexpected pending interaction while Task was running: {:?}\n{}",
                    interaction.kind,
                    self.diagnostics().await
                );
            }

            let mut child_is_active = false;
            if let Some(task) = self.runtime.session_task_view(&self.session_id).await? {
                if task.phase == "completed" {
                    return Ok(task);
                }
                if is_failed_phase(&task.phase) {
                    bail!(
                        "Task entered terminal phase `{}`: {}\n{}",
                        task.phase,
                        task.status_message
                            .as_deref()
                            .unwrap_or("no status message"),
                        self.diagnostics().await
                    );
                }
                child_is_active = task.agents.iter().any(|agent| {
                    matches!(
                        agent.status.as_str(),
                        "queued" | "running" | "waitingForDelivery"
                    )
                });
            }

            if self.runtime.runtime_snapshot().active_turns.is_empty() && !child_is_active {
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

    pub async fn wait_for_running_executor(&self) -> Result<String> {
        let deadline = Instant::now() + Duration::from_secs(5 * 60);
        loop {
            if let Some(task) = self.runtime.session_task_view(&self.session_id).await? {
                if let Some(executor) = task
                    .agents
                    .iter()
                    .find(|agent| agent.role == "executor" && agent.status == "running")
                {
                    return Ok(executor.agent_id.clone());
                }
                if is_failed_phase(&task.phase) {
                    bail!(
                        "Task entered terminal phase `{}` before an executor was running\n{}",
                        task.phase,
                        self.diagnostics().await
                    );
                }
            }
            if Instant::now() >= deadline {
                bail!(
                    "Task did not expose a running executor before timeout\n{}",
                    self.diagnostics().await
                );
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    pub async fn wait_for_no_active_turns(&self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if self.runtime.runtime_snapshot().active_turns.is_empty() {
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

    pub async fn successful_executor_owned_paths(&self) -> Result<Vec<Vec<String>>> {
        let snapshot = self
            .runtime
            .session_event_snapshot(&self.session_id)
            .await?;
        snapshot
            .parts
            .into_iter()
            .filter_map(|part| match part.content {
                pl_studio_runtime::SessionPartContent::Tool { tool }
                    if tool.name == "task_spawn_executor" && tool.result.is_some() =>
                {
                    Some((tool.arguments, tool.result.expect("checked above")))
                }
                _ => None,
            })
            .map(|(arguments, result)| {
                let result: serde_json::Value = serde_json::from_str(&result)
                    .context("successful task_spawn_executor result is not JSON")?;
                if result
                    .get("agentId")
                    .and_then(serde_json::Value::as_str)
                    .is_none()
                {
                    bail!("task_spawn_executor result does not contain agentId");
                }
                let arguments: serde_json::Value = serde_json::from_str(&arguments)
                    .context("task_spawn_executor arguments are not JSON")?;
                arguments
                    .get("ownedPaths")
                    .and_then(serde_json::Value::as_array)
                    .context("task_spawn_executor arguments do not contain ownedPaths")?
                    .iter()
                    .map(|path| {
                        path.as_str()
                            .map(ToOwned::to_owned)
                            .context("task_spawn_executor ownedPaths contains a non-string value")
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect()
    }

    pub async fn wait_for_successful_interrupt_target(&self) -> Result<String> {
        let deadline = Instant::now() + Duration::from_secs(5 * 60);
        loop {
            let snapshot = self
                .runtime
                .session_event_snapshot(&self.session_id)
                .await?;
            if let Some(tool) = snapshot
                .parts
                .into_iter()
                .find_map(|part| match part.content {
                    pl_studio_runtime::SessionPartContent::Tool { tool }
                        if part.status == pl_studio_runtime::SessionPartStatus::Completed
                            && part.error.is_none()
                            && tool.name == "send_input"
                            && tool.result.is_some() =>
                    {
                        Some(tool)
                    }
                    _ => None,
                })
            {
                let arguments: serde_json::Value = serde_json::from_str(&tool.arguments)
                    .context("headless shooter send_input arguments are not JSON")?;
                if arguments
                    .get("delivery")
                    .and_then(serde_json::Value::as_str)
                    != Some("interruptThenStart")
                {
                    bail!(
                        "headless shooter send_input did not use interruptThenStart: {}",
                        tool.arguments
                    );
                }
                return arguments
                    .get("target")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
                    .context("headless shooter send_input arguments do not contain a target");
            }
            if Instant::now() >= deadline {
                bail!(
                    "headless shooter did not record a successful send_input call before timeout\n{}",
                    self.diagnostics().await
                );
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    pub async fn diagnostics(&self) -> String {
        let task = self
            .runtime
            .session_task_view(&self.session_id)
            .await
            .map(|task| format!("{task:#?}"))
            .unwrap_or_else(|error| format!("task projection failed: {error:#}"));
        let interactions = self
            .store
            .list_pending_interactions(&self.session_id)
            .await
            .map(|interactions| format!("{interactions:#?}"))
            .unwrap_or_else(|error| format!("pending interaction query failed: {error:#}"));
        let snapshot = self.runtime.session_event_snapshot(&self.session_id).await;
        let events = snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .timeline_events
                    .iter()
                    .rev()
                    .take(20)
                    .map(|event| format!("{event:#?}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_else(|error| format!("session event query failed: {error:#}"));
        let parts = snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .parts
                    .iter()
                    .rev()
                    .take(20)
                    .map(compact_part_diagnostic)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_else(|error| format!("session projection query failed: {error:#}"));
        let runtime = format!("{:#?}", self.runtime.runtime_snapshot());
        let git = if self.workspace.join(".git").exists() {
            git_output(&self.workspace, &["status", "--porcelain"])
                .unwrap_or_else(|error| format!("git status failed: {error:#}"))
        } else {
            "repository not initialized".to_string()
        };
        format!(
            "model routes:\n{}\nNode.js: {}\ntask projection:\n{task}\n\
             pending interactions:\n{interactions}\nruntime snapshot:\n{runtime}\n\
             recent events:\n{events}\nrecent session parts:\n{parts}\ngit status:\n{git}",
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

fn is_failed_phase(phase: &str) -> bool {
    matches!(phase, "blocked" | "failed" | "cancelled")
}

fn compact_part_diagnostic(part: &pl_studio_runtime::SessionPart) -> String {
    let content = match &part.content {
        pl_studio_runtime::SessionPartContent::Tool { tool } => format!(
            "tool={} arguments={} result={}",
            tool.name,
            truncate_diagnostic(&tool.arguments, 800),
            truncate_diagnostic(tool.result.as_deref().unwrap_or(""), 1_200)
        ),
        pl_studio_runtime::SessionPartContent::Text { channel, text, .. } => {
            format!(
                "text channel={channel:?} value={}",
                truncate_diagnostic(text, 1_200)
            )
        }
        content => truncate_diagnostic(&format!("{content:?}"), 1_200),
    };
    format!(
        "part={} turn={} status={:?} error={:?} {content}",
        part.part_id, part.turn_id, part.status, part.error
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
