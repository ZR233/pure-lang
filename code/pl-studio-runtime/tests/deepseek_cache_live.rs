use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};
use pl_studio_runtime::{
    AgentMessageChannel, ConfigPaths, ConfigStore, McpServerStatusKind, ProviderWireProtocol,
    STUDIO_CONFIG_SCHEMA_VERSION, StudioConfig, StudioRole, StudioRuntime, StudioStore,
    StudioSubmitPromptOptions, StudioSubmitPromptRequest, ThreadItemContent, TurnBillingRecord,
    TurnState,
};
use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement};

const LIVE_CONFIG_ENV: &str = "PURE_STUDIO_LIVE_INSTALLED_CONFIG";
const LIVE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const ZHIPU_SEARCH_ID: &str = "zhipu_search";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "uses the installed Studio DeepSeek and MCP configuration and incurs real usage"]
async fn installed_config_deepseek_cache_tools_and_billing_survive_restart() -> Result<()> {
    if std::env::var(LIVE_CONFIG_ENV).as_deref() != Ok("1") {
        eprintln!("set {LIVE_CONFIG_ENV}=1 to run the installed-config live test");
        return Ok(());
    }

    let installed = InstalledConfigGuard::load()?;
    let root = TempRoot::new("pure-deepseek-installed-config")?;
    let result = tokio::time::timeout(LIVE_TIMEOUT, run_live_flow(&installed, &root.path))
        .await
        .context("installed-config DeepSeek live test exceeded 30 minutes")
        .and_then(|result| result);
    let unchanged = installed.assert_unchanged();
    let cleanup = root.cleanup();
    cleanup?;
    result?;
    unchanged
}

