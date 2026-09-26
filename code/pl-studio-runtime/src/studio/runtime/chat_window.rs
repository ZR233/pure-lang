//! HTTP 与 FRB 共用的**内容窗口**：共享 `ChatSession` 上的实时 ChatView。
//!
//! 内容窗口与 `/timeline`（纯 SQL 的 durable history 分页）职责不同：窗口读的是共享会话的实时
//! 投影，因此既有已落盘条目，也有尚未落盘的流式条目；窗口版本是窗口自己的版本，不是状态流水位。
//!
//! 每个消费者（FRB 的 `BridgeChatView`、HTTP 的一条 SSE 连接）持有**自己**的 `ChatView`：窗口只
//! 覆盖当前焦点附近的有界条目，换锚点/方向即换窗口并重发一次权威 `Reset`。这里不引入全局窗口注册表、
//! 轮询或第二份正文缓存：条目正文始终由共享会话拥有，本模块只做「core typed 差量 → wire 差量」的
//! 有界映射，并且是这条映射的**唯一实现**（FRB 只把结果映射成 DTO，见 bridge 的
//! `api/studio/convert/chat_window.rs`）。
//!
//! 内容变化直接消费 core 的 [`pl_core::chat::ViewChange::UpdateItem`]：一个 `expectedRevision` /
//! `revision` 提交点，加上按字段身份的 `Append`/`Replace`/`Remove`。这里**没有** JSON 差量、没有前值
//! 索引、也没有整条 JSON 的字符串追加——正文只在交付边界用 [`chat_item::canonical_item`] 组装成
//! canonical 条目。
//!
//! 基线规则只有一条：本窗口对外交付的 `Reset` 版本**必须**等于之后差量的 `from`。因此任何“有效基线
//! 重置”（换焦点/方向、按需重读当前窗口、镜像失同步）都重新 `subscribe()`，让 core 的差量基线与
//! 交付出去的窗口是同一次读取。

use anyhow::Result;
use pl_core::chat::{
    ChatField, ChatFocus, ChatItem, ChatLifecycle, ChatSnapshot, ChatUpdate, ChatUpdates, ChatView,
    Direction, FieldChange, PresentationPart, ViewChange,
};
use pl_protocol::{
    ChatWindowChange, ChatWindowDirection, ChatWindowFocus, ChatWindowItem, ChatWindowLifecycle,
    ChatWindowPriority, ChatWindowQuery, ChatWindowSnapshot, ChatWindowUpdate, ThreadContentField,
    ThreadFieldChange, ThreadFieldUpdate,
};

use super::StudioRuntime;
use super::chat_item::{TOOL_ARGUMENTS_FIELD, TOOL_RESULT_FIELD, canonical_item};

/// 同一个 view 的**锁外**句柄：按身份读取不经过窗口锁，也不参与窗口差量基线。
///
/// 它只共享这条窗口自己的 `ChatView`（同一个 `Session`），因此读取的是与窗口完全相同的事实来源：
/// 不重新 `open_chat`、不按 thread id 另找运行时实例，窗口句柄关闭后也不会读到别的实例的条目。
pub struct ChatWindowHandle {
    view: ChatView,
}

impl ChatWindowHandle {
    /// 这条 view 当前的窗口版本（同步、廉价，不构快照/差量、不等待）。
    pub fn version(&self) -> u64 {
        self.view.version()
    }

    /// **锁外** readiness 等待：等这条 view 出现需要交付的变化，返回可取的版本。
    ///
    /// 只读版本与边界水位两个数，不构快照、不构差量、不持有任何窗口锁；`seen` 必须是消费者自己的
    /// **已看过**水位（[`ChatWindowStream::observed_version`]），否则已被跳过的版本会让等待反复立即
    /// 返回。可取消安全：等待跑在私有的 watch 句柄上，future 在变化前后被取消都不消耗版本。`None`
    /// 只表示会话不再变化（通道关闭）。
    pub async fn wait_change(&self, seen: u64) -> Option<u64> {
        self.view.watch().wait(seen).await
    }

