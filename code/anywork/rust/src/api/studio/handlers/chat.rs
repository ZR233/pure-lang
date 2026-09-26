use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::chat_window;
use crate::api::studio::types::{
    BridgeChatDirection, BridgeChatFocus, BridgeChatItem, BridgeChatSnapshot, BridgeChatUpdate,
    BridgeError,
};
use pl_studio_runtime::{ChatWindowHandle, ChatWindowStream};

/// 一个独立定位、有界的 FRB 内容窗口。
///
/// 窗口差量、前值索引与基线一致性都由 runtime 的 `ChatWindowStream` 负责（HTTP 与 FRB 共用同一条
/// 映射）；这里只把 typed 结果映射成 FRB DTO。
///
/// `window: Mutex<ChatWindowStream>` 串行化所有接触窗口的操作（`snapshot`/`load`/`focus`/取帧），
/// 让窗口差量只有一个写入者；`handle: ChatWindowHandle` 是同一个 `ChatView`（同一个 `Session`）的
/// **锁外**句柄，按身份读正文走它，不经过窗口锁，因此一个挂起的帧等待不会挡住正文读取。
///
/// 帧等待也走**锁外**：`handle.wait_change(observed_version)` 只看版本与边界水位（不合帧、不构 diff、
/// 不持有窗口锁），随后一个**短临界区**的同步 `take` 只算一次差量并原子推进「已交付」与「已看过」两个
/// 水位。等待期间不持有窗口锁，因此一个挂起的取帧不会挡住 `load`/`focus`/读正文；`close` 靠 select 的
/// cancel 分支立刻结束等待。两个水位分别用 `observed_version()`（等待入参）与交付基线（`take` 的差量
/// 起点），不再用 `subscribe().next()` 冒充锁外等待。
pub struct BridgeChatView {
    /// 串行化所有接触窗口的操作；取帧只在短临界区里持锁（见结构体文档）。
    window: Mutex<ChatWindowStream>,
    /// 同一个 view 的锁外句柄：按身份读条目不经过窗口锁，也不参与窗口差量基线。
    handle: ChatWindowHandle,
    initial: BridgeChatSnapshot,
    cancel: CancellationToken,
}

pub async fn open_chat_view(
    thread_id: String,
    focus: BridgeChatFocus,
) -> Result<BridgeChatView, BridgeError> {
    let bridge = active_bridge().await?;
    let stream = bridge
        .studio
        .open_chat_window(&thread_id, &chat_window::query(focus))
        .await?;
    let handle = stream.handle();
    let initial = chat_window::snapshot(stream.initial()?)?;
    let cancel = bridge.shutdown.child_token();
    Ok(BridgeChatView {
        window: Mutex::new(stream),
        handle,
        initial,
        cancel,
    })
}

/// 视图已关闭：明确的生命周期错误，而不是让调用方永久挂起。
fn view_closed() -> BridgeError {
    pl_protocol::studio::StudioError::new(
        pl_protocol::studio::StudioErrorCode::Cancelled,
        "chat view is closed",
        false,
    )
    .into()
}

impl BridgeChatView {
    /// The subscription baseline is captured before returning the handle.
    pub fn initial(&self) -> BridgeChatSnapshot {
        self.initial.clone()
    }

    /// 当前窗口，并把订阅基线对齐到它：返回的快照版本一定等于随后差量的 `from`。
    pub async fn snapshot(&self) -> Result<BridgeChatSnapshot, BridgeError> {
        if self.cancel.is_cancelled() {
            return Err(view_closed());
        }
        let mut window = self.window.lock().await;
        chat_window::snapshot(window.snapshot().await?)
    }