async fn run_live_flow(installed: &InstalledConfigGuard, root: &Path) -> Result<()> {
    let installed_config = installed.store.load().with_context(|| {
        format!(
            "installed Studio config `{}` is invalid",
            installed.path.display()
        )
    })?;
    ensure!(
        installed_config.schema_version == STUDIO_CONFIG_SCHEMA_VERSION,
        "installed config schema is {}, expected {}",
        installed_config.schema_version,
        STUDIO_CONFIG_SCHEMA_VERSION
    );
    let home = root.join("home");
    let workspace = root.join("workspace");
    let database_path = root.join("studio.sqlite");
    tokio::fs::create_dir_all(&home).await?;
    tokio::fs::create_dir_all(&workspace).await?;
    tokio::fs::write(
        workspace.join("README.md"),
        "# Installed configuration cache verification\n",
    )
    .await?;

    let config_store = copy_config_to_temp_home(&installed_config, &home)?;
    let config = config_store.load()?;
    assert_preserved_config(&installed_config, &config)?;
    validate_live_route_and_mcp(&config)?;
    let skill_dir = workspace
        .join(&config.skills.project_dir)
        .join("cache-live");
    tokio::fs::create_dir_all(&skill_dir).await?;
    tokio::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: cache-live\ndescription: Verify installed DeepSeek prompt caching.\n---\n\nRead README.md, use the zhipu_search MCP lease, then create live-cache-result.txt with apply_patch.\n",
    )
    .await?;

    let store = StudioStore::open(&database_path).await?;
    let runtime = StudioRuntime::new(store.clone(), config_store.clone())?;
    let project = runtime.open_project(&workspace).await?;
    let thread = runtime
        .create_thread(&project.id, "Installed DeepSeek cache live")
        .await?;
    runtime.start_runtime().await?;

    let prompts = std::iter::once(
        "Execute this exact verification flow: call skill_view for cache-live; call read_file for README.md; call one read-only tool whose name begins mcp__zhipu_search__; call apply_patch to create live-cache-result.txt containing exactly installed cache verified; then give a final response.".to_string(),
    )
    .chain((2..=15).map(|round| {
        format!(
            "Confirm the installed cache verification result for round {round}. Keep the same model, instructions, and tool set."
        )
    }));
    let mut generations = Vec::new();
    let mut cached_rounds = 0_u64;
    for (index, prompt) in prompts.enumerate() {
        let round = index + 1;
        let submitted = runtime
            .submit_prompt(StudioSubmitPromptRequest {
                thread_id: thread.id.clone(),
                prompt,
                attachment_ids: Vec::new(),
                options: StudioSubmitPromptOptions::default(),
            })
            .await?;
        wait_for_completed_turn(&runtime, &thread.id, &submitted.turn_id).await?;
        generations.push(read_prompt_generation(&database_path, &thread.id).await?);
        let turn_billing = read_turn_billing(&database_path, &submitted.turn_id).await?;
        let turn_usage = turn_billing.aggregate_usage();
        if turn_usage.cached_prompt_tokens > 0 {
            cached_rounds += 1;
        }
        let round_snapshot = runtime.thread_snapshot(&thread.id).await?;
        let cumulative = &round_snapshot
            .runtime
            .context("live Thread runtime usage missing during cache report")?
            .usage;
        eprintln!(
            "DeepSeek cache round {round:02}: input={} cached={} hit={:.2}% cumulative_input={} cumulative_cached={} cumulative_hit={:.2}%",
            turn_usage.prompt_tokens,
            turn_usage.cached_prompt_tokens,
            cache_hit_percent(turn_usage.cached_prompt_tokens, turn_usage.prompt_tokens),
            cumulative.prompt_tokens,
            cumulative.cached_prompt_tokens,
            cache_hit_percent(cumulative.cached_prompt_tokens, cumulative.prompt_tokens),
        );
    }

    ensure!(
        generations.windows(2).all(|pair| pair[0] == pair[1]),
        "prompt generation changed across append-only live turns: {generations:?}"
    );
    let snapshot = runtime.thread_snapshot(&thread.id).await?;
    let usage = snapshot
        .runtime
        .as_ref()
        .context("live Thread runtime usage missing")?
        .usage
        .clone();
    ensure!(
        usage.cached_prompt_tokens > 0,
        "DeepSeek reported no cached prompt tokens across fifteen append-only turns"
    );
    ensure!(
        cached_rounds > 0,
        "DeepSeek reported no cache hit in any individual live Turn"
    );
    ensure!(
        !usage.estimated_costs.is_empty() && !usage.has_unpriced_usage,
        "live DeepSeek usage did not resolve to priced billing records"
    );
    assert_live_tool_sequence(&snapshot.items)?;
    ensure!(
        snapshot
            .items
            .iter()
            .filter(|item| matches!(
                &item.content,
                ThreadItemContent::AgentMessage {
                    channel: AgentMessageChannel::Final,
                    text,
                } if !text.trim().is_empty()
            ))
            .count()
            >= 15,
        "live model did not complete every Turn with a final response"
    );
    ensure!(
        tokio::fs::read_to_string(workspace.join("live-cache-result.txt"))
            .await?
            .trim()
            == "installed cache verified",
        "apply_patch did not create the expected live result"
    );

    runtime.shutdown_runtime().await?;
    drop(runtime);
    drop(store);

    let reopened_store = StudioStore::open(&database_path).await?;
    let reopened = StudioRuntime::new(reopened_store, config_store)?;
    reopened.start_runtime().await?;
    let restored = reopened.thread_snapshot(&thread.id).await?;
    let restored_usage = &restored
        .runtime
        .as_ref()
        .context("restored live Thread runtime usage missing")?
        .usage;
    ensure!(
        restored_usage.prompt_tokens == usage.prompt_tokens
            && restored_usage.cached_prompt_tokens == usage.cached_prompt_tokens
            && restored_usage.completion_tokens == usage.completion_tokens
            && restored_usage.estimated_costs == usage.estimated_costs,
        "restart did not restore the authoritative usage and cost snapshot"
    );
    let (context_segments, billing) = read_durable_diagnostics(&database_path, &thread.id).await?;
    ensure!(
        context_segments >= 1,
        "no durable context segment was restored"
    );
    ensure!(
        billing.len() == 15 && billing.iter().all(|turn| !turn.inferences.is_empty()),
        "live turns did not retain per-inference billing snapshots"
    );
    reopened.shutdown_runtime().await?;
    Ok(())
}

