//! 回归：根会话仍在执行时提交第二条 prompt 的中断与消费语义。
//!
//! 用真实 `StudioRuntime`/Thread 装配调用 `submit_prompt_command`，模型端是本地可控 SSE
//! fixture：第一条 Turn 的第一个模型请求被 fixture 挂起，因此提交第二条 prompt 时根会话确实
//! 仍在执行。等待全部走通知/订阅屏障而非任意 sleep，终态断言前用产品 observer 的同步屏障让
//! 持久历史追平 core commit。断言第二条新 inputId 的受理回执、第一条 Turn 的真实中断清理，以及
//! 后续模型请求只消费一次第二条输入。

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use pl_core::thread::{TurnState as CoreTurnState, input::InputState, task::TaskStatus};
use pl_model::{
    config::{AgentRoleId, ModelRouteConfig, ProviderConfig, ProviderId},
    model::ModelInfo,
    provider::ProviderEndpoint,
};
use pl_protocol::{
    PricingMode, ThreadModeId, TurnCancellationCause, TurnState,
    studio::{StudioPromptInput, SubmitPromptRequest},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Notify, mpsc},
    task::JoinHandle,
};

use crate::{StudioHostKind, StudioRole, StudioRuntime, StudioRuntimeOptions, WebSearchMode};

const FIXTURE_PROVIDER_ID: &str = "fixture";
const FIXTURE_MODEL: &str = "fixture-model";
const FIRST_PROMPT: &str = "PROMPT-ONE-SENTINEL-7f3a";
const SECOND_PROMPT: &str = "PROMPT-TWO-SENTINEL-9c1b";

/// 本地可控模型 provider。
///
/// 只把携带 JSON object 的 POST 视为补全请求：第 0 个补全请求被挂起，直到测试释放；后续补全
/// 请求立即返回最终回答。这样提交第二条 prompt 时，第一条 Turn 的第一个模型步骤确实在途。
///
/// 仓库内没有可复用的 in-crate 可控 SSE fixture：`pl_model::runtime::test_support` 只对
/// pl-model 可见且是一次性的，`tests/support/engine.rs` 属于集成测试 target 且不能挂起响应。
/// 因此本模块保留这个最小本地 fixture。
struct ScriptedProvider {
    url: String,
    requests: Arc<Mutex<Vec<serde_json::Value>>>,
    arrivals: mpsc::UnboundedReceiver<usize>,
    release_first: Arc<Notify>,
    _server: JoinHandle<()>,
}

impl ScriptedProvider {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (arrivals, arrivals_rx) = mpsc::unbounded_channel();
        let release_first = Arc::new(Notify::new());
        let completed = Arc::new(AtomicUsize::new(0));

        let captured = Arc::clone(&requests);
        let release = Arc::clone(&release_first);
        let server = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let captured = Arc::clone(&captured);
                let release = Arc::clone(&release);
                let arrivals = arrivals.clone();
                let completed = Arc::clone(&completed);
                tokio::spawn(async move {
                    let request = read_request(&mut socket).await;
                    let is_completion = request.method == "POST"
                        && request
                            .body
                            .as_ref()
                            .is_some_and(serde_json::Value::is_object);
                    if !is_completion {
                        // Provider 探测等非补全调用不参与脚本，直接拒绝。
                        let _ = socket
                            .write_all(
                                b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                            )
                            .await;
                        let _ = socket.shutdown().await;
                        return;
                    }
                    let index = completed.fetch_add(1, Ordering::SeqCst);
                    captured
                        .lock()
                        .unwrap()
                        .push(request.body.expect("a completion request carries a body"));
                    let _ = arrivals.send(index);
                    if index == 0 {
                        // 第一条 Turn 的模型调用在提交第二条 prompt 前一直不返回。
                        release.notified().await;
                    }
                    let body = completion_sse("fixture answer");
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });

        Self {
            url: format!("http://{address}"),
            requests,
            arrivals: arrivals_rx,
            release_first,
            _server: server,
        }
    }

    /// 等待第 `expected` 个补全请求（0 起）到达 fixture；用通知而非任意 sleep 同步。
    async fn await_arrival(&mut self, expected: usize) {
        let index = tokio::time::timeout(Duration::from_secs(10), self.arrivals.recv())
            .await
            .expect("a model request must arrive within the bounded window")
            .expect("the scripted provider stays alive");
        assert_eq!(index, expected, "model requests arrive in submission order");
    }

    fn captured(&self) -> Vec<serde_json::Value> {
        self.requests.lock().unwrap().clone()
    }

    fn release_first(&self) {
        self.release_first.notify_one();
    }
}

struct IncomingRequest {
    method: String,
    body: Option<serde_json::Value>,
}