    /// 按 identity 读一条条目：同 view 内存优先（未落盘的流式条目同样可读）。
    ///
    /// 只读、不推进窗口差量基线，也不参与帧等待；因此一个挂起的帧等待不会挡住按身份读取正文。
    pub async fn read_item(&self, item_id: &str) -> Result<Option<ChatWindowItem>> {
        self.view
            .read_item(item_id)
            .await
            .map_err(anyhow::Error::new)?
            .map(window_item)
            .transpose()
    }

    /// **锁外**请求一条完整正文：同一 authoritative 窗口/revision，进行中的条目走内存，历史条目至多
    /// 冷读一次并在该 view 内保留，直到身份离开窗口。
    ///
    /// 它只是把完整正文**请求进这条 view**（core 的 retain 绑定到本 view 且只在身份仍在窗口时生效），
    /// 不重新打开窗口、不另查 SQL、也不推进窗口版本。调用方随后应当在**短临界区**里
    /// [`ChatWindowStream::resync_snapshot`] 取得包含完整正文的权威窗口，并让后续 `Patch.from` 与之
    /// 对齐——core `read_complete` 本身不 bump 版本也不通知（见
    /// `/tmp/anywork-realtime-impl/activity-typed-contract.md` 的 core 依赖）。
    ///
    /// 取消/身份已离开窗口都不会把结果写回窗口：core 的 retain 只在身份仍在本窗口时保留。
    pub async fn request_complete(&self, item_id: &str) -> Result<Option<ChatWindowItem>> {
        self.view
            .read_complete(item_id)
            .await
            .map_err(anyhow::Error::new)?
            .map(window_item)
            .transpose()
    }
}

/// 一条 HTTP 连接或一个 FRB 窗口自有的内容窗口。
pub struct ChatWindowStream {
    thread_id: String,
    view: ChatView,
    updates: ChatUpdates,
    /// 当前**订阅读取**的窗口快照。
    ///
    /// `initial()` 返回它而不是重新 `snapshot()`，这样对外交付的窗口版本与后续差量的 `from` 一定是
    /// 同一个版本。任何订阅读取（建立窗口、换锚点/方向、权威重同步）都经 [`Self::resubscribe`]。
    baseline: ChatSnapshot,
}

impl ChatWindowStream {
    /// 这条窗口**已经交付给消费者**的版本。
    ///
    /// 直接读 core `ChatUpdates` 自己维护的交付水位（`take()` 交付一帧时原子推进），不再由本结构另存
    /// 一份同样的数字，避免两份语义混用。
    pub fn delivered_version(&self) -> u64 {
        self.updates.delivered_version()
    }

    /// 本消费者**已看过**的唤醒水位：每个新版本都会推进（交付或被跳过）。
    ///
    /// 它是锁外 [`ChatWindowHandle::wait_change`] 的入参，而不是交付水位——传交付水位会让等待对已被
    /// 跳过的版本反复立即返回。
    pub fn observed_version(&self) -> u64 {
        self.updates.observed_version()
    }

    /// 同一 view 的锁外句柄：按身份读取不经过窗口锁，也不推进差量基线。
    pub fn handle(&self) -> ChatWindowHandle {
        ChatWindowHandle {
            view: self.view.clone(),
        }
    }

    /// 订阅基线的权威快照：这是建立/重建窗口后交付的首帧，也是随后差量的 `from`。
    pub fn initial(&self) -> Result<ChatWindowSnapshot> {
        window_snapshot(&self.thread_id, self.baseline.clone())
    }

