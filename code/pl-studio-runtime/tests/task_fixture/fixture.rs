use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use pl_studio_runtime::{
    ConfigPaths, ConfigStore, InteractionKind, InteractionRequest, StudioHostKind, StudioMode,
    StudioRuntime, StudioRuntimeOptions, StudioStore, StudioTaskRuntime,
};
use tokio::net::TcpListener;

use super::config::task_test_config;
use super::git::git_output;
use super::server::{ScriptMode, ScriptedModelServer};

pub const DESIGN_PATH: &str = "design/task-flow.md";
pub const FEATURE_PATH: &str = "src/feature.txt";
pub const FEATURE_CONTENT: &str = "offline integration verified\n";
pub const PLANNER_FOLLOWUP_PATH: &str = "src/planner-followup.txt";
pub const PLANNER_FOLLOWUP_CONTENT: &str = "planner merge adjustment verified\n";

const TEST_TIMEOUT: Duration = Duration::from_secs(60);

pub struct TaskFlowFixture {
    pub runtime: StudioRuntime,
    pub store: StudioStore,
    pub workspace: PathBuf,
    pub thread_id: String,
    root: PathBuf,
    server: ScriptedModelServer,
}

impl TaskFlowFixture {
    pub async fn new() -> Result<Self> {
        Self::new_with_mode(ScriptMode::SingleExecutorEquivalent).await
    }

    pub async fn new_with_mode(mode: ScriptMode) -> Result<Self> {
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
        let studio_home = config_store.paths().config_dir().to_path_buf();
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
            .create_thread(&project.id, "Offline task orchestration")
            .await?;
        runtime
            .set_thread_mode(&session.id, StudioMode::Task)
            .await?;
        let server = ScriptedModelServer::start(
            listener,
            runtime.clone(),
            session.id.clone(),
            workspace.clone(),
            mode,
        );

        Ok(Self {
            runtime,
            store,
            workspace,
            thread_id: session.id,
            root,
            server,
        })
    }

    pub async fn wait_for_plan_confirmation(&self) -> Result<InteractionRequest> {
        let deadline = Instant::now() + TEST_TIMEOUT;
        loop {
            if let Some(interaction) = self
                .store
                .list_pending_interactions(&self.thread_id)
                .await?
                .into_iter()
                .find(|interaction| interaction.kind() == InteractionKind::PlanConfirmation)
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
            if let Some(task) = self.runtime.thread_task_view(&self.thread_id).await?
                && matches!(
                    &task.state,
                    pl_studio_runtime::StudioTaskState::Completed(_)
                )
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

    pub async fn shutdown(&self) -> Result<()> {
        self.runtime.shutdown_runtime().await?;
        Ok(())
    }

    async fn diagnostics(&self) -> String {
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
        let items = self
            .runtime
            .thread_snapshot(&self.thread_id)
            .await
            .map(|snapshot| {
                snapshot
                    .items
                    .into_iter()
                    .rev()
                    .take(20)
                    .map(|item| format!("{item:#?}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_else(|error| format!("Thread snapshot query failed: {error:#}"));
        let git = if self.workspace.join(".git").exists() {
            git_output(&self.workspace, &["status", "--porcelain"])
                .unwrap_or_else(|error| format!("git status failed: {error:#}"))
        } else {
            "repository not initialized".to_string()
        };
        let server = self.server.diagnostics().await;
        format!(
            "task projection:\n{task}\npending interactions:\n{interactions}\nrecent Thread Items:\n{items}\ngit status:\n{git}\n{server}"
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
