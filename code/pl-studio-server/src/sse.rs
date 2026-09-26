use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::Stream;
use pl_protocol::{
    ChatWindowQuery, ChatWindowUpdate, ThreadNotification, ThreadSubscriptionRequest,
    ThreadSubscriptionUpdate,
};
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

use crate::AppState;
use crate::error::ApiError;
use crate::routes::{ApiQuery, StudioApiErrors};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StaleEvent {
    reason: &'static str,
    dropped: Option<u64>,
    resync: &'static str,
}

/// Product 事件是可合并的目录/状态更新：缓冲满时丢弃该条并经一次 `stale` 让客户端
/// 从 `/api/v1/state` 重同步。`stale` 等待容量后送达，因此慢消费者不会因为缓冲满被
/// 静默断开；连接本身的终态由流的关闭表达。
enum Delivery {
    Sent,
    /// 缓冲已满：该条更新被合并，需要先送达一次 `stale` 标记。
    Stalled,
    /// 该帧无法送达：接收端已关闭，或事件无法编码。调用方应结束该流，让客户端重连重同步，
    /// 不把未送达的观测当成已送达。
    Closed,
}

#[utoipa::path(
    get,
    path = "/api/v1/events/product",
    operation_id = "studio.subscribeProduct",
    responses(StudioApiErrors, (status = 200, description = "Product event stream", body = String, content_type = "text/event-stream"))
)]
pub(crate) async fn product_events(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let permit = state
        .streams
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::overloaded())?;
    let mut events = state.runtime.subscribe_product();
    let shutdown = state.shutdown.clone();
    let has_cursor = headers.contains_key("last-event-id");
    let (sender, receiver) = mpsc::channel(64);
    tokio::spawn(async move {
        let _permit = permit;
        if has_cursor
            && !flush_stale(
                &sender,
                &shutdown,
                StaleEvent {
                    reason: "replayUnsupported",
                    dropped: None,
                    resync: "/api/v1/state",
                },
            )
            .await
        {
            return;
        }
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                event = events.recv() => match event {
                    Ok(event) => {
                        match try_send_json(&sender, "event", Some(&event.event_id), &event) {
                            Delivery::Sent => {}
                            Delivery::Stalled => {
                                if !flush_stale(
                                    &sender,
                                    &shutdown,
                                    StaleEvent {
                                        reason: "slowConsumer",
                                        dropped: None,
                                        resync: "/api/v1/state",
                                    },
                                )
                                .await
                                {
                                    return;
                                }
                            }
                            Delivery::Closed => return,
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped)) => {
                        if !flush_stale(
                            &sender,
                            &shutdown,
                            StaleEvent {
                                reason: "lagged",
                                dropped: Some(dropped),
                                resync: "/api/v1/state",
                            },
                        )
                        .await
                        {
                            return;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
        // 终态标记是尽力而为：shutdown 必须立即结束连接，不能用等待容量阻塞优雅关闭。
        let _ = sender.try_send(Ok(Event::default().event("closed").data("{}")));
    });
    Ok(Sse::new(ReceiverStream::new(receiver))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("")))
}

/// 状态订阅：只转发当前执行、活动、交互与运行时状态。
///
/// 内容**不**经本流传送，也不再用一条“窗口版本已变化”的通知让客户端去重读历史页：内容窗口有自己的
/// 实时通道 `/api/v1/threads/{id}/window/events`（初始 `/window`、分页 `?anchor=`、按身份读
/// `/window/items/{id}`），与 FRB 的 `BridgeChatView` 用同一份共享会话事实。旧
/// `itemStarted`/`itemDelta`/`itemCompleted` 事件已删除——内容只有窗口这一条推送源。
#[utoipa::path(
    get,
    path = "/api/v1/threads/{thread_id}/events",
    operation_id = "thread.subscribe",
    params(("thread_id" = String, Path, description = "Thread ID")),
    responses(StudioApiErrors, (status = 200, description = "Authoritative Thread stream", body = String, content_type = "text/event-stream"))
)]
pub(crate) async fn thread_events(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let permit = state
        .streams
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::overloaded())?;
    let mut events = state
        .runtime
        .subscribe_thread(ThreadSubscriptionRequest {
            thread_id: thread_id.clone(),
        })
        .await
        .map_err(ApiError::from)?;
    let residency_pin = state.runtime.pin_thread(&thread_id);
    let shutdown = state.shutdown.clone();
    let has_cursor = headers.contains_key("last-event-id");
    let (sender, receiver) = mpsc::channel(128);
    tokio::spawn(async move {
        let _permit = permit;
        let _residency_pin = residency_pin;
        if has_cursor
            && !send_json(
                &sender,
                "stale",
                None,
                &StaleEvent {
                    reason: "replayUnsupported",
                    dropped: None,
                    resync: "resubscribe",
                },
            )
            .await
        {
            return;
        }
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = sender.closed() => break,
                update = events.recv() => {
                    let update = match update {
                        Ok(Some(update)) => update,
                        Ok(None) => break,
                        Err(error) => {
                            send_json(
                                &sender,
                                "error",
                                None,
                                &serde_json::json!({"message": error.to_string()}),
                            )
                            .await;
                            break;
                        }
                    };
                    let (event_name, event_id) = match &update {
                        ThreadSubscriptionUpdate::Snapshot { snapshot } => (
                            "snapshot",
                            Some(format!("thread:{}", snapshot.revision)),
                        ),
                        ThreadSubscriptionUpdate::Notification { notification } => {
                            let name = if matches!(notification.notification, ThreadNotification::Lagged { .. }) {
                                "lagged"
                            } else {
                                "notification"
                            };
                            // 事件 id 携带 stream epoch 与 revision；客户端据此判断缺口，
                            // 重连时 SSE 仍然只发 `stale`，由数据库窗口重同步。
                            (
                                name,
                                Some(format!(
                                    "thread:{}:{}",
                                    notification.epoch, notification.revision
                                )),
                            )
                        }
                    };
                    if !send_json(&sender, event_name, event_id.as_deref(), &update).await {
                        return;
                    }
                }
            }
        }
        let _ = sender.try_send(Ok(Event::default().event("closed").data("{}")));
    });
    Ok(Sse::new(ReceiverStream::new(receiver))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("")))
}

