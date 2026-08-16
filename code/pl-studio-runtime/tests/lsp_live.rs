use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use pl_studio_runtime::{
    AgentMessageChannel, BuiltinMcpServerState, ConfigPaths, ConfigStore, McpServerStatusKind,
    STUDIO_CONFIG_SCHEMA_VERSION, StudioConfig, StudioRole, StudioRuntime, StudioStore,
    StudioSubmitPromptOptions, StudioSubmitPromptRequest, ThreadItem, ThreadItemContent,
    ThreadItemStatus, TurnState, WebSearchMode, builtin_mcp_server_ids,
};

const LIVE_CONFIG_ENV: &str = "PURE_STUDIO_LIVE_INSTALLED_CONFIG";
const LIVE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const LIVE_VERIFY_MARKER: &str = "PURE_LSP_PROMPT_VERIFY_OK";
const LSP_TOOL_NAME: &str = "lsp_query";
const RUST_LANGUAGE_ID: &str = "rust";
const RUST_ANALYZER_ID: &str = "rust-analyzer";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "uses the installed Studio executor configuration and incurs real model usage"]
async fn installed_config_prompt_uses_rust_analyzer_on_temporary_cargo_demo() -> Result<()> {
    if std::env::var(LIVE_CONFIG_ENV).as_deref() != Ok("1") {
        eprintln!("set {LIVE_CONFIG_ENV}=1 to run the installed-config live test");
        return Ok(());
    }

    let installed = InstalledConfigGuard::load()?;
    let root = tempfile::Builder::new()
        .prefix("pure-lsp-prompt-live-")
        .tempdir()?;
    let result = tokio::time::timeout(LIVE_TIMEOUT, run_live_flow(&installed, root.path()))
        .await
        .context("installed-config LSP live test exceeded fifteen minutes")
        .and_then(|result| result);
    let unchanged = installed.assert_unchanged();
    let cleanup = root.close().context("failed to remove LSP live-test root");

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
    let installed_route = installed_config.resolve_role(StudioRole::Executor)?;
    ensure!(
        installed_route.provider_info.bearer_token.is_some(),
        "installed executor route has no resolved credential"
    );
    eprintln!(
        "LSP live route: provider={}, model={}",
        installed_route.provider_id, installed_route.model.slug
    );

    let home = root.join("home");
    let workspace = root.join("workspace");
    let database_path = root.join("studio.sqlite");
    create_cargo_demo(&workspace).await?;
    let config_store = isolated_config_store(&installed_config, &home)?;
    let isolated_config = config_store.load()?;
    assert_isolated_capabilities(&isolated_config)?;
    let isolated_route = isolated_config.resolve_role(StudioRole::Executor)?;
    ensure!(
        isolated_route.provider_id == installed_route.provider_id
            && isolated_route.model.slug == installed_route.model.slug,
        "isolated live config changed the executor route"
    );
    ensure!(
        isolated_route.provider_info.bearer_token.is_some(),
        "isolated live config did not retain the executor credential"
    );
    let store = StudioStore::open(&database_path).await?;
    let runtime = StudioRuntime::new(store.clone(), config_store.clone())?;
    let project = runtime.open_project(&workspace).await?;
    let thread = runtime
        .create_thread(&project.id, "Installed config Rust LSP live")
        .await?;
    runtime.start_runtime().await?;

    let live_result = run_prompt_and_assert(&runtime, &thread.id).await;
    let shutdown = tokio::time::timeout(Duration::from_secs(30), runtime.shutdown_runtime())
        .await
        .context("Studio runtime shutdown timed out")
        .and_then(|result| result.map(|_| ()));
    drop(runtime);
    drop(store);

    shutdown?;
    live_result?;

    let reopened_store = StudioStore::open(&database_path).await?;
    let reopened = StudioRuntime::new(reopened_store.clone(), config_store)?;
    let persisted_result = async {
        reopened.start_runtime().await?;
        let persisted = reopened.thread_snapshot(&thread.id).await?;
        assert_lsp_tool_results(&persisted.items)?;
        assert_final_marker(&persisted.items)
    }
    .await;
    let reopened_shutdown =
        tokio::time::timeout(Duration::from_secs(30), reopened.shutdown_runtime())
            .await
            .context("reopened Studio runtime shutdown timed out")
            .and_then(|result| result.map(|_| ()));
    drop(reopened);
    drop(reopened_store);
    reopened_shutdown?;
    persisted_result
}