    /// 等待下一次窗口变化。
    ///
    /// 「锁外 readiness 等待 + 短临界区同步取帧」：等待只看版本与边界水位（首字、终态准入、保存确认、
    /// 字段删除、权威替换、结构变化立即就绪；普通已开始文本按一个固定合帧窗口），随后一次同步 `take`
    /// 只相对**本消费者已交付基线**算一次差量并原子推进两个水位。`None` 表示会话真的结束。
    pub async fn next(&mut self) -> Result<Option<ChatWindowUpdate>> {
        loop {
            let seen = self.updates.observed_version();
            if self.handle().wait_change(seen).await.is_none() {
                return Ok(None);
            }
            if let Some(update) = self.take()? {
                return Ok(Some(update));
            }
            // 只推进了 observed、本窗口看不到的变化（无关 live 更新）或已被控制操作交付：回去锁外等待。
        }
    }

    /// 短临界区里的一次同步取帧：不 await，只算一次差量。
    ///
    /// `None` 表示这次唤醒没有本窗口可见的变化（只推进了已看过水位，交付基线不动），调用方应回到
    /// [`ChatWindowHandle::wait_change`] 继续等待而不是热转。
    pub fn take(&mut self) -> Result<Option<ChatWindowUpdate>> {
        let Some(update) = self.updates.take() else {
            return Ok(None);
        };
        let thread_id = self.thread_id.clone();
        Ok(Some(self.convert(&thread_id, update)?))
    }

    /// 按请求（锚点 + 方向）重建窗口；返回新的权威窗口快照。
    pub async fn focus(&mut self, query: &ChatWindowQuery) -> Result<ChatWindowSnapshot> {
        self.view
            .focus(focus_of(query.anchor.as_deref()))
            .await
            .map_err(anyhow::Error::new)?;
        if let Some(direction) = query.direction {
            self.view
                .load(core_direction(direction))
                .await
                .map_err(anyhow::Error::new)?;
        }
        self.resubscribe();
        self.initial()
    }

    /// 从当前窗口继续向指定方向翻一页。
    pub async fn load(&mut self, direction: ChatWindowDirection) -> Result<ChatWindowSnapshot> {
        self.view
            .load(core_direction(direction))
            .await
            .map_err(anyhow::Error::new)?;
        self.resubscribe();
        self.initial()
    }

    /// 读取当前窗口，并把它变成新的订阅基线。
    pub async fn snapshot(&mut self) -> Result<ChatWindowSnapshot> {
        self.resubscribe();
        self.initial()
    }

    /// 按 identity 读一条条目（同 view，内存优先）。
    pub async fn read_item(&self, item_id: &str) -> Result<Option<ChatWindowItem>> {
        self.handle().read_item(item_id).await
    }

    /// 同一 view 的窗口操作：重新订阅当前窗口并返回与订阅基线一致的权威快照。
    ///
    /// 用在「请求完整正文」之后：core `read_complete` 把完整正文保留进这条 view 后，重新订阅会让权威
    /// 窗口（含完整正文）成为新的订阅基线，因此随后差量的 `from` 与它对齐。**不虚构版本**：返回的
    /// 版本就是 core 订阅读取到的窗口版本。
    pub fn resync_snapshot(&mut self) -> Result<ChatWindowSnapshot> {
        self.resubscribe();
        self.initial()
    }

    /// 同一 view 的「展开完整正文」窗口操作：请求完整正文（可能冷读一次），再返回与之基线一致的权威
    /// 窗口快照。单任务消费者（HTTP 的一条 SSE 连接）可直接用它；持锁消费者用
    /// [`ChatWindowHandle::request_complete`] + [`Self::resync_snapshot`] 把等待移到锁外。
    pub async fn expand(&mut self, item_id: &str) -> Result<ChatWindowSnapshot> {
        // 请求把完整正文保留进这条 view；权威窗口在下面统一交出。
        let _ = self
            .view
            .read_complete(item_id)
            .await
            .map_err(anyhow::Error::new)?;
        self.resync_snapshot()
    }