/// 内容窗口订阅：每条连接持有**自己**的有界 `ChatView`。
///
/// 按 `anchor` + `direction` 建立窗口（`anchor` 缺省为最新窗口；`anchor` = 当前窗口首条 +
/// `direction=older` 是上翻一页，末条 + `newer` 是下翻一页，由 core 的窗口分页推进）后先发一次权威
/// `reset`（整窗快照），之后转发该窗口自己的差量：同身份字段增长交付 `content` 增量、单格替换交付
/// `splice`，无法用差量表达（含窗口镜像与共享会话失同步）时回退成权威 `reset`。换锚点/方向即换
/// 窗口，客户端重新连接就得到新的 `reset`，因此不存在跨窗口的增量拼接。窗口版本是窗口自己的版本，
/// 与 `/events` 状态流的 envelope revision 相互独立。
#[utoipa::path(
    get,
    path = "/api/v1/threads/{thread_id}/window/events",
    operation_id = "thread.subscribeWindow",
    params(
        ("thread_id" = String, Path, description = "Thread ID"),
        ("anchor" = Option<String>, Query, description = "canonical item identity；缺省或 latest 为最新窗口"),
        ("direction" = Option<String>, Query, description = "older/newer：相对 anchor 翻一页（缺省为跳到 anchor 的窗口）")
    ),
    responses(StudioApiErrors, (status = 200, description = "Live content window", body = String, content_type = "text/event-stream"))
)]
pub(crate) async fn thread_window_events(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    ApiQuery(query): ApiQuery<ChatWindowQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let permit = state
        .streams
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::overloaded())?;
    let runtime = state.runtime.clone();
    let mut stream = runtime
        .open_chat_window(&thread_id, &query)
        .await
        .map_err(ApiError::from)?;
    let initial = stream.initial().map_err(ApiError::from)?;
    let shutdown = state.shutdown.clone();
    let (sender, receiver) = mpsc::channel(128);
    tokio::spawn(async move {
        let _permit = permit;
        if !send_window(&sender, ChatWindowUpdate::Reset { window: initial }).await {
            return;
        }
        // `stream` 只在 select 表达式里被借用；结果拿到后再赋值，避免与 `select!` 的 future 借用冲突。
        enum Step {
            Update(ChatWindowUpdate),
            Failed(anyhow::Error),
            End,
        }
        loop {
            let step = tokio::select! {
                _ = shutdown.cancelled() => Step::End,
                _ = sender.closed() => Step::End,
                update = stream.next() => match update {
                    Ok(Some(update)) => Step::Update(update),
                    Ok(None) => Step::End,
                    Err(error) => Step::Failed(error),
                }
            };
            match step {
                Step::Update(update) => {
                    if !send_window(&sender, update).await {
                        return;
                    }
                }
                Step::End => break,
                Step::Failed(error) => {
                    // 权威重同步：重新打开同一个窗口并重发一次 `reset`，客户端直接替换本地窗口，
                    // 不做跨版本拼接。
                    tracing::debug!(thread_id, %error, "content window stream resynchronized");
                    let Ok(fresh) = runtime.open_chat_window(&thread_id, &query).await else {
                        break;
                    };
                    let Ok(window) = fresh.initial() else {
                        break;
                    };
                    stream = fresh;
                    if !send_window(&sender, ChatWindowUpdate::Reset { window }).await {
                        return;
                    }
                }
            }
        }
        let _ = sender.try_send(Ok(Event::default().event("closed").data("{}")));
    });
    Ok(Sse::new(ReceiverStream::new(receiver))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("")))
}

