use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use pl_core::{AgentModelConfig, McpServerConfig, McpServerTransport, ProviderConfig};
use pl_protocol::{ThreadItemContent, ThreadItemStatus, TurnBillingRecord};
use rmcp::handler::server::ServerHandler;
use rmcp::model::*;
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{ErrorData as McpError, RoleServer};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

use super::{StudioRuntime, StudioSubmitPromptOptions, StudioSubmitPromptRequest};
use crate::config::{
    ConfigPaths, ConfigStore, ModelRouteConfig, ProviderId, ReasoningEffort, StudioConfig,
    StudioRole,
};
use crate::studio::StudioStore;
use crate::studio::entity::{thread, thread_context_segment, turn};

const MODEL: &str = "deepseek-v4-pro";
const EXPECTED_MODEL_REQUESTS: usize = 7;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deepseek_prompt_prefix_billing_and_tools_survive_sqlite_restart() -> Result<()> {
    let root = unique_temp_path("pure-deepseek-cache-billing");
    let result = run_fixture(&root).await;
    if root.starts_with(std::env::temp_dir()) {
        let _ = std::fs::remove_dir_all(&root);
    }
    result
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tool_failure_is_returned_to_the_model_and_kept_in_durable_history() -> Result<()> {
    let root = unique_temp_path("pure-deepseek-tool-failure");
    let result = run_tool_failure_fixture(&root).await;
    if root.starts_with(std::env::temp_dir()) {
        let _ = std::fs::remove_dir_all(&root);
    }
    result
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn interrupted_inference_ignores_a_late_provider_completion() -> Result<()> {
    let root = unique_temp_path("pure-deepseek-late-completion");
    let result = run_interrupted_fixture(&root).await;
    if root.starts_with(std::env::temp_dir()) {
        let _ = std::fs::remove_dir_all(&root);
    }
    result
}

async fn run_tool_failure_fixture(root: &Path) -> Result<()> {
    let home = root.join("home");
    let workspace = root.join("workspace");
    tokio::fs::create_dir_all(&home).await?;
    tokio::fs::create_dir_all(workspace.join(".git")).await?;
    tokio::fs::write(workspace.join("README.md"), "# Tool failure fixture\n").await?;

    let model_server = ScriptedDeepSeekServer::start(2, scripted_tool_failure_response).await?;
    let mcp_server = FixtureMcpServer::start().await?;
    let config_store = ConfigStore::new(ConfigPaths::from_home(&home));
    config_store.save(&fixture_config(
        model_server.base_url.clone(),
        mcp_server.url.clone(),
        &home,
    ))?;
    let store = StudioStore::open(root.join("studio.sqlite")).await?;
    let runtime = StudioRuntime::new(store.clone(), config_store)?;
    let project = runtime.open_project(&workspace).await?;
    let thread = runtime.create_thread(&project.id, "Tool failure").await?;
    runtime.start_runtime().await?;
    let submitted = runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: thread.id.clone(),
            prompt: "Read the deliberately missing file and report the tool failure.".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await?;
    if let Err(error) = wait_for_turn(&store, &submitted.turn_id).await {
        let request_count = model_server.requests().await.len();
        let accepted_count = model_server.accepted.load(Ordering::Relaxed);
        let server_errors = model_server.errors().await;
        let agent = match runtime.ensure_thread_agent(&thread.id).await {
            Ok((handle, agent_id)) => handle
                .snapshot(agent_id)
                .await
                .map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        let thread_snapshot = runtime.thread_snapshot(&thread.id).await;
        bail!(
            "{error:#}; model requests={request_count}; accepted={accepted_count}; \
             server errors={server_errors:?}; agent={agent:#?}; Thread={thread_snapshot:#?}"
        );
    }
    model_server.wait_complete().await?;

    let requests = model_server.requests().await;
    assert_eq!(requests.len(), 2);
    assert_request_is_append_only(&requests[0], &requests[1])?;
    assert!(serde_json::to_string(&requests[1])?.contains("missing-cache-file.txt"));
    let snapshot = runtime.thread_snapshot(&thread.id).await?;
    let failed_tool = snapshot
        .items
        .iter()
        .find(|item| {
            matches!(
                &item.content,
                ThreadItemContent::ToolCall { tool } if tool.name == "read_file"
            )
        })
        .context("failed read_file call missing from Thread timeline")?;
    let ThreadItemContent::ToolCall { tool } = &failed_tool.content else {
        unreachable!("matched tool call above")
    };
    let diagnostic = format!(
        "{} {}",
        failed_tool.error.as_deref().unwrap_or_default(),
        tool.result.as_deref().unwrap_or_default()
    )
    .to_ascii_lowercase();
    assert!(
        failed_tool.status == ThreadItemStatus::Failed
            || diagnostic.contains("missing-cache-file")
            || diagnostic.contains("not found")
    );
    assert!(snapshot.items.iter().any(|item| matches!(
        &item.content,
        ThreadItemContent::AgentMessage { text, .. } if text == "tool failure observed"
    )));

    runtime.shutdown_runtime().await?;
    mcp_server.stop();
    Ok(())
}

async fn run_interrupted_fixture(root: &Path) -> Result<()> {
    let home = root.join("home");
    let workspace = root.join("workspace");
    tokio::fs::create_dir_all(&home).await?;
    tokio::fs::create_dir_all(&workspace).await?;
    let model_server = DelayedDeepSeekServer::start().await?;
    let mcp_server = FixtureMcpServer::start().await?;
    let config_store = ConfigStore::new(ConfigPaths::from_home(&home));
    config_store.save(&fixture_config(
        model_server.base_url.clone(),
        mcp_server.url.clone(),
        &home,
    ))?;
    let store = StudioStore::open(root.join("studio.sqlite")).await?;
    let runtime = StudioRuntime::new(store.clone(), config_store)?;
    let project = runtime.open_project(&workspace).await?;
    let thread = runtime
        .create_thread(&project.id, "Late completion")
        .await?;
    runtime.start_runtime().await?;
    let submitted = runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: thread.id.clone(),
            prompt: "Wait until interrupted.".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await?;
    model_server.wait_until_requested().await?;
    let stopped = runtime.stop_prompt(thread.id.clone()).await?;
    assert!(stopped.stopped);
    model_server.release_completion();
    wait_for_turn(&store, &submitted.turn_id).await?;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let row = turn::Entity::find_by_id(submitted.turn_id)
        .one(store.database())
        .await?
        .context("interrupted Turn row missing")?;
    assert_eq!(row.status, "interrupted");
    assert!(row.model_json.is_none());
    let usage: pl_model::TokenUsage = serde_json::from_str(&row.usage_json)?;
    assert_eq!(usage, pl_model::TokenUsage::default());

    runtime.shutdown_runtime().await?;
    mcp_server.stop();
    Ok(())
}

async fn run_fixture(root: &Path) -> Result<()> {
    let home = root.join("home");
    let workspace = root.join("workspace");
    let database_path = root.join("studio.sqlite");
    tokio::fs::create_dir_all(&home).await?;
    tokio::fs::create_dir_all(workspace.join(".git")).await?;
    tokio::fs::create_dir_all(workspace.join("skills/cache-fixture")).await?;
    tokio::fs::write(workspace.join("README.md"), "# Cache fixture\n").await?;
    tokio::fs::write(
        workspace.join("skills/cache-fixture/SKILL.md"),
        "---\nname: cache-fixture\ndescription: Verify durable prompt caching.\n---\n\nRead the fixture and use the MCP lookup before editing.\n",
    )
    .await?;

    let model_server = ScriptedDeepSeekServer::start_cache_flow().await?;
    let mcp_server = FixtureMcpServer::start().await?;
    let config_store = ConfigStore::new(ConfigPaths::from_home(&home));
    config_store.save(&fixture_config(
        model_server.base_url.clone(),
        mcp_server.url.clone(),
        &home,
    ))?;

    let store = StudioStore::open(&database_path).await?;
    let runtime = StudioRuntime::new(store.clone(), config_store.clone())?;
    let project = runtime.open_project(&workspace).await?;
    let thread = runtime
        .create_thread(&project.id, "DeepSeek cache billing")
        .await?;
    runtime.start_runtime().await?;

    for prompt in [
        "Run the cache fixture tools and create cache-result.txt.",
        "Confirm the first result without changing the tool set.",
        "Confirm once more so the shared prefix can be reused.",
    ] {
        let submitted = runtime
            .submit_prompt(StudioSubmitPromptRequest {
                thread_id: thread.id.clone(),
                prompt: prompt.to_string(),
                attachment_ids: Vec::new(),
                options: StudioSubmitPromptOptions::default(),
            })
            .await?;
        if let Err(error) = wait_for_turn(&store, &submitted.turn_id).await {
            let request_count = model_server.requests().await.len();
            let accepted_count = model_server.accepted.load(Ordering::Relaxed);
            let server_errors = model_server.errors().await;
            let server_finished = model_server.handle.is_finished();
            let snapshot = runtime.runtime_snapshot().await?;
            let thread_snapshot = runtime.thread_snapshot(&thread.id).await;
            bail!(
                "{error:#}; model requests={request_count}; accepted={accepted_count}; \
                 server finished={server_finished}; \
                 server errors={server_errors:?}; runtime={snapshot:#?}; Thread={thread_snapshot:#?}"
            );
        }
    }

    model_server.wait_complete().await?;
    let requests = model_server.requests().await;
    assert_stable_append_only_requests(&requests)?;
    assert_eq!(mcp_server.lookup_calls().await, 1);
    let public = runtime.thread_snapshot(&thread.id).await?;
    assert_eq!(
        tokio::fs::read_to_string(workspace.join("cache-result.txt"))
            .await
            .with_context(|| format!("apply_patch 未创建结果文件，Thread={public:#?}"))?,
        "cache integration verified\n"
    );

    assert!(
        public
            .items
            .iter()
            .all(|item| !matches!(item.content, ThreadItemContent::ContextCompaction { .. }))
    );
    let runtime_usage = public
        .runtime
        .as_ref()
        .context("Thread runtime usage missing")?;
    assert!(runtime_usage.usage.cached_prompt_tokens > 0);
    assert!(
        runtime_usage
            .usage
            .cache_hit_rate
            .is_some_and(|rate| (0.0..=1.0).contains(&rate))
    );

    assert_persisted_billing(&store, &thread.id).await?;
    let context_segment_count = thread_context_segment::Entity::find()
        .filter(thread_context_segment::Column::ThreadId.eq(thread.id.clone()))
        .count(store.database())
        .await?;
    assert!(context_segment_count >= 2);

    runtime.shutdown_runtime().await?;
    drop(runtime);
    drop(store);

    let reopened_store = StudioStore::open(&database_path).await?;
    let reopened_runtime = StudioRuntime::new(reopened_store.clone(), config_store)?;
    reopened_runtime.start_runtime().await?;
    let restored = reopened_runtime.thread_snapshot(&thread.id).await?;
    let restored_usage = restored
        .runtime
        .as_ref()
        .context("restored runtime usage missing")?;
    assert_eq!(
        restored_usage.usage.prompt_tokens,
        runtime_usage.usage.prompt_tokens
    );
    assert_eq!(
        restored_usage.usage.cached_prompt_tokens,
        runtime_usage.usage.cached_prompt_tokens
    );
    assert_eq!(
        restored_usage.usage.estimated_costs,
        runtime_usage.usage.estimated_costs
    );
    assert!(
        restored
            .items
            .iter()
            .all(|item| !matches!(item.content, ThreadItemContent::ContextCompaction { .. }))
    );
    let restored_thread = thread::Entity::find_by_id(thread.id.clone())
        .one(reopened_store.database())
        .await?
        .context("restored Thread row missing")?;
    let persisted_usage: pl_model::TokenUsage = serde_json::from_str(&restored_thread.usage_json)?;
    assert_eq!(
        persisted_usage.prompt_tokens,
        restored_usage.usage.prompt_tokens
    );
    reopened_runtime.shutdown_runtime().await?;
    mcp_server.stop();
    Ok(())
}

async fn assert_persisted_billing(store: &StudioStore, thread_id: &str) -> Result<()> {
    let rows = turn::Entity::find()
        .filter(turn::Column::ThreadId.eq(thread_id))
        .order_by_asc(turn::Column::Ordinal)
        .all(store.database())
        .await?;
    assert_eq!(rows.len(), 3);
    let mut inference_ids = BTreeSet::new();
    let mut total_cached = 0;
    for row in rows {
        let billing: TurnBillingRecord = serde_json::from_str(
            row.model_json
                .as_deref()
                .context("Turn model_json has no billing record")?,
        )?;
        assert_eq!(billing.version, TurnBillingRecord::VERSION);
        for inference in &billing.inferences {
            assert!(inference_ids.insert(inference.inference_id.clone()));
            assert_eq!(inference.provider, "DeepSeek");
            assert_eq!(inference.model, MODEL);
            assert_eq!(inference.pricing.currency.as_deref(), Some("CNY"));
            assert_eq!(inference.pricing.input_per_mtok, Some(3.0));
            assert_eq!(inference.pricing.cache_read_per_mtok, Some(0.025));
            assert_eq!(
                inference.normalized_usage.cached_prompt_tokens,
                inference
                    .reported_usage
                    .cached_prompt_tokens
                    .min(inference.reported_usage.prompt_tokens)
            );
            total_cached += inference.normalized_usage.cached_prompt_tokens;
            assert_eq!(inference.estimated_costs.len(), 1);
        }
        let persisted_usage: pl_model::TokenUsage = serde_json::from_str(&row.usage_json)?;
        let aggregate = billing.aggregate_usage();
        assert_eq!(persisted_usage.prompt_tokens, aggregate.prompt_tokens);
        assert_eq!(
            persisted_usage.cached_prompt_tokens,
            aggregate.cached_prompt_tokens
        );
        assert_eq!(
            persisted_usage.completion_tokens,
            aggregate.completion_tokens
        );
    }
    assert_eq!(inference_ids.len(), EXPECTED_MODEL_REQUESTS);
    assert!(total_cached > 0);
    Ok(())
}

fn assert_stable_append_only_requests(requests: &[Value]) -> Result<()> {
    if requests.len() != EXPECTED_MODEL_REQUESTS {
        bail!(
            "expected {EXPECTED_MODEL_REQUESTS} model requests, got {}",
            requests.len()
        );
    }
    let first_tools = requests[0]["tools"].clone();
    let first_tools_bytes = serde_json::to_vec(&first_tools)?;
    let names = tool_names(&requests[0]);
    for required in ["skill_view", "read_file", "apply_patch"] {
        if !names.iter().any(|name| name == required) {
            bail!("model-visible tool set omitted {required}: {names:?}");
        }
    }
    if !names.iter().any(|name| name == "mcp__fixture__lookup") {
        bail!("model-visible tool set omitted fixture MCP tool: {names:?}");
    }
    let mut sorted_names = names.clone();
    sorted_names.sort();
    assert_eq!(names, sorted_names);

    for request in requests {
        assert_eq!(request["model"], MODEL);
        assert_eq!(serde_json::to_vec(&request["tools"])?, first_tools_bytes);
        assert!(request.get("prompt_cache_key").is_none());
    }
    for pair in requests.windows(2) {
        let previous = transcript_messages(&pair[0])?;
        let current = transcript_messages(&pair[1])?;
        if current.len() < previous.len() || current[..previous.len()] != previous[..] {
            bail!("DeepSeek transcript is not a strict append-only prefix");
        }
    }
    let serialized = serde_json::to_string(requests)?;
    assert!(!serialized.contains("# Current working context"));
    assert!(serialized.contains("cache fixture lookup ok"));
    Ok(())
}

fn transcript_messages(request: &Value) -> Result<Vec<Value>> {
    let messages = request["input"]
        .as_array()
        .context("request input missing")?;
    let tail_count = messages
        .iter()
        .filter(|message| is_working_context_tail(message))
        .count();
    if tail_count > 1 {
        bail!("model request contains {tail_count} working-context tails");
    }
    Ok(messages
        .iter()
        .filter(|message| !is_working_context_tail(message))
        .cloned()
        .collect())
}

fn is_working_context_tail(item: &Value) -> bool {
    item["type"] == "message"
        && item["role"] == "developer"
        && item["content"].as_array().is_some_and(|parts| {
            parts.iter().any(|part| {
                part["type"] == "input_text"
                    && part["text"]
                        .as_str()
                        .is_some_and(|text| text.starts_with("# Current working context"))
            })
        })
}

fn tool_names(request: &Value) -> Vec<String> {
    request["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| tool["name"].as_str())
        .map(str::to_string)
        .collect()
}

pub(super) async fn wait_for_turn(store: &StudioStore, turn_id: &str) -> Result<()> {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            if let Some(row) = turn::Entity::find_by_id(turn_id)
                .one(store.database())
                .await?
                && !matches!(row.status.as_str(), "queued" | "inProgress")
            {
                return Ok::<(), sea_orm::DbErr>(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .context("Studio turn did not reach a terminal state")??;
    Ok(())
}

fn fixture_config(model_base_url: String, mcp_url: String, home: &Path) -> StudioConfig {
    let mut info = pl_model::ProviderEndpoint::deepseek(Some(model_base_url));
    info.bearer_token = Some("fixture-token".to_string());
    let model = pl_model::default_models()
        .into_iter()
        .find(|model| model.slug == MODEL)
        .expect("bundled DeepSeek model");
    let effort = model.default_effort().map(ReasoningEffort::new);
    let provider_id = ProviderId::new("deepseek-fixture").expect("provider id");
    let route = ModelRouteConfig {
        provider: provider_id.clone(),
        model: MODEL.to_string(),
        effort,
    };
    let mut config = StudioConfig::default_config();
    config.models = AgentModelConfig {
        providers: BTreeMap::from([(
            provider_id,
            ProviderConfig::from_endpoint(info, vec![model]),
        )]),
        routes: [
            StudioRole::Explorer,
            StudioRole::Planner,
            StudioRole::Executor,
            StudioRole::Reviewer,
        ]
        .into_iter()
        .map(|role| (role.id(), route.clone()))
        .collect(),
    };
    config.runtime.permission_mode = pl_core::PermissionMode::FullAccess;
    config.runtime.tool_capabilities.exec = false;
    config.runtime.tool_capabilities.lsp = false;
    config.skills.enabled = true;
    config.skills.auto_learn = false;
    config.skills.system.enabled = false;
    config.skills.user_dir = home.join("user-skills").to_string_lossy().into_owned();
    config.mcp.servers.insert(
        "fixture".to_string(),
        McpServerConfig {
            enabled: true,
            transport: McpServerTransport::StreamableHttp,
            command: None,
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            url: Some(mcp_url),
            bearer_token_env_var: None,
            headers: BTreeMap::new(),
            startup_timeout_secs: Some(5),
            tool_timeout_secs: Some(5),
            enabled_tools: None,
            disabled_tools: Vec::new(),
        },
    );
    config
}

struct ScriptedDeepSeekServer {
    base_url: String,
    expected_requests: usize,
    requests: Arc<Mutex<Vec<Value>>>,
    errors: Arc<Mutex<Vec<String>>>,
    accepted: Arc<AtomicUsize>,
    handle: tokio::task::JoinHandle<Result<()>>,
}

impl ScriptedDeepSeekServer {
    async fn start_cache_flow() -> Result<Self> {
        Self::start(EXPECTED_MODEL_REQUESTS, scripted_chat_response).await
    }

    async fn start(
        expected_requests: usize,
        script: fn(usize, &Value) -> Result<String>,
    ) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let base_url = format!("http://{}", listener.local_addr()?);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let errors = Arc::new(Mutex::new(Vec::new()));
        let captured_errors = errors.clone();
        let accepted = Arc::new(AtomicUsize::new(0));
        let captured_accepted = accepted.clone();
        let handle = tokio::spawn(async move {
            let result = async {
                for step in 0..expected_requests {
                    let (mut socket, _) = listener.accept().await?;
                    captured_accepted.fetch_add(1, Ordering::Relaxed);
                    let request = read_http_json(&mut socket).await?;
                    captured.lock().await.push(request.clone());
                    let response = script(step, &request)?;
                    write_http_response(&mut socket, "text/event-stream", &response).await?;
                }
                Ok(())
            }
            .await;
            if let Err(error) = &result {
                captured_errors.lock().await.push(format!("{error:#}"));
            }
            result
        });
        Ok(Self {
            base_url,
            expected_requests,
            requests,
            errors,
            accepted,
            handle,
        })
    }

    async fn requests(&self) -> Vec<Value> {
        self.requests.lock().await.clone()
    }

    async fn errors(&self) -> Vec<String> {
        self.errors.lock().await.clone()
    }

    async fn wait_complete(&self) -> Result<()> {
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            while self.requests.lock().await.len() < self.expected_requests {
                if self.handle.is_finished() {
                    let errors = self.errors.lock().await;
                    bail!(
                        "scripted DeepSeek server stopped after {} requests: {errors:?}",
                        self.requests.lock().await.len()
                    );
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            Ok(())
        })
        .await
        .context("scripted DeepSeek server did not receive every request")??;
        Ok(())
    }
}

impl Drop for ScriptedDeepSeekServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

struct DelayedDeepSeekServer {
    base_url: String,
    requested: Arc<AtomicBool>,
    release: Arc<Notify>,
    handle: tokio::task::JoinHandle<Result<()>>,
}

impl DelayedDeepSeekServer {
    async fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let base_url = format!("http://{}", listener.local_addr()?);
        let requested = Arc::new(AtomicBool::new(false));
        let captured_requested = requested.clone();
        let release = Arc::new(Notify::new());
        let completion_release = release.clone();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await?;
            let _request = read_http_json(&mut socket).await?;
            captured_requested.store(true, Ordering::Release);
            completion_release.notified().await;
            let response = scripted_response(0, final_action("late completion"))?;
            write_http_response(&mut socket, "text/event-stream", &response).await
        });
        Ok(Self {
            base_url,
            requested,
            release,
            handle,
        })
    }

    async fn wait_until_requested(&self) -> Result<()> {
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            while !self.requested.load(Ordering::Acquire) {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .context("delayed DeepSeek server did not receive the model request")
    }

    fn release_completion(&self) {
        self.release.notify_one();
    }
}

impl Drop for DelayedDeepSeekServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

fn scripted_chat_response(step: usize, request: &Value) -> Result<String> {
    let action = match step {
        0 => tool_action("skill-1", "skill_view", json!({"name": "cache-fixture"})),
        1 => tool_action("read-1", "read_file", json!({"path": "README.md"})),
        2 => {
            let mcp = tool_names(request)
                .into_iter()
                .find(|name| name == "mcp__fixture__lookup")
                .context("fixture MCP tool was not installed")?;
            tool_action("mcp-1", &mcp, json!({"query": "prefix cache"}))
        }
        3 => tool_action(
            "patch-1",
            "apply_patch",
            json!({
                "input": "*** Begin Patch\n*** Add File: cache-result.txt\n+cache integration verified\n*** End Patch"
            }),
        ),
        4 => final_action("cache fixture complete"),
        5 => final_action("second turn confirmed"),
        6 => final_action("third turn confirmed"),
        _ => bail!("unexpected scripted model step {step}"),
    };
    scripted_response(step, action)
}

fn scripted_tool_failure_response(step: usize, _request: &Value) -> Result<String> {
    let action = match step {
        0 => tool_action(
            "missing-read",
            "read_file",
            json!({"path": "missing-cache-file.txt"}),
        ),
        1 => final_action("tool failure observed"),
        _ => bail!("unexpected tool failure script step {step}"),
    };
    scripted_response(step, action)
}

fn scripted_response(step: usize, action: ScriptedAction) -> Result<String> {
    let usage = responses_usage(step);
    let response_id = format!("resp-{step}");
    let mut events = Vec::new();
    match &action {
        ScriptedAction::ToolCall {
            id,
            name,
            arguments,
        } => {
            events.push(json!({
                "type": "response.output_item.added",
                "item": {"type": "function_call", "id": id, "call_id": id, "name": name}
            }));
            events.push(json!({
                "type": "response.function_call_arguments.delta",
                "item_id": id,
                "delta": arguments
            }));
            events.push(json!({
                "type": "response.output_item.done",
                "item": {
                    "type": "function_call",
                    "id": id,
                    "call_id": id,
                    "name": name,
                    "arguments": arguments
                }
            }));
        }
        ScriptedAction::FinalText(text) => {
            let message_id = format!("msg-{step}");
            events.push(json!({
                "type": "response.output_item.added",
                "item": {
                    "type": "message",
                    "id": message_id,
                    "role": "assistant",
                    "phase": "final_answer"
                }
            }));
            events.push(json!({
                "type": "response.output_text.delta",
                "item_id": message_id,
                "delta": text
            }));
            events.push(json!({
                "type": "response.output_item.done",
                "item": {
                    "type": "message",
                    "id": message_id,
                    "role": "assistant",
                    "phase": "final_answer",
                    "content": [{"type": "output_text", "text": text}]
                }
            }));
        }
    }
    events.push(json!({
        "type": "response.completed",
        "response": {"id": response_id, "usage": usage}
    }));
    let body = events
        .iter()
        .map(|event| Ok(format!("data: {}\n\n", serde_json::to_string(event)?)))
        .collect::<Result<String>>()?;
    Ok(format!("{body}data: [DONE]\n\n"))
}

/// 覆盖三种 Responses usage 形状：`input_tokens_details.cached_tokens`、
/// `prompt_tokens_details.cached_tokens` 与顶层 `prompt_cache_hit_tokens`。
fn responses_usage(step: usize) -> Value {
    let input_tokens = 100 + step as u64;
    let cached_tokens = if step == 0 { 0 } else { 20 + step as u64 };
    match step % 3 {
        0 => json!({
            "input_tokens": input_tokens,
            "output_tokens": 10,
            "total_tokens": input_tokens + 10,
            "input_tokens_details": {"cached_tokens": cached_tokens}
        }),
        1 => json!({
            "input_tokens": input_tokens,
            "output_tokens": 10,
            "total_tokens": input_tokens + 10,
            "prompt_tokens_details": {"cached_tokens": cached_tokens}
        }),
        _ => json!({
            "input_tokens": input_tokens,
            "output_tokens": 10,
            "total_tokens": input_tokens + 10,
            "prompt_cache_hit_tokens": cached_tokens
        }),
    }
}

fn assert_request_is_append_only(previous: &Value, current: &Value) -> Result<()> {
    let previous_messages = transcript_messages(previous)?;
    let current_messages = transcript_messages(current)?;
    if current_messages.len() < previous_messages.len()
        || current_messages[..previous_messages.len()] != previous_messages[..]
    {
        bail!("DeepSeek transcript is not a strict append-only prefix");
    }
    assert_eq!(previous["tools"], current["tools"]);
    Ok(())
}

enum ScriptedAction {
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    FinalText(String),
}

fn tool_action(id: &str, name: &str, arguments: Value) -> ScriptedAction {
    ScriptedAction::ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments: serde_json::to_string(&arguments).expect("tool arguments"),
    }
}

fn final_action(content: &str) -> ScriptedAction {
    ScriptedAction::FinalText(content.to_string())
}

#[derive(Debug, Clone)]
struct FixtureMcpHandler {
    calls: Arc<AtomicUsize>,
}

#[expect(
    clippy::manual_async_fn,
    reason = "RPITIT keeps the required Send bound explicit"
)]
impl ServerHandler for FixtureMcpHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2026_07_28)
            .with_server_info(Implementation::new("studio-cache-fixture", "1.0.0"))
            .with_instructions("Use lookup for cache fixture evidence.")
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(&[ProtocolVersion::V_2026_07_28])
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<ListToolsResult, McpError>> + Send + '_ {
        async {
            let schema = json!({
                "type": "object",
                "properties": { "query": { "type": "string" } },
                "required": ["query"],
                "additionalProperties": false
            })
            .as_object()
            .expect("fixture schema")
            .clone();
            Ok(ListToolsResult {
                tools: vec![Tool::new(
                    "lookup",
                    "Look up deterministic fixture evidence.",
                    Arc::new(schema),
                )],
                ..Default::default()
            })
        }
    }

    fn call_tool(
        &self,
        _request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<CallToolResponse, McpError>> + Send + '_ {
        async {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(CallToolResult::success(vec![ContentBlock::text("cache fixture lookup ok")]).into())
        }
    }
}