    /// Waits for one consolidated batch, then returns control to the consumer.
    ///
    /// 「锁外 readiness 等待 + 短临界区同步取帧」：等待期间不持窗口锁（见结构体文档），`close` 立刻
    /// 打断等待。`Ok(None)` 只表示窗口流真的结束（会话不再变化或视图已关闭）。
    pub async fn next(&self) -> Result<Option<BridgeChatUpdate>, BridgeError> {
        loop {
            // 短临界区：只读本消费者「已看过」的唤醒水位，不构 diff、不等待。
            let seen = { self.window.lock().await.observed_version() };
            // 锁外 readiness 等待：只看版本与边界水位，cancel 可打断，不持有窗口锁。
            let ready = tokio::select! {
                biased;
                () = self.cancel.cancelled() => return Ok(None),
                ready = self.handle.wait_change(seen) => ready,
            };
            if ready.is_none() {
                return Ok(None);
            }
            // 短临界区：一次同步 take，只算一次差量并原子推进两个水位。
            let update = { self.window.lock().await.take()? };
            if let Some(update) = update {
                return chat_window::update(update).map(Some);
            }
            // 只推进 observed、本窗口看不到的变化，或已被控制操作交付：回到锁外等待。
        }
    }

    /// 按方向翻页：从**当前窗口**继续（core `ChatView::load` 换入一页、换出相反方向的一页），
    /// 因此每次调用都前进，也不会用同一个 item 反复拿到同一个居中窗口。
    ///
    /// `close` 会取消正在等待 history 的翻页，避免视图关闭后调用永久挂起。
    pub async fn load(
        &self,
        direction: BridgeChatDirection,
    ) -> Result<BridgeChatSnapshot, BridgeError> {
        if self.cancel.is_cancelled() {
            return Err(view_closed());
        }
        let mut window = self.window.lock().await;
        tokio::select! {
            biased;
            () = self.cancel.cancelled() => Err(view_closed()),
            result = window.load(chat_window::direction(direction)) => {
                Ok(chat_window::snapshot(result?)?)
            }
        }
    }

    pub async fn focus(&self, focus: BridgeChatFocus) -> Result<BridgeChatSnapshot, BridgeError> {
        if self.cancel.is_cancelled() {
            return Err(view_closed());
        }
        let query = chat_window::query(focus);
        let mut window = self.window.lock().await;
        tokio::select! {
            biased;
            () = self.cancel.cancelled() => Err(view_closed()),
            result = window.focus(&query) => Ok(chat_window::snapshot(result?)?),
        }
    }

    /// 按身份读一条完整条目（内存优先，必要时回落 durable history）。
    ///
    /// 它走的是**同一个 view** 的锁外句柄，读的是与窗口完全相同的共享会话事实：不重新 `open_chat`、
    /// 不按 thread id 另找运行时实例，窗口句柄关闭后也不会读到别的实例的条目。一个挂起的 `next`
    /// 同样不会挡住正文读取。
    pub async fn read_item(&self, item_id: String) -> Result<Option<BridgeChatItem>, BridgeError> {
        self.handle
            .read_item(&item_id)
            .await?
            .map(chat_window::item)
            .transpose()
    }

    /// 展开被有界窗口省略的条目：这是一次**同 view 的窗口操作**，不是另开窗口的只读查询。
    ///
    /// 步骤：锁外请求该身份的**完整**正文（同 view/revision，历史条目至多冷读一次并保留到身份离开
    /// 窗口；被取消不会写回），再在短临界区里重新订阅这条窗口并把**权威窗口快照**交出去——返回的快照
    /// 版本就是后续 `Patch.from` 的基线，因此 Dart 直接把它当成一次的 `Reset` 应用即可，不需要自己
    /// 覆一份 item 或维持另一份 baseline（据此删除 `_completedBodies` 覆盖层）。
    pub async fn expand(&self, item_id: String) -> Result<BridgeChatSnapshot, BridgeError> {
        if self.cancel.is_cancelled() {
            return Err(view_closed());
        }
        tokio::select! {
            biased;
            () = self.cancel.cancelled() => return Err(view_closed()),
            result = self.handle.request_complete(&item_id) => {
                // 完整正文已请求进这条 view；返回值本身不直接交给 UI，权威窗口在下面统一交出。
                let _ = result?;
            }
        }
        let mut window = self.window.lock().await;
        chat_window::snapshot(window.resync_snapshot()?)
    }

    /// 结束这个窗口：取消锁内的帧等待、打断正在 await history 的翻页/换焦点。
    pub fn close(&self) {
        self.cancel.cancel();
    }
}

impl Drop for BridgeChatView {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