/// 无损送达一次窗口更新；事件 id 携带窗口版本，客户端据此判断本地窗口是否还能继续套用差量。
async fn send_window(
    sender: &mpsc::Sender<Result<Event, Infallible>>,
    update: ChatWindowUpdate,
) -> bool {
    let event_id = match &update {
        ChatWindowUpdate::Reset { window } => format!("window:{}", window.version),
        ChatWindowUpdate::Patch { to, .. } => format!("window:{to}"),
    };
    send_json(sender, "window", Some(&event_id), &update).await
}

/// Lossless send: awaits channel capacity so transcript/turn terminal frames are never dropped.
async fn send_json(
    sender: &mpsc::Sender<Result<Event, Infallible>>,
    event_name: &'static str,
    event_id: Option<&str>,
    value: &impl Serialize,
) -> bool {
    let Ok(event) = encode_event(event_name, event_id, value) else {
        return false;
    };
    sender.send(Ok(event)).await.is_ok()
}

/// Best-effort send for coalescable product updates: reports a full buffer instead of dropping
/// silently, so the caller can deliver one `stale` marker and keep the stream alive.
fn try_send_json(
    sender: &mpsc::Sender<Result<Event, Infallible>>,
    event_name: &'static str,
    event_id: Option<&str>,
    value: &impl Serialize,
) -> Delivery {
    let Ok(event) = encode_event(event_name, event_id, value) else {
        return Delivery::Closed;
    };
    match sender.try_send(Ok(event)) {
        Ok(()) => Delivery::Sent,
        Err(mpsc::error::TrySendError::Full(_)) => Delivery::Stalled,
        Err(mpsc::error::TrySendError::Closed(_)) => Delivery::Closed,
    }
}

/// 无损送达一次 `stale`：等待缓冲容量，因此慢消费者一定收到重同步指令而不是被静默断开。
///
/// 返回 `false` 表示接收端已关闭或事件编码失败，调用方应结束该流。
async fn flush_stale(
    sender: &mpsc::Sender<Result<Event, Infallible>>,
    shutdown: &CancellationToken,
    stale: StaleEvent,
) -> bool {
    let permit = tokio::select! {
        _ = shutdown.cancelled() => return false,
        permit = sender.reserve() => match permit {
            Ok(permit) => permit,
            Err(_) => return false,
        },
    };
    let Ok(event) = encode_event("stale", None, &stale) else {
        return false;
    };
    // 已持有保留容量：`reserve()` 成功即保证这次 `send` 不会失败（接收端关闭会在上面的
    // `reserve()` 分支就已经返回 false），因此这里返回 true 是真实送达，而不是乐观假设。
    permit.send(Ok(event));
    true
}

fn encode_event(
    event_name: &'static str,
    event_id: Option<&str>,
    value: &impl Serialize,
) -> Result<Event, axum::Error> {
    let mut event = Event::default().event(event_name);
    if let Some(event_id) = event_id {
        event = event.id(event_id);
    }
    event.json_data(value)
}