pub(super) struct FixtureMcpServer {
    pub(super) url: String,
    calls: Arc<AtomicUsize>,
    cancellation: CancellationToken,
    handle: tokio::task::JoinHandle<Result<()>>,
}

impl FixtureMcpServer {
    pub(super) async fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/mcp", listener.local_addr()?);
        let calls = Arc::new(AtomicUsize::new(0));
        let server_calls = calls.clone();
        let cancellation = CancellationToken::new();
        let config = StreamableHttpServerConfig::default()
            .with_json_response(true)
            .with_sse_keep_alive(None)
            .with_cancellation_token(cancellation.clone());
        let service: StreamableHttpService<FixtureMcpHandler, LocalSessionManager> =
            StreamableHttpService::new(
                move || {
                    Ok(FixtureMcpHandler {
                        calls: server_calls.clone(),
                    })
                },
                Default::default(),
                config,
            );
        let router = axum::Router::new().nest_service("/mcp", service);
        let shutdown = cancellation.clone();
        let handle = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
                .map_err(Into::into)
        });
        Ok(Self {
            url,
            calls,
            cancellation,
            handle,
        })
    }

    pub(super) async fn lookup_calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    pub(super) fn stop(&self) {
        self.cancellation.cancel();
        self.handle.abort();
    }
}

