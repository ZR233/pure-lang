use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use pl_studio_runtime::{
    ConfigPaths, ConfigStore, InteractionKind, InteractionRequest, StudioMode, StudioRuntime,
    StudioStore, StudioTaskRuntime,
};
use tokio::net::TcpListener;

use super::config::task_test_config;
use super::git::git_output;
use super::server::{ScriptedModelServer, TaskFlowScenario};

pub const DESIGN_PATH: &str = "design/task-flow.md";
pub const FEATURE_PATH: &str = "src/feature.txt";
pub const FEATURE_CONTENT: &str = "offline integration verified\n";

const TEST_TIMEOUT: Duration = Duration::from_secs(60);

pub struct TaskFlowFixture {
    pub runtime: StudioRuntime,
    pub store: StudioStore,
    pub workspace: PathBuf,
    pub session_id: String,
    root: PathBuf,
    server: ScriptedModelServer,
}

impl TaskFlowFixture {
    pub async fn new() -> Result<Self> {
        Self::new_with_scenario(TaskFlowScenario::HappyPath).await
    }

    pub async fn new_interrupted_executor() -> Result<Self> {
        Self::new_with_scenario(TaskFlowScenario::InterruptedExecutor).await
    }

    async fn new_with_scenario(scenario: TaskFlowScenario) -> Result<Self> {
        let root = unique_temp_path("pure-task-orchestration-integration");
        let home = root.join("home");
        let workspace = root.join("workspace");
        tokio::fs::create_dir_all(&home).await?;
        tokio::fs::create_dir_all(&workspace).await?;
        tokio::fs::write(workspace.join("README.md"), "# Offline Task Fixture\n").await?;

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let base_url = format!("http://{}", listener.local_addr()?);
        let config_store = ConfigStore::new(ConfigPaths::from_home(&home));
        config_store.save(&task_test_config(base_url))?;

        let store = StudioStore::open_memory().await?;
        let runtime = StudioRuntime::new(store.clone(), config_store);
        let project = runtime.open_project(&workspace).await?;
        let session = runtime
            .create_session(&project.id, "Offline task orchestration")
            .await?;
        runtime
            .set_session_mode(&session.id, StudioMode::Task)
            .await?;
        runtime.start_runtime().await?;
        let server =
            ScriptedModelServer::start(listener, runtime.clone(), session.id.clone(), scenario);

        Ok(Self {
            runtime,
            store,
            workspace,
            session_id: session.id,
            root,
            server,
        })
    }

    pub async fn wait_for_plan_confirmation(&self) -> Result<InteractionRequest> {
        let deadline = Instant::now() + TEST_TIMEOUT;
        loop {
            if let Some(interaction) = self
                .store
                .list_pending_interactions(&self.session_id)
                .await?
                .into_iter()
                .find(|interaction| interaction.kind == InteractionKind::PlanConfirmation)
            {
                return Ok(interaction);
            }
            if Instant::now() >= deadline {
                bail!(
                    "plan confirmation did not appear before timeout\n{}",
                    self.diagnostics().await
                );
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    pub async fn wait_for_no_active_turns(&self) -> Result<()> {
        let deadline = Instant::now() + TEST_TIMEOUT;
        loop {
            if self.runtime.runtime_snapshot().active_turns.is_empty() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!(
                    "Studio runtime retained an active turn before timeout\n{}",
                    self.diagnostics().await
                );
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    pub async fn wait_for_completed_task(&self) -> Result<StudioTaskRuntime> {
        let deadline = Instant::now() + TEST_TIMEOUT;
        loop {
            if let Some(task) = self.runtime.session_task_view(&self.session_id).await?
                && task.phase == "completed"
            {
                return Ok(task);
            }
            if Instant::now() >= deadline {
                bail!(
                    "Task run did not complete before timeout\n{}",
                    self.diagnostics().await
                );
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    pub async fn assert_script_complete(&self) -> Result<()> {
        self.server.assert_complete().await
    }

    pub async fn wait_for_interrupted_executor_request(&self) -> Result<()> {
        self.server.wait_for_interrupted_executor_request().await
    }

    pub async fn successful_interrupt_target(&self) -> Result<String> {
        let snapshot = self
            .runtime
            .session_event_snapshot(&self.session_id)
            .await?;
        let tool = snapshot
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
            .context("successful send_input call was not projected")?;
        let arguments: serde_json::Value =
            serde_json::from_str(&tool.arguments).context("send_input arguments are not JSON")?;
        if arguments
            .get("delivery")
            .and_then(serde_json::Value::as_str)
            != Some("interruptThenStart")
        {
            bail!(
                "send_input did not use interruptThenStart: {}",
                tool.arguments
            );
        }
        arguments
            .get("target")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
            .context("send_input arguments do not contain a target")
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.runtime.shutdown_runtime().await?;
        Ok(())
    }

    async fn diagnostics(&self) -> String {
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
        let events = self
            .runtime
            .session_event_snapshot(&self.session_id)
            .await
            .map(|snapshot| {
                snapshot
                    .timeline_events
                    .into_iter()
                    .rev()
                    .take(12)
                    .map(|event| format!("{event:#?}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_else(|error| format!("session event query failed: {error:#}"));
        let session_parts = self
            .runtime
            .session_event_snapshot(&self.session_id)
            .await
            .map(|snapshot| {
                snapshot
                    .parts
                    .into_iter()
                    .rev()
                    .take(20)
                    .map(|part| format!("{part:#?}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_else(|error| format!("session projection query failed: {error:#}"));
        let git = if self.workspace.join(".git").exists() {
            git_output(&self.workspace, &["status", "--porcelain"])
                .unwrap_or_else(|error| format!("git status failed: {error:#}"))
        } else {
            "repository not initialized".to_string()
        };
        let server = self.server.diagnostics().await;
        format!(
            "task projection:\n{task}\npending interactions:\n{interactions}\nrecent events:\n{events}\nrecent session parts:\n{session_parts}\ngit status:\n{git}\n{server}"
        )
    }
}

impl Drop for TaskFlowFixture {
    fn drop(&mut self) {
        self.server.stop();
        if self.root.starts_with(std::env::temp_dir()) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

fn unique_temp_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("{label}-{}-{stamp}", std::process::id()))
}

pub fn normalized_text(path: &Path) -> Result<String> {
    std::fs::read_to_string(path)
        .with_context(|| format!("failed to read `{}`", path.display()))
        .map(|content| content.replace("\r\n", "\n"))
}
