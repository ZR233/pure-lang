use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use pl_core::{AgentModelConfig, McpServerConfig, McpServerTransport, ProviderConfig};
use pl_model::{ProviderConnectionMode, TokenUsage};
use pl_protocol::{InferenceOrchestrationMetrics, ThreadItemContent, TurnBillingRecord};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use super::deepseek_cache::{
    FixtureMcpServer, read_http_json, unique_temp_path, wait_for_turn, write_http_response,
};
use super::{StudioRuntime, StudioSubmitPromptOptions, StudioSubmitPromptRequest};
use crate::config::{
    ConfigPaths, ConfigStore, ModelRouteConfig, ProviderId, ReasoningEffort, StudioConfig,
    StudioRole,
};
use crate::studio::StudioStore;
use crate::studio::entity::{thread, thread_context_segment, turn};

const MODEL: &str = "gpt-5.6-sol";
const EXPECTED_MODEL_REQUESTS: usize = 7;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn openai_responses_cache_key_billing_and_tools_survive_sqlite_restart() -> Result<()> {
    let root = unique_temp_path("pure-openai-cache-billing");
    let result = run_fixture(&root).await;
    if root.starts_with(std::env::temp_dir()) {
        let _ = std::fs::remove_dir_all(&root);
    }
    result
}

async fn run_fixture(root: &Path) -> Result<()> {
    let home = root.join("home");
    let workspace = root.join("workspace");
    let database_path = root.join("studio.sqlite");
    tokio::fs::create_dir_all(&home).await?;
    tokio::fs::create_dir_all(workspace.join(".git")).await?;
    tokio::fs::create_dir_all(workspace.join("skills/cache-fixture")).await?;
    tokio::fs::write(workspace.join("README.md"), "# OpenAI cache fixture\n").await?;
    tokio::fs::write(
        workspace.join("skills/cache-fixture/SKILL.md"),
        "---\nname: cache-fixture\ndescription: Verify durable OpenAI prompt caching.\n---\n\nRead the fixture and use the MCP lookup before editing.\n",
    )
    .await?;

    let model_server = ScriptedResponsesServer::start().await?;
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
        .create_thread(&project.id, "OpenAI cache billing")
        .await?;
    runtime.start_runtime().await?;

    for prompt in [
        "Run the cache fixture tools and create openai-cache-result.txt.",
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
        wait_for_turn(&store, &submitted.turn_id).await?;
    }

    model_server.wait_complete().await?;
    let requests = model_server.requests().await;
    assert_stable_append_only_requests(&requests)?;
    assert_eq!(mcp_server.lookup_calls().await, 1);
    let public = runtime.thread_snapshot(&thread.id).await?;
    assert_eq!(
        tokio::fs::read_to_string(workspace.join("openai-cache-result.txt"))
            .await
            .with_context(|| format!("apply_patch 未创建结果文件，Thread={public:#?}"))?,
        "openai cache integration verified\n"
    );

    assert!(
        public
            .items
            .iter()
            .all(|item| !matches!(item.content, ThreadItemContent::ContextCompaction { .. }))
    );
    let runtime_usage = &public
        .runtime
        .as_ref()
        .context("Thread runtime usage missing")?
        .usage;
    assert!(runtime_usage.cached_prompt_tokens > 0);
    assert!(runtime_usage.cache_write_tokens > 0);
    assert!(runtime_usage.reasoning_tokens > 0);
    assert_eq!(
        runtime_usage.inference_count,
        EXPECTED_MODEL_REQUESTS as u64
    );

    assert_persisted_billing(&store, &thread.id).await?;
    let context_segment_count = thread_context_segment::Entity::find()
        .filter(thread_context_segment::Column::ThreadId.eq(thread.id.clone()))
        .count(store.database())
        .await?;
    assert!(context_segment_count >= 2);

    let usage_before_restart = runtime_usage.clone();
    runtime.shutdown_runtime().await?;
    drop(runtime);
    drop(store);

    let reopened_store = StudioStore::open(&database_path).await?;
    let reopened_runtime = StudioRuntime::new(reopened_store.clone(), config_store)?;
    reopened_runtime.start_runtime().await?;
    let restored = reopened_runtime.thread_snapshot(&thread.id).await?;
    let restored_usage = &restored
        .runtime
        .as_ref()
        .context("restored runtime usage missing")?
        .usage;
    assert_eq!(restored_usage, &usage_before_restart);
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
    let persisted_usage: TokenUsage = serde_json::from_str(&restored_thread.usage_json)?;
    assert_eq!(persisted_usage.prompt_tokens, restored_usage.prompt_tokens);
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
    let mut saw_negative_savings = false;
    let mut orchestration = InferenceOrchestrationMetrics::default();
    for row in rows {
        let billing: TurnBillingRecord = serde_json::from_str(
            row.model_json
                .as_deref()
                .context("Turn model_json has no billing record")?,
        )?;
        assert_eq!(billing.version, TurnBillingRecord::VERSION);
        for inference in &billing.inferences {
            assert!(inference_ids.insert(inference.inference_id.clone()));
            assert_eq!(inference.provider, "OpenAI");
            assert_eq!(inference.model, MODEL);
            assert_eq!(inference.pricing.currency.as_deref(), Some("USD"));
            assert_eq!(inference.pricing.input_per_mtok, Some(2.0));
            assert_eq!(inference.pricing.output_per_mtok, Some(8.0));
            assert_eq!(inference.pricing.cache_read_per_mtok, Some(0.5));
            assert_eq!(inference.pricing.cache_write_per_mtok, Some(2.5));
            assert_eq!(
                inference.prompt_cache_policy.as_deref(),
                Some("openAiPromptCacheKey")
            );
            assert_eq!(inference.estimated_costs.len(), 1);
            assert_eq!(inference.estimated_cache_savings.len(), 1);
            assert!(inference.orchestration.tool_schema_estimated_tokens > 0);
            orchestration.merge(&inference.orchestration);
            saw_negative_savings |= inference.estimated_cache_savings[0].amount < 0.0;
        }
        let persisted_usage: TokenUsage = serde_json::from_str(&row.usage_json)?;
        let aggregate = billing.aggregate_usage();
        assert_eq!(persisted_usage.prompt_tokens, aggregate.prompt_tokens);
        assert_eq!(
            persisted_usage.cached_prompt_tokens,
            aggregate.cached_prompt_tokens
        );
        assert_eq!(
            persisted_usage.cache_write_tokens,
            aggregate.cache_write_tokens
        );
        assert_eq!(persisted_usage.reasoning_tokens, aggregate.reasoning_tokens);
        assert_eq!(
            persisted_usage.completion_tokens,
            aggregate.completion_tokens
        );
    }
    assert_eq!(inference_ids.len(), EXPECTED_MODEL_REQUESTS);
    assert_eq!(
        orchestration.transport_attempts,
        EXPECTED_MODEL_REQUESTS as u64
    );
    assert_eq!(orchestration.tool_calls, 4);
    assert!(orchestration.tool_result_estimated_tokens > 0);
    assert!(saw_negative_savings);
    Ok(())
}