    /// 把一次 core typed 差量映射成 wire 差量。
    fn convert(&mut self, thread_id: &str, update: ChatUpdate) -> Result<ChatWindowUpdate> {
        Ok(match update {
            ChatUpdate::Reset(window) => {
                self.baseline = window.clone();
                ChatWindowUpdate::Reset {
                    window: window_snapshot(thread_id, window)?,
                }
            }
            ChatUpdate::Patch {
                from,
                to,
                changes,
                has_newer,
                priority,
            } => {
                let mut converted = Vec::with_capacity(changes.len());
                for change in changes {
                    match change {
                        ViewChange::Splice {
                            index,
                            remove,
                            items,
                        } => converted.push(ChatWindowChange::Splice {
                            index: u64::try_from(index)
                                .map_err(|_| anyhow::anyhow!("window index overflow"))?,
                            remove: u64::try_from(remove)
                                .map_err(|_| anyhow::anyhow!("window splice overflow"))?,
                            items: items.into_iter().map(window_item).collect::<Result<_>>()?,
                        }),
                        ViewChange::UpdateItem {
                            item_id,
                            expected_revision,
                            revision,
                            omitted_bytes,
                            saved,
                            fields,
                        } => {
                            let Some(fields) = field_updates(&fields) else {
                                // 出现了宿主没有 wire 字段域的 core 字段：用权威 Reset 收束，
                                // 绝不静默丢弃结构化数据。
                                return self.resync();
                            };
                            converted.push(ChatWindowChange::UpdateItem {
                                item_id,
                                expected_revision,
                                revision,
                                omitted_bytes,
                                saved,
                                fields,
                            });
                        }
                    }
                }
                ChatWindowUpdate::Patch {
                    from,
                    to,
                    changes: converted,
                    has_newer,
                    priority: priority_of(priority),
                }
            }
        })
    }

    /// 有效基线重置：重新订阅，让 core 的差量与对外窗口来自同一次读取。
    fn resubscribe(&mut self) {
        let (baseline, updates) = self.view.subscribe();
        self.updates = updates;
        self.baseline = baseline;
    }

    /// 权威整窗重同步：重置基线并交付这一份窗口。
    fn resync(&mut self) -> Result<ChatWindowUpdate> {
        self.resubscribe();
        Ok(ChatWindowUpdate::Reset {
            window: self.initial()?,
        })
    }
}

impl StudioRuntime {
    /// 打开一个**内存优先**的内容窗口。
    ///
    /// 先按请求建立窗口（缺省/`latest` 是最新窗口；锚点 + 方向决定跳转或翻页），再订阅它。打开窗口会
    /// 像 GUI 首次打开一样读一次 history 填首窗，之后窗口只消费共享会话的实时事实，不再回源 SQL。
    ///
    /// # Errors
    /// 未知 Thread，或锚点不属于该 Thread 的 history。
    pub async fn open_chat_window(
        &self,
        thread_id: &str,
        query: &ChatWindowQuery,
    ) -> Result<ChatWindowStream> {
        let view = self
            .open_chat(thread_id, focus_of(query.anchor.as_deref()))
            .await?;
        if let Some(direction) = query.direction {
            view.load(core_direction(direction))
                .await
                .map_err(anyhow::Error::new)?;
        }
        let (baseline, updates) = view.subscribe();
        Ok(ChatWindowStream {
            thread_id: thread_id.to_owned(),
            view,
            updates,
            baseline,
        })
    }

    /// 按 identity 读一条窗口条目，读的是共享会话（内存优先），因此未落盘的流式条目同样可读。
    ///
    /// 这是内容窗口的「按身份读」入口；**完整**正文请走同 view 的窗口操作
    /// [`ChatWindowStream::expand`]（或 [`ChatWindowHandle::request_complete`] +
    /// [`ChatWindowStream::resync_snapshot`] 的两步形态）。
    ///
    /// # Errors
    /// 未知 Thread（不激活执行、也不读 durable 正文），或条目与其产品载荷不一致。
    pub async fn read_chat_window_item(
        &self,
        thread_id: &str,
        item_id: &str,
    ) -> Result<Option<ChatWindowItem>> {
        let item = self
            .chat_session(thread_id)
            .await?
            .read_item(item_id)
            .await
            .map_err(anyhow::Error::new)?;
        item.map(window_item).transpose()
    }
}