impl Drop for FixtureMcpServer {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.handle.abort();
    }
}

pub(super) async fn read_http_json(socket: &mut TcpStream) -> Result<Value> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let (header_end, content_length) = loop {
        let read = socket.read(&mut chunk).await?;
        if read == 0 {
            bail!("HTTP request ended before headers completed");
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let header_end = position + 4;
            let headers = String::from_utf8_lossy(&buffer[..header_end]);
            if headers.lines().any(|line| {
                line.split_once(':').is_some_and(|(name, value)| {
                    name.eq_ignore_ascii_case("expect")
                        && value.trim().eq_ignore_ascii_case("100-continue")
                })
            }) {
                socket.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await?;
            }
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':').and_then(|(name, value)| {
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                })
                .unwrap_or(0);
            break (header_end, content_length);
        }
    };
    while buffer.len() < header_end + content_length {
        let read = socket.read(&mut chunk).await?;
        if read == 0 {
            bail!("HTTP request ended before body completed");
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    serde_json::from_slice(&buffer[header_end..header_end + content_length]).map_err(Into::into)
}

pub(super) async fn write_http_response(
    socket: &mut TcpStream,
    content_type: &str,
    body: &str,
) -> Result<()> {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(response.as_bytes()).await?;
    socket.shutdown().await?;
    Ok(())
}

pub(super) fn unique_temp_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("{label}-{}-{stamp}", std::process::id()))
}