fn assert_stable_append_only_requests(requests: &[Value]) -> Result<()> {
    if requests.len() != EXPECTED_MODEL_REQUESTS {
        bail!(
            "expected {EXPECTED_MODEL_REQUESTS} model requests, got {}",
            requests.len()
        );
    }
    let first_tools = serde_json::to_vec(&requests[0]["tools"])?;
    let first_instructions = serde_json::to_vec(&requests[0]["instructions"])?;
    let names = tool_names(&requests[0]);
    for required in ["skill_view", "read_file", "apply_patch"] {
        if !names.iter().any(|name| name == required) {
            bail!("model-visible tool set omitted {required}: {names:?}");
        }
    }
    if !names.iter().any(|name| name == "mcp__fixture__lookup") {
        bail!("model-visible tool set omitted fixture MCP tool: {names:?}");
    }
    let unique_names = names.iter().collect::<BTreeSet<_>>();
    assert_eq!(unique_names.len(), names.len());

    let cache_key = requests[0]["prompt_cache_key"]
        .as_str()
        .context("OpenAI Responses request omitted prompt_cache_key")?;
    for request in requests {
        assert_eq!(request["model"], MODEL);
        assert_eq!(serde_json::to_vec(&request["tools"])?, first_tools);
        assert_eq!(
            serde_json::to_vec(&request["instructions"])?,
            first_instructions
        );
        assert_eq!(request["prompt_cache_key"], cache_key);
        assert!(request.get("prompt_cache_breakpoint").is_none());
        assert!(request.get("prompt_cache_options").is_none());
    }
    for pair in requests.windows(2) {
        let previous = transcript_input(&pair[0])?;
        let current = transcript_input(&pair[1])?;
        if current.len() < previous.len() || current[..previous.len()] != previous[..] {
            bail!("OpenAI Responses transcript is not a strict append-only prefix");
        }
    }
    let serialized = serde_json::to_string(requests)?;
    assert!(!serialized.contains("# Current working context"));
    assert!(serialized.contains("cache fixture lookup ok"));
    Ok(())
}