async fn run_prompt_and_assert(runtime: &StudioRuntime, thread_id: &str) -> Result<()> {
    let submitted = runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: thread_id.to_string(),
            prompt: live_prompt().to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await?;
    wait_for_completed_turn(runtime, thread_id, &submitted.turn_id).await?;
    let snapshot = runtime.thread_snapshot(thread_id).await?;
    let active_lsp_servers = &snapshot
        .runtime
        .as_ref()
        .context("live Thread runtime snapshot is missing")?
        .active_lsp_servers;
    ensure!(
        active_lsp_servers
            .iter()
            .any(|server| server == RUST_ANALYZER_ID),
        "live Thread did not expose rust-analyzer: {active_lsp_servers:?}"
    );
    assert_lsp_tool_results(&snapshot.items)?;
    assert_final_marker(&snapshot.items)
}

async fn create_cargo_demo(workspace: &Path) -> Result<()> {
    let source_root = workspace.join("src");
    tokio::fs::create_dir_all(&source_root).await?;
    tokio::fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname='pure_lsp_prompt_demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .await?;
    tokio::fs::write(
        source_root.join("lib.rs"),
        "pub fn answer() -> i32 {\n    42\n}\n",
    )
    .await?;
    tokio::fs::write(
        source_root.join("main.rs"),
        "use pure_lsp_prompt_demo::answer;\n\nfn main() {\n    println!(\"{}\", answer());\n}\n",
    )
    .await?;
    Ok(())
}

fn isolated_config_store(config: &StudioConfig, home: &Path) -> Result<ConfigStore> {
    let mut copied = config.clone();
    copied.web_search.mode = WebSearchMode::Disabled;
    copied.runtime.tool_capabilities.exec = false;
    copied.runtime.tool_capabilities.workspace_files = false;
    copied.runtime.tool_capabilities.skills = false;
    copied.runtime.tool_capabilities.mcp = false;
    copied.runtime.tool_capabilities.lsp = true;
    copied.runtime.tool_capabilities.ask_user = false;
    copied.runtime.tool_capabilities.git = false;
    copied.runtime.active_skills.clear();
    copied.runtime.active_mcp_servers.clear();
    copied.skills.enabled = false;
    copied.skills.auto_learn = false;
    copied.skills.user_dir = home.join("skills").to_string_lossy().into_owned();
    copied.mcp.servers.clear();
    copied.mcp.builtin_servers = builtin_mcp_server_ids()
        .into_iter()
        .map(|server_id| {
            (
                server_id.to_string(),
                BuiltinMcpServerState { enabled: false },
            )
        })
        .collect();

    let store = ConfigStore::new(ConfigPaths::from_home(home));
    store.save(&copied)?;
    Ok(store)
}