/// anchor → 窗口焦点；`latest`/缺省是最新窗口，其它值是 canonical item identity。
fn focus_of(anchor: Option<&str>) -> ChatFocus {
    match anchor {
        None | Some("") | Some("latest") => ChatFocus::Latest,
        Some(item_id) => ChatFocus::Around(item_id.to_owned()),
    }
}

fn core_direction(direction: ChatWindowDirection) -> Direction {
    match direction {
        ChatWindowDirection::Older => Direction::Older,
        ChatWindowDirection::Newer => Direction::Newer,
    }
}

fn priority_of(priority: pl_core::chat::ChatUpdatePriority) -> ChatWindowPriority {
    match priority {
        pl_core::chat::ChatUpdatePriority::Immediate => ChatWindowPriority::Immediate,
        pl_core::chat::ChatUpdatePriority::Coalesced => ChatWindowPriority::Coalesced,
    }
}

fn window_snapshot(thread_id: &str, window: ChatSnapshot) -> Result<ChatWindowSnapshot> {
    Ok(ChatWindowSnapshot {
        thread_id: thread_id.to_owned(),
        focus: match window.focus {
            ChatFocus::Latest => ChatWindowFocus::Latest,
            ChatFocus::Around(item_id) => ChatWindowFocus::Around { item_id },
        },
        version: window.version,
        items: window
            .items
            .into_iter()
            .map(window_item)
            .collect::<Result<_>>()?,
        has_older: window.has_older,
        has_newer: window.has_newer,
    })
}

/// 一条 core typed 条目 → wire 条目：结构化字段由 [`canonical_item`] 在交付边界如实组装。
fn window_item(item: ChatItem) -> Result<ChatWindowItem> {
    let lifecycle = match item.lifecycle {
        ChatLifecycle::Streaming => ChatWindowLifecycle::Streaming,
        ChatLifecycle::Terminal => ChatWindowLifecycle::Terminal,
    };
    Ok(ChatWindowItem {
        item: canonical_item(&item)?,
        saved: item.saved,
        lifecycle,
        omitted_bytes: item.omitted_bytes,
    })
}

/// core 字段身份 → wire 字段身份；出现宿主没有域映射的字段时返回 `None`（调用方改权威 Reset）。
fn wire_field(field: &ChatField) -> Option<ThreadContentField> {
    match field {
        ChatField::Body => Some(ThreadContentField::Text),
        ChatField::Part(PresentationPart::OutputText(_)) => Some(ThreadContentField::Text),
        ChatField::Part(PresentationPart::ReasoningText(index)) => {
            Some(ThreadContentField::ThinkingContent {
                chunk_index: *index,
            })
        }
        ChatField::Part(PresentationPart::SummaryText(index)) => {
            Some(ThreadContentField::ThinkingSummary {
                chunk_index: *index,
            })
        }
        ChatField::Host(key) if key.as_ref() == TOOL_ARGUMENTS_FIELD => {
            Some(ThreadContentField::ToolArguments)
        }
        ChatField::Host(key) if key.as_ref() == TOOL_RESULT_FIELD => {
            Some(ThreadContentField::ToolResult)
        }
        ChatField::Host(_) => None,
    }
}

/// 一批 core 字段变化 → wire 字段变化；任一字段无域映射即返回 `None`。
fn field_updates(fields: &[pl_core::chat::FieldUpdate]) -> Option<Vec<ThreadFieldUpdate>> {
    let mut updates = Vec::with_capacity(fields.len());
    for update in fields {
        let field = wire_field(&update.field)?;
        let change = match &update.change {
            FieldChange::Unchanged => ThreadFieldChange::Unchanged,
            FieldChange::Append(text) => ThreadFieldChange::Append { text: text.clone() },
            FieldChange::Replace(block) => ThreadFieldChange::Replace { text: block.text() },
            FieldChange::Remove => ThreadFieldChange::Remove,
        };
        updates.push(ThreadFieldUpdate { field, change });
    }
    Some(updates)
}