fn transcript_input(request: &Value) -> Result<Vec<Value>> {
    let input = request["input"]
        .as_array()
        .context("Responses request input missing")?;
    let tail_count = input
        .iter()
        .filter(|item| is_working_context_tail(item))
        .count();
    if tail_count > 1 {
        bail!("Responses request contains {tail_count} working-context tails");
    }
    Ok(input
        .iter()
        .filter(|item| !is_working_context_tail(item))
        .cloned()
        .collect())
}

fn is_working_context_tail(item: &Value) -> bool {
    item["role"] == "developer"
        && item["content"].as_array().is_some_and(|parts| {
            parts.iter().any(|part| {
                part["text"]
                    .as_str()
                    .is_some_and(|text| text.starts_with("# Current working context"))
            })
        })
}

fn tool_names(request: &Value) -> Vec<String> {
    fn collect(tools: &[Value], names: &mut Vec<String>) {
        for tool in tools {
            match tool["type"].as_str() {
                Some("function" | "custom") => {
                    if let Some(name) = tool["name"].as_str() {
                        names.push(name.to_string());
                    }
                }
                Some("namespace") => {
                    if let Some(tools) = tool["tools"].as_array() {
                        collect(tools, names);
                    }
                }
                _ => {}
            }
        }
    }

    let mut names = Vec::new();
    if let Some(tools) = request["tools"].as_array() {
        collect(tools, &mut names);
    }
    names
}