fn assert_isolated_capabilities(config: &StudioConfig) -> Result<()> {
    let capabilities = &config.runtime.tool_capabilities;
    ensure!(
        !capabilities.exec
            && !capabilities.workspace_files
            && !capabilities.skills
            && !capabilities.mcp
            && capabilities.lsp
            && !capabilities.ask_user
            && !capabilities.git,
        "isolated live config did not expose only the LSP capability: {capabilities:?}"
    );
    ensure!(
        config.web_search.mode == WebSearchMode::Disabled,
        "isolated live config retained web search"
    );
    ensure!(
        config.runtime.active_skills.is_empty()
            && config.runtime.active_mcp_servers.is_empty()
            && !config.skills.enabled,
        "isolated live config retained Skills or MCP activation"
    );
    ensure!(
        pl_studio_runtime::config::effective_mcp_servers(config)
            .values()
            .all(|server| server.status_kind != McpServerStatusKind::Enabled),
        "isolated live config retained an enabled MCP server"
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

fn assert_lsp_tool_results(items: &[ThreadItem]) -> Result<()> {
    let mut results = BTreeMap::new();
    let mut tool_call_count = 0;
    for item in items {
        let ThreadItemContent::ToolCall { tool } = &item.content else {
            continue;
        };
        tool_call_count += 1;
        ensure!(
            tool.name == LSP_TOOL_NAME,
            "live model called unexpected tool `{}`",
            tool.name
        );
        ensure!(
            item.status == ThreadItemStatus::Completed,
            "LSP tool item `{}` did not complete: {:?}",
            item.id,
            item.status
        );
        let arguments: serde_json::Value =
            serde_json::from_str(&tool.arguments).context("LSP tool arguments are not JSON")?;
        ensure!(
            arguments["languageId"].as_str() == Some(RUST_LANGUAGE_ID),
            "live model called {LSP_TOOL_NAME} without languageId `{RUST_LANGUAGE_ID}`: {arguments}"
        );
        let operation = arguments["operation"]
            .as_str()
            .context("LSP tool arguments omit operation")?;
        let output: serde_json::Value = serde_json::from_str(
            tool.result
                .as_deref()
                .context("completed LSP tool result is missing")?,
        )
        .context("LSP tool result is not JSON")?;
        ensure!(
            output["success"].as_bool() == Some(true),
            "LSP operation `{operation}` failed: {output}"
        );
        ensure!(
            output["serverId"].as_str() == Some(RUST_ANALYZER_ID),
            "LSP operation `{operation}` used an unexpected server: {output}"
        );
        ensure!(
            output["operation"].as_str() == Some(operation),
            "LSP operation mismatch for `{operation}`: {output}"
        );
        ensure!(
            results.insert(operation.to_string(), output).is_none(),
            "live model repeated LSP operation `{operation}`"
        );
    }

    ensure!(
        tool_call_count == 4,
        "live model made {tool_call_count} tool calls instead of exactly four"
    );

    for operation in [
        "documentSymbol",
        "hover",
        "goToDefinition",
        "findReferences",
    ] {
        ensure!(
            results.contains_key(operation),
            "live model did not complete `{operation}` through {LSP_TOOL_NAME}"
        );
    }
    ensure!(
        results["documentSymbol"]["result"]
            .as_str()
            .is_some_and(|result| result.contains("answer")),
        "documentSymbol did not return `answer`"
    );
    ensure!(
        results["hover"]["result"]
            .as_str()
            .is_some_and(|result| result.contains("answer")),
        "hover did not return `answer`"
    );
    let definition = normalized_result(&results["goToDefinition"])?;
    ensure!(
        definition.contains("src/lib.rs"),
        "goToDefinition did not resolve src/lib.rs: {definition}"
    );
    ensure!(
        results["findReferences"]["resultCount"]
            .as_u64()
            .is_some_and(|count| count >= 2),
        "findReferences did not include both declaration and call site"
    );
    let references = normalized_result(&results["findReferences"])?;
    ensure!(
        references.contains("src/lib.rs") && references.contains("src/main.rs"),
        "findReferences did not cover the declaration and call site: {references}"
    );
    Ok(())
}

fn normalized_result(output: &serde_json::Value) -> Result<String> {
    Ok(output["result"]
        .as_str()
        .context("LSP output omits result text")?
        .replace('\\', "/"))
}

fn assert_final_marker(items: &[ThreadItem]) -> Result<()> {
    ensure!(
        items.iter().any(|item| matches!(
            &item.content,
            ThreadItemContent::AgentMessage {
                channel: AgentMessageChannel::Final,
                text,
            } if text.contains(LIVE_VERIFY_MARKER)
        )),
        "live model final response did not contain `{LIVE_VERIFY_MARKER}`"
    );
    Ok(())
}

fn live_prompt() -> &'static str {
    r#"Perform this deterministic Rust LSP verification in the current temporary Cargo workspace.
Do not delegate and do not use any tool except lsp_query.
Call lsp_query with languageId "rust" for all four operations below, using these exact inputs:
1. documentSymbol on src/lib.rs.
2. hover on src/lib.rs at line 1, character 8.
3. goToDefinition on src/main.rs at line 4, character 20.
4. findReferences on src/main.rs at line 4, character 20.
After every tool call succeeds, respond with a final answer containing exactly the marker PURE_LSP_PROMPT_VERIFY_OK, and state that the definition is in src/lib.rs and the references include src/lib.rs and src/main.rs."#
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