fn copy_config_to_temp_home(config: &StudioConfig, home: &Path) -> Result<ConfigStore> {
    ensure!(
        config.skills.enabled,
        "installed config has Skills disabled"
    );
    let mut copied = config.clone();
    copied.skills.auto_learn = false;
    copied.skills.user_dir = home.join("skills").to_string_lossy().into_owned();
    let store = ConfigStore::new(ConfigPaths::from_home(home));
    store.save(&copied)?;
    Ok(store)
}

fn assert_preserved_config(installed: &StudioConfig, copied: &StudioConfig) -> Result<()> {
    ensure!(
        installed.models == copied.models,
        "temporary config changed model providers or routes"
    );
    ensure!(
        installed.instructions == copied.instructions,
        "temporary config changed instructions"
    );
    ensure!(
        installed.mcp == copied.mcp,
        "temporary config changed MCP credentials or settings"
    );
    ensure!(
        installed.web_search == copied.web_search,
        "temporary config changed web search settings"
    );
    ensure!(
        installed.runtime == copied.runtime,
        "temporary config changed runtime settings"
    );
    ensure!(
        !copied.skills.auto_learn,
        "temporary config did not disable skill auto-learn"
    );
    Ok(())
}

fn validate_live_route_and_mcp(config: &StudioConfig) -> Result<()> {
    let route = config.resolve_role(StudioRole::Executor)?;
    ensure!(
        route.model.transport.protocol == ProviderWireProtocol::ChatCompletions,
        "executor provider does not use the DeepSeek Chat protocol"
    );
    ensure!(
        matches!(
            route.model.slug.as_str(),
            "deepseek-v4-flash" | "deepseek-v4-pro"
        ),
        "executor route resolves to `{}` instead of a bundled DeepSeek V4 model",
        route.model.slug
    );
    ensure!(
        route.model.currency.as_deref() == Some("CNY")
            && route.model.input_price_per_mtok.is_some()
            && route.model.output_price_per_mtok.is_some()
            && route.model.cache_read_price_per_mtok.is_some(),
        "executor DeepSeek model has incomplete pricing metadata"
    );
    let servers = pl_studio_runtime::config::effective_mcp_servers(config);
    let search = servers
        .get(ZHIPU_SEARCH_ID)
        .context("installed config did not expose the built-in zhipu_search MCP server")?;
    ensure!(
        search.status_kind == McpServerStatusKind::Enabled && search.bearer_token.is_some(),
        "zhipu_search MCP is not enabled with a credential"
    );
    Ok(())
}