fn fixture_config(model_base_url: String, mcp_url: String, home: &Path) -> StudioConfig {
    let mut info = pl_model::ProviderEndpoint::openai(Some(model_base_url));
    info.bearer_token = Some("fixture-token".to_string());
    let mut model = pl_model::default_models()
        .into_iter()
        .find(|model| model.slug == MODEL)
        .expect("bundled OpenAI model");
    model.transport.default_connection_mode = ProviderConnectionMode::Http;
    model.currency = Some("USD".to_string());
    model.input_price_per_mtok = Some(2.0);
    model.output_price_per_mtok = Some(8.0);
    model.cache_read_price_per_mtok = Some(0.5);
    model.cache_write_price_per_mtok = None;
    let effort = model.default_effort().map(ReasoningEffort::new);
    let provider_id = ProviderId::new("openai-fixture").expect("provider id");
    let route = ModelRouteConfig {
        provider: provider_id.clone(),
        model: MODEL.to_string(),
        effort,
    };
    let mut config = StudioConfig::default_config();
    config.models = AgentModelConfig {
        providers: BTreeMap::from([(
            provider_id,
            ProviderConfig::from_explicit_models(info, vec![model]),
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

struct ScriptedResponsesServer {
    base_url: String,
    requests: Arc<Mutex<Vec<Value>>>,
    errors: Arc<Mutex<Vec<String>>>,
    handle: tokio::task::JoinHandle<Result<()>>,
}

impl ScriptedResponsesServer {
    async fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let base_url = format!("http://{}", listener.local_addr()?);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let errors = Arc::new(Mutex::new(Vec::new()));
        let captured_errors = errors.clone();
        let handle = tokio::spawn(async move {
            let result = async {
                for step in 0..EXPECTED_MODEL_REQUESTS {
                    let (mut socket, _) = listener.accept().await?;
                    let request = read_http_json(&mut socket).await?;
                    captured.lock().await.push(request.clone());
                    let response = scripted_response(step, &request)?;
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
            requests,
            errors,
            handle,
        })
    }

    async fn requests(&self) -> Vec<Value> {
        self.requests.lock().await.clone()
    }

    async fn wait_complete(&self) -> Result<()> {
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            while self.requests.lock().await.len() < EXPECTED_MODEL_REQUESTS {
                if self.handle.is_finished() {
                    let errors = self.errors.lock().await;
                    bail!(
                        "scripted OpenAI server stopped after {} requests: {errors:?}",
                        self.requests.lock().await.len()
                    );
                }
                tokio::task::yield_now().await;
            }
            Ok(())
        })
        .await
        .context("scripted OpenAI server did not receive every request")??;
        Ok(())
    }
}

impl Drop for ScriptedResponsesServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

fn scripted_response(step: usize, request: &Value) -> Result<String> {
    match step {
        0 => function_tool_response(
            step,
            "skill-1",
            "skill_view",
            json!({"name": "cache-fixture"}),
        ),
        1 => function_tool_response(step, "read-1", "read_file", json!({"path": "README.md"})),
        2 => {
            let mcp = tool_names(request)
                .into_iter()
                .find(|name| name == "mcp__fixture__lookup")
                .context("fixture MCP tool was not installed")?;
            function_tool_response(step, "mcp-1", &mcp, json!({"query": "prefix cache"}))
        }
        3 => custom_tool_response(
            step,
            "patch-1",
            "apply_patch",
            "*** Begin Patch\n*** Add File: openai-cache-result.txt\n+openai cache integration verified\n*** End Patch",
        ),
        4 => final_response(step, "OpenAI cache fixture complete"),
        5 => final_response(step, "OpenAI second turn confirmed"),
        6 => final_response(step, "OpenAI third turn confirmed"),
        _ => bail!("unexpected scripted OpenAI step {step}"),
    }
}

fn function_tool_response(step: usize, id: &str, name: &str, arguments: Value) -> Result<String> {
    let item_id = format!("fc-{id}");
    let call_id = format!("call-{id}");
    responses_sse(
        step,
        vec![
            json!({
                "type": "response.output_item.added",
                "item": {
                    "type": "function_call",
                    "id": item_id,
                    "call_id": call_id,
                    "name": name
                }
            }),
            json!({
                "type": "response.output_item.done",
                "item": {
                    "type": "function_call",
                    "id": item_id,
                    "call_id": call_id,
                    "name": name,
                    "arguments": serde_json::to_string(&arguments)?
                }
            }),
        ],
    )
}

fn custom_tool_response(step: usize, id: &str, name: &str, input: &str) -> Result<String> {
    let item_id = format!("ctc-{id}");
    let call_id = format!("call-{id}");
    responses_sse(
        step,
        vec![
            json!({
                "type": "response.output_item.added",
                "item": {
                    "type": "custom_tool_call",
                    "id": item_id,
                    "call_id": call_id,
                    "name": name
                }
            }),
            json!({
                "type": "response.output_item.done",
                "item": {
                    "type": "custom_tool_call",
                    "id": item_id,
                    "call_id": call_id,
                    "name": name,
                    "input": input
                }
            }),
        ],
    )
}

fn final_response(step: usize, content: &str) -> Result<String> {
    let item_id = format!("msg-{step}");
    responses_sse(
        step,
        vec![
            json!({
                "type": "response.output_item.added",
                "item": {
                    "id": item_id,
                    "type": "message",
                    "role": "assistant",
                    "phase": "final_answer"
                }
            }),
            json!({
                "type": "response.output_text.delta",
                "item_id": item_id,
                "delta": content
            }),
            json!({
                "type": "response.output_item.done",
                "item": {
                    "id": item_id,
                    "type": "message",
                    "role": "assistant",
                    "phase": "final_answer",
                    "content": [{"type": "output_text", "text": content}]
                }
            }),
        ],
    )
}

fn responses_sse(step: usize, mut events: Vec<Value>) -> Result<String> {
    let input_tokens = 200 + step as u64;
    let cached_tokens = if step == 0 { 0 } else { 60 + step as u64 };
    let cache_write_tokens = if step == 0 { 40 } else { 5 };
    events.push(json!({
        "type": "response.completed",
        "response": {
            "id": format!("resp-{step}"),
            "model": MODEL,
            "usage": {
                "input_tokens": input_tokens,
                "output_tokens": 12,
                "total_tokens": input_tokens + 12,
                "input_tokens_details": {
                    "cached_tokens": cached_tokens,
                    "cache_write_tokens": cache_write_tokens
                },
                "output_tokens_details": {"reasoning_tokens": 3}
            }
        }
    }));
    events
        .into_iter()
        .map(|event| Ok(format!("data: {}\n\n", serde_json::to_string(&event)?)))
        .chain(std::iter::once(Ok("data: [DONE]\n\n".to_string())))
        .collect()
}