/// 读取一个最小 HTTP 请求：请求行方法名与 `content-length` 限定的 JSON body。
async fn read_request(socket: &mut TcpStream) -> IncomingRequest {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let (header_end, content_length) = loop {
        let read = socket.read(&mut chunk).await.unwrap_or(0);
        if read == 0 {
            return IncomingRequest {
                method: String::new(),
                body: None,
            };
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(header_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&buffer[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            break (header_end, content_length);
        }
    };
    let method = String::from_utf8_lossy(&buffer[..header_end])
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    while buffer.len() < header_end + 4 + content_length {
        let read = socket.read(&mut chunk).await.unwrap_or(0);
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body = buffer
        .get(header_end + 4..header_end + 4 + content_length)
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(bytes).ok());
    IncomingRequest { method, body }
}

fn completion_sse(text: &str) -> String {
    format!(
        "data: {}\n\ndata: [DONE]\n\n",
        serde_json::json!({
            "choices": [{"delta": {"content": text}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5}
        })
    )
}

fn fixture_provider_id() -> ProviderId {
    ProviderId::new(FIXTURE_PROVIDER_ID).expect("static provider id is valid")
}

fn fixture_provider(url: &str) -> ProviderConfig {
    let mut provider = ProviderConfig::from_explicit_models(
        ProviderEndpoint::deepseek(Some(url.to_string())),
        vec![ModelInfo::compatible(FIXTURE_MODEL)],
    );
    provider.pricing_mode = PricingMode::Disabled;
    provider
}

fn fixture_route() -> ModelRouteConfig {
    ModelRouteConfig {
        provider: fixture_provider_id(),
        model: FIXTURE_MODEL.to_string(),
        effort: None,
    }
}

fn prompt(input_id: &str, text: &str) -> SubmitPromptRequest {
    SubmitPromptRequest {
        input: StudioPromptInput {
            input_id: input_id.to_string(),
            text: text.to_string(),
            attachment_draft_ids: Vec::new(),
        },
    }
}

fn occurrences(value: &serde_json::Value, needle: &str) -> usize {
    value.to_string().matches(needle).count()
}

/// 订阅 owner 快照直到 core 现场空闲（无运行中 Turn/任务、无 pending 输入）。
///
/// 事件驱动，不做轮询 sleep；判定与 `thread_is_busy` 一致。整个等待有界：若不空闲则超时失败，
/// 超时只作为失败信号，绝不作为成功判据。
async fn wait_until_owner_idle(runtime: &StudioRuntime, thread_id: &str) {
    let thread = runtime.ensure_thread_owner(thread_id).await.unwrap();
    let mut updates = thread.subscribe();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let snapshot = updates.next().await.expect("the Thread owner stays open");
            if !snapshot
                .turns
                .iter()
                .any(|turn| turn.state == CoreTurnState::Running)
                && !snapshot
                    .inputs
                    .iter()
                    .any(|input| input.state == InputState::Pending)
                && !snapshot
                    .tasks
                    .values()
                    .any(|task| task.status == TaskStatus::Running)
            {
                return;
            }
        }
    })
    .await
    .expect("the Thread owner must reach an idle core state within the bounded window");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn submitting_a_second_prompt_interrupts_the_running_root_turn_and_consumes_the_new_input_once()
 {
    let mut provider = ScriptedProvider::start().await;
    let provider_url = provider.url.clone();
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();

    let runtime = StudioRuntime::with_options(StudioRuntimeOptions {
        studio_home: Some(home.path().to_path_buf()),
        host: StudioHostKind::Test,
    })
    .await
    .unwrap();
    let revision = runtime.config_runtime.read().unwrap().revision;
    runtime
        .config_runtime
        .update(revision, |config| {
            let providers: BTreeMap<ProviderId, ProviderConfig> =
                BTreeMap::from([(fixture_provider_id(), fixture_provider(&provider_url))]);
            let routes: BTreeMap<AgentRoleId, ModelRouteConfig> = StudioRole::child_roles()
                .into_iter()
                .map(|role| (role.id(), fixture_route()))
                .collect();
            let mode_routes: BTreeMap<ThreadModeId, ModelRouteConfig> = BTreeMap::from([
                (ThreadModeId::simple(), fixture_route()),
                (ThreadModeId::task(), fixture_route()),
            ]);
            let mut next = config.clone();
            next.models.providers = providers;
            next.models.routes = routes;
            next.mode_model_routes = mode_routes;
            // 关闭 Web Search，避免 hosted 工具让请求形态与断言目标无关地波动。
            next.web_search.mode = WebSearchMode::Disabled;
            next.deepseek_web_search.enabled = false;
            // 对齐既有最小工具现场（thread_service::threads::runtime_with_thread_without_optional_tools）：
            // 本用例只验证 prompt 提交路径，不装配 exec/MCP/LSP/Skills，避免环境相关噪声。
            next.runtime.tool_capabilities.exec = false;
            next.runtime.tool_capabilities.workspace_files = false;
            next.runtime.tool_capabilities.skills = false;
            next.runtime.tool_capabilities.mcp = false;
            next.runtime.tool_capabilities.lsp = false;
            next.runtime.tool_capabilities.ask_user = false;
            next.runtime.tool_capabilities.git = false;
            next.skills.enabled = false;
            Ok(next)
        })
        .unwrap();
    runtime.start_runtime().await.unwrap();
    let project = runtime.open_project(workspace.path()).await.unwrap();
    let thread = runtime
        .create_thread(&project.id, "second prompt interrupt")
        .await
        .unwrap();
    let thread_id = thread.id.clone();

    // 第一条 prompt 受理后，Turn 的第一个模型步骤停在 fixture 上不返回。
    let first = runtime
        .submit_prompt_command(thread_id.clone(), prompt("prompt-1", FIRST_PROMPT))
        .await
        .unwrap();
    assert_eq!(first.input_id, "prompt-1");
    assert_eq!(first.thread_id, thread_id);

    provider.await_arrival(0).await;
    // 请求只可能由正在执行的 Turn 发出，因此此刻第一条 Turn 必然仍在 Running；不做任意 sleep，
    // 直接断言现场。
    let running = runtime.ensure_thread_owner(&thread_id).await.unwrap();
    assert!(
        running
            .snapshot()
            .turns
            .iter()
            .any(|turn| turn.state == CoreTurnState::Running),
        "the first Turn must still be running while its model step is in flight"
    );

    // 根会话仍在执行时提交第二条 prompt：受理回执是新的 inputId，提交位置严格前进。
    let second = runtime
        .submit_prompt_command(thread_id.clone(), prompt("prompt-2", SECOND_PROMPT))
        .await
        .unwrap();
    assert_eq!(second.input_id, "prompt-2");
    assert_eq!(second.thread_id, thread_id);
    assert!(
        second.cursor > first.cursor,
        "the second receipt advances the commit watermark: {} > {}",
        second.cursor,
        first.cursor
    );

    // 下一条 Turn 的模型请求只能在被中断的 Turn 收束后发出，因此它到达即证明第一次 Turn 已真实
    // 中断并清理。
    provider.await_arrival(1).await;
    provider.release_first();
    wait_until_owner_idle(&runtime, &thread_id).await;

    // core 物理空闲不等于持久历史已投影：分页事实来自产品 observer 的持久化历史，必须显式同步到
    // 当前 commit 再断言终态，不能靠 sleep 掩盖投影水位差。
    runtime
        .synchronize_thread_observation(&thread_id)
        .await
        .unwrap();
    runtime
        .persistence_repository()
        .await
        .unwrap()
        .flush()
        .await
        .unwrap();
    let page = runtime
        .list_thread_turns(&thread_id, None, 20)
        .await
        .unwrap();
    let interrupted = page
        .turns
        .iter()
        .find(|entry| entry.turn.input_id.as_deref() == Some("prompt-1"))
        .expect("the interrupted Turn is recorded in durable history");
    let TurnState::Cancelled(cancelled) = &interrupted.turn.state else {
        panic!(
            "the first Turn must be interrupted by the second prompt: {:?}",
            interrupted.turn.state
        );
    };
    assert_eq!(cancelled.cause(), &TurnCancellationCause::Interrupted);
    let completed = page
        .turns
        .iter()
        .find(|entry| entry.turn.input_id.as_deref() == Some("prompt-2"))
        .expect("the new input opens the follow-up Turn");
    assert!(
        matches!(completed.turn.state, TurnState::Completed(_)),
        "the follow-up Turn completes: {:?}",
        completed.turn.state
    );

    let requests = provider.captured();
    assert!(
        requests.len() >= 2,
        "the interrupted request and the follow-up request are both observed"
    );
    assert_eq!(
        occurrences(&requests[0], FIRST_PROMPT),
        1,
        "the interrupted request carries its own input exactly once"
    );
    assert_eq!(
        occurrences(&requests[0], SECOND_PROMPT),
        0,
        "the second input never reaches the interrupted request"
    );
    let carrying: Vec<&serde_json::Value> = requests
        .iter()
        .filter(|request| occurrences(request, SECOND_PROMPT) > 0)
        .collect();
    assert_eq!(
        carrying.len(),
        1,
        "the second input is consumed by exactly one model request, not replayed"
    );
    assert_eq!(
        occurrences(carrying[0], SECOND_PROMPT),
        1,
        "the follow-up request carries the second input exactly once"
    );
    assert_eq!(
        occurrences(carrying[0], FIRST_PROMPT),
        1,
        "the follow-up request keeps the interrupted input exactly once"
    );

    runtime.shutdown().await;
}