async fn wait_for_completed_turn(
    runtime: &StudioRuntime,
    thread_id: &str,
    turn_id: &str,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10 * 60);
    loop {
        let page = runtime.list_thread_turns(thread_id, None, 100).await?;
        if let Some(history) = page.turns.iter().find(|history| history.turn.id == turn_id) {
            match &history.turn.state {
                TurnState::Completed => return Ok(()),
                TurnState::Failed { reason } | TurnState::Interrupted { reason } => {
                    bail!("live Turn ended before completion: {reason}")
                }
                TurnState::Queued | TurnState::InProgress { .. } => {}
            }
        }
        if Instant::now() >= deadline {
            bail!("live Turn did not complete within ten minutes");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn assert_live_tool_sequence(items: &[pl_studio_runtime::ThreadItem]) -> Result<()> {
    let names = items
        .iter()
        .filter_map(|item| match &item.content {
            ThreadItemContent::ToolCall { tool } => Some(tool.name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let skill = names.iter().position(|name| *name == "skill_view");
    let read = names.iter().position(|name| *name == "read_file");
    let mcp = names
        .iter()
        .position(|name| name.starts_with("mcp__zhipu_search__"));
    let patch = names.iter().position(|name| *name == "apply_patch");
    ensure!(
        matches!((skill, read, mcp, patch), (Some(a), Some(b), Some(c), Some(d)) if a < b && b < c && c < d),
        "live model did not execute skill_view -> read_file -> zhipu_search MCP -> apply_patch"
    );
    Ok(())
}

async fn read_prompt_generation(database_path: &Path, thread_id: &str) -> Result<u64> {
    let database = Database::connect(sqlite_url(database_path)).await?;
    let row = database
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT state_json FROM thread_session_state WHERE thread_id = ?",
            [thread_id.to_string().into()],
        ))
        .await?
        .context("live Thread session state row missing")?;
    let state_json: String = row.try_get("", "state_json")?;
    database.close().await?;
    let state: serde_json::Value = serde_json::from_str(&state_json)?;
    let prompt = &state["prompt"];
    let scope = prompt["activeScope"]
        .as_str()
        .context("active prompt scope missing")?;
    prompt["slots"][scope]["generation"]
        .as_u64()
        .context("active prompt generation missing")
}

async fn read_durable_diagnostics(
    database_path: &Path,
    thread_id: &str,
) -> Result<(u64, Vec<TurnBillingRecord>)> {
    let database = Database::connect(sqlite_url(database_path)).await?;
    let context_row = database
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM thread_context_segments WHERE thread_id = ?",
            [thread_id.to_string().into()],
        ))
        .await?
        .context("context segment count query returned no row")?;
    let context_segments: i64 = context_row.try_get("", "count")?;
    let rows = database
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT model_json FROM turns WHERE thread_id = ? AND model_json IS NOT NULL ORDER BY ordinal",
            [thread_id.to_string().into()],
        ))
        .await?;
    let billing = rows
        .into_iter()
        .map(|row| {
            let model_json: String = row.try_get("", "model_json")?;
            serde_json::from_str(&model_json).map_err(Into::into)
        })
        .collect::<Result<Vec<_>>>()?;
    database.close().await?;
    Ok((u64::try_from(context_segments)?, billing))
}

async fn read_turn_billing(database_path: &Path, turn_id: &str) -> Result<TurnBillingRecord> {
    let database = Database::connect(sqlite_url(database_path)).await?;
    let row = database
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT model_json FROM turns WHERE id = ?",
            [turn_id.to_string().into()],
        ))
        .await?
        .context("live Turn billing row missing")?;
    let model_json: String = row.try_get("", "model_json")?;
    database.close().await?;
    serde_json::from_str(&model_json).map_err(Into::into)
}

fn cache_hit_percent(cached_tokens: u64, prompt_tokens: u64) -> f64 {
    if prompt_tokens == 0 {
        0.0
    } else {
        cached_tokens as f64 * 100.0 / prompt_tokens as f64
    }
}

fn sqlite_url(path: &Path) -> String {
    format!(
        "sqlite://{}?mode=rwc",
        path.to_string_lossy().replace('\\', "/")
    )
}

struct InstalledConfigGuard {
    store: ConfigStore,
    path: PathBuf,
    original_bytes: Vec<u8>,
}

impl InstalledConfigGuard {
    fn load() -> Result<Self> {
        let store = ConfigStore::default_app()?;
        ensure!(
            store.config_exists(),
            "installed Studio config is missing at `{}`",
            store.paths().config_file().display()
        );
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
        ensure!(
            current == self.original_bytes,
            "live test modified installed Studio config `{}`",
            self.path.display()
        );
        Ok(())
    }
}

impl Drop for InstalledConfigGuard {
    fn drop(&mut self) {
        let unchanged =
            std::fs::read(&self.path).is_ok_and(|current| current == self.original_bytes);
        if !unchanged && !std::thread::panicking() {
            panic!(
                "installed Studio config changed during live test: {}",
                self.path.display()
            );
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
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn cleanup(&self) -> Result<()> {
        let temp_dir = std::env::temp_dir();
        ensure!(
            self.path.starts_with(&temp_dir),
            "refusing to clean live-test path outside `{}`",
            temp_dir.display()
        );
        if self.path.exists() {
            std::fs::remove_dir_all(&self.path).with_context(|| {
                format!(
                    "failed to remove temporary live configuration `{}`",
                    self.path.display()
                )
            })?;
        }
        Ok(())
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        if self.path.starts_with(std::env::temp_dir()) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
