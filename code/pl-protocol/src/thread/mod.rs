pub mod mode;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    InteractionRequest, McpHealthSnapshot, RuntimeCostAmount, ThreadItem, ThreadModeId,
    TodoListSnapshot, Turn,
};

pub const THREAD_SCHEMA_VERSION: u32 = 14;

/// Timeline 游标 token 的版本号。
///
/// 读取水位、数据库身份或 ordinal 语义发生变化时递增；客户端持有的旧版本游标会被
/// 明确拒绝并要求回到数据库重新读取，而不是回落到猜测位置。
pub const TIMELINE_CURSOR_VERSION: u32 = 1;

/// Timeline 游标的 opaque 前缀；item identity 不会以它开头，因此解码是确定性的。
const TIMELINE_CURSOR_PREFIX: &str = "tlc1.";

/// 会话级工作区模式：Project 根目录，或从 Project Git 仓库 `HEAD` 派生的独立 checkout。
///
/// 它是 Thread 的 canonical 产品事实，在创建会话时确定；与 Profile 的
/// [`crate::AgentWorkspaceMode`] 是两条语义轴：本模式决定会话工作区在哪里，
/// Profile 模式决定 child 相对会话工作区的隔离方式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ThreadWorkspaceMode {
    /// 使用 Project 根目录（默认）。
    #[default]
    Local,
    /// 从 Project 的 Git 仓库 `HEAD` 派生独立 worktree 并在其中工作。
    Worktree,
}

impl ThreadWorkspaceMode {
    /// Canonical 稳定标签，也是 `threads.workspace_mode` 的持久化值。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Worktree => "worktree",
        }
    }

    /// 解析 canonical 标签；未知标签明确失败，不默认为 `local`。
    pub fn from_label(label: &str) -> Result<Self, crate::UnknownLabelError> {
        match label {
            "local" => Ok(Self::Local),
            "worktree" => Ok(Self::Worktree),
            other => Err(crate::UnknownLabelError::new("ThreadWorkspaceMode", other)),
        }
    }
}

/// 一个 agent 独占的对话和执行队列。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub mode: ThreadModeId,
    /// 会话工作区模式；缺失的旧编码按默认 `local` 解码。
    #[serde(default)]
    pub workspace_mode: ThreadWorkspaceMode,
    /// 会话对外唯一 canonical 工作区地址；缺失的旧编码按空串解码。
    #[serde(default)]
    pub workspace_path: String,
    pub root_thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_thread_id: Option<String>,
    pub role: String,
    pub agent_path: String,
    pub status: ThreadStatus,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub archived: bool,
}

impl Thread {
    pub fn placeholder(id: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            project_id: String::new(),
            title: String::new(),
            mode: ThreadModeId::simple(),
            workspace_mode: ThreadWorkspaceMode::Local,
            workspace_path: String::new(),
            root_thread_id: id.clone(),
            parent_thread_id: None,
            role: String::new(),
            agent_path: String::new(),
            status: ThreadStatus::Idle,
            created_at: 0,
            updated_at: 0,
            archived: false,
            id,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ThreadStatus {
    Idle,
    Queued,
    Running,
    WaitingTool,
    WaitingInteraction,
    Cancelling,
    Closing,
    Closed,
    Faulted,
}

/// Timeline 记录是否仍属于后续模型的有效上下文。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ThreadContextDisposition {
    #[default]
    Active,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSnapshot {
    pub schema_version: u32,
    pub revision: u64,
    pub thread: Thread,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_turn: Option<Turn>,
    #[serde(default)]
    pub interactions: Vec<InteractionRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<ThreadRuntimeSnapshot>,
    /// 当前执行活动的权威小摘要；`None` 表示当前没有活动。
    ///
    /// 它是独立于 ChatView 历史窗口的 typed 投影：窗口只决定展示哪些历史条目，活动只描述当前
    /// 在做什么。完整正文不在快照里，按活动身份按需读取。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<crate::ThreadActivity>,
    /// Thread 的存储状态；`None` 表示该投影没有可报告的存储事实（不是“健康”）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<crate::ThreadStorageState>,
}

impl ThreadSnapshot {
    pub fn empty(thread_id: impl Into<String>) -> Self {
        Self {
            schema_version: THREAD_SCHEMA_VERSION,
            revision: 0,
            thread: Thread::placeholder(thread_id),
            active_turn: None,
            interactions: Vec::new(),
            runtime: None,
            activity: None,
            storage: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRuntimeSnapshot {
    pub thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_route: Option<ThreadModelRouteSnapshot>,
    pub usage: ThreadRuntimeUsage,
    #[serde(default)]
    pub turn_completion_tokens: u64,
    #[serde(default)]
    pub turn_decode_millis: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub todo: Option<TodoListSnapshot>,
    #[serde(default)]
    pub active_skills: Vec<String>,
    #[serde(default)]
    pub active_mcp_servers: Vec<String>,
    #[serde(default)]
    pub active_lsp_servers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_health: Option<McpHealthSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<crate::WorkflowRuntimeSnapshot>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadModelRouteSnapshot {
    pub provider_id: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    pub revision: u64,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRuntimeUsage {
    #[serde(default)]
    pub has_incomplete_usage: bool,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    pub latest_context_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub inference_count: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub cache_usage: CacheUsageSummary,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub estimated_costs: Vec<RuntimeCostAmount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub estimated_cache_savings: Vec<RuntimeCostAmount>,
    #[serde(default)]
    pub has_unpriced_usage: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix_changed_reason: Option<crate::PromptPrefixChangedReason>,
    pub updated_at: i64,
}

/// 由已报告输入样本累计出的 prompt cache 有效用量。
///
/// 只有 input 与 cache read 同时报告、且 cache read（连同可选的 cache
/// write）不超过 input 的样本才计入累计；缺失或矛盾的样本只置
/// `has_incomplete_usage`，不改变累计值。命中率分母是累计 input，因为它
/// 已经包含 cache read，未知不等于真实零命中。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CacheUsageSummary {
    /// 计入累计的有效样本 input 总量，已包含 cache read。
    #[serde(default)]
    pub input_tokens: u64,
    /// 计入累计的有效样本 cache read 总量。
    #[serde(default)]
    pub cache_read_tokens: u64,
    /// `cache_read_tokens / input_tokens`；累计 input 为零时为 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_rate: Option<f64>,
    /// 是否存在被排除在累计之外的缺失或矛盾样本。
    #[serde(default)]
    pub has_incomplete_usage: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum ThreadNotification {
    TurnStarted {
        turn: Turn,
    },
    TurnUpdated {
        turn: Turn,
    },
    TurnCompleted {
        turn: Turn,
    },
    InteractionChanged {
        interaction: Box<InteractionRequest>,
    },
    ThreadRuntimeUpdated {
        runtime: Box<ThreadRuntimeSnapshot>,
    },
    /// 当前执行活动变化；`None` 表示当前没有活动（Turn 已结束或尚未开始）。
    ///
    /// 帧只在活动实际变化时发出，并只携带小型 typed 摘要与身份；完整正文按活动身份按需读取。
    ActivityChanged {
        activity: Option<Box<crate::ThreadActivity>>,
    },
    /// Thread 存储状态变化；`None` 表示当前没有可报告的存储事实（未知，不是“健康”）。
    ///
    /// 帧只在存储状态实际变化时发出，携带 typed 故障类别、代数、水位与恢复阶段——故障类别来自
    /// 协调器的 typed 值，绝不从错误文本推断。快照首帧已带权威存储状态，后续帧只在它变化时补发，
    /// 因此正文更新不会额外走一遍状态流。
    StorageChanged {
        storage: Option<Box<crate::ThreadStorageState>>,
    },
    Lagged {
        dropped: u64,
    },
}

/// 一条实时增量通知的封套。
///
/// `epoch` 是生产端一次连续广播生命周期的标识：重订阅、owner 重建或数据库重同步后
/// 递增，客户端据此丢弃旧生命周期的迟到帧。`base_revision` 是本通知之前的状态水位，
/// `revision` 是应用本通知之后的状态水位；`base_revision != 上一条 revision`（或两帧间
/// `revision` 出现跳变）即表示缺口，客户端必须重同步而不是继续拼接增量。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadNotificationEnvelope {
    pub thread_id: String,
    pub epoch: u64,
    pub base_revision: u64,
    pub revision: u64,
    pub emitted_at: i64,
    pub notification: ThreadNotification,
}

impl ThreadNotificationEnvelope {
    /// 用生产端水位构造通知；`emitted_at` 由调用方的时钟提供。
    pub fn new(
        thread_id: impl Into<String>,
        epoch: u64,
        base_revision: u64,
        revision: u64,
        emitted_at: i64,
        notification: ThreadNotification,
    ) -> Self {
        Self {
            thread_id: thread_id.into(),
            epoch,
            base_revision,
            revision,
            emitted_at,
            notification,
        }
    }

    /// 本通知是否可由客户端接在上一条 `revision` 之后拼接。
    ///
    /// 同 epoch 且 `base_revision` 恰好等于客户端已知水位才连续；否则是缺口。
    pub fn continues_from(&self, epoch: u64, last_revision: u64) -> bool {
        self.epoch == epoch && self.base_revision == last_revision
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum ThreadSubscriptionUpdate {
    Snapshot {
        snapshot: Box<ThreadSnapshot>,
    },
    Notification {
        notification: Box<ThreadNotificationEnvelope>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSubscriptionRequest {
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadTurnPage {
    pub turns: Vec<ThreadTurnHistory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadTurnHistory {
    pub turn: Turn,
    pub items: Vec<ThreadItem>,
    #[serde(default)]
    pub context_disposition: ThreadContextDisposition,
}

/// Stable item query; before/after exclude the cursor, around includes it.
///
/// `item_id` 既接受 canonical item identity，也接受 `TimelineCursor` 的 opaque token：
/// 分页返回的 `older_cursor`/`newer_cursor` 必须原样传回，显式锚点（`around`）可以用
/// item identity。两类输入都只读数据库，不激活或 flush owner。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum TimelineQuery {
    Latest,
    Before { item_id: String },
    After { item_id: String },
    Around { item_id: String },
}

/// 版本化、自校验的 Timeline 游标。
///
/// 游标绑定 Thread 身份、history 数据库身份、条目 ordinal、条目 identity 与读取时的
/// applied write sequence。任何一项在当前数据库上不成立（Thread 不符、数据库被重建、
/// 水位回退）都必须拒绝，避免把游标当成"附近某条"的猜测位置。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimelineCursor {
    pub version: u32,
    pub thread_id: String,
    pub database_id: String,
    pub ordinal: u64,
    pub item_id: String,
    pub applied_write_sequence: u64,
}

impl TimelineCursor {
    /// 构造当前版本游标。
    pub fn new(
        thread_id: impl Into<String>,
        database_id: impl Into<String>,
        ordinal: u64,
        item_id: impl Into<String>,
        applied_write_sequence: u64,
    ) -> Self {
        Self {
            version: TIMELINE_CURSOR_VERSION,
            thread_id: thread_id.into(),
            database_id: database_id.into(),
            ordinal,
            item_id: item_id.into(),
            applied_write_sequence,
        }
    }

    /// 编码为可跨 wire 传输的 opaque token。
    ///
    /// # Panics
    /// 仅在内部 JSON 编码失败时 panic；字段均为可序列化的原始类型。
    pub fn encode(&self) -> String {
        let payload = serde_json::to_string(self).expect("timeline cursor is serializable");
        format!("{TIMELINE_CURSOR_PREFIX}{payload}")
    }

    /// 解析 opaque token；非游标输入（例如原始 item identity）返回 `None`。
    pub fn decode(token: &str) -> Option<Self> {
        let payload = token.strip_prefix(TIMELINE_CURSOR_PREFIX)?;
        let cursor: Self = serde_json::from_str(payload).ok()?;
        (cursor.version == TIMELINE_CURSOR_VERSION).then_some(cursor)
    }
}

/// An inclusive item range at one canonical commit watermark.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimelinePage {
    pub thread_id: String,
    /// 该页来自哪个 history 数据库实体；数据库重建后此值变化。
    pub database_id: String,
    /// 读取水位：构造本页时的 applied write sequence。
    pub watermark: u64,
    pub items: Vec<ThreadItem>,
    pub older_cursor: Option<String>,
    pub newer_cursor: Option<String>,
    pub first_item_id: Option<String>,
    pub last_item_id: Option<String>,
    /// 是否因为总字节预算而在条目上限之前截断。
    #[serde(default)]
    pub truncated: bool,
    /// 因超过单条预览预算而只以预览 + 引用返回的条目；身份与 ordinal 不变。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previews: Vec<TimelineItemPreview>,
    pub turns: Vec<TimelineTurn>,
}

/// 单条条目的可展示预览预算。
///
/// 页面同时限制条目数、总序列化字节与单条预览大小；超过本预算的正文不会整条进入页，
/// 而是压缩为同身份、同 kind 的预览，并在 [`TimelineItemPreview`] 中给出完整字节数。
/// 它必须小于页面的总字节预算，这样单条预览永远能落入一页。
pub const TIMELINE_ITEM_PREVIEW_BYTES: usize = 256 * 1024;

/// 一条超大条目在页面中只以预览呈现时的显式引用。
///
/// `item_id` / `ordinal` / `revision` 与被预览的条目完全一致，因此预览不改变身份、
/// 不改变顺序，也不与实时尾部或其它页的同一身份条目混淆。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineItemPreview {
    pub item_id: String,
    pub ordinal: u64,
    pub revision: u64,
    /// 完整条目的序列化字节数。
    pub total_bytes: u64,
    /// 预览条目的序列化字节数。
    pub preview_bytes: u64,
    /// 预览省略的序列化字节数。
    pub omitted_bytes: u64,
}

/// 直接读取单条完整条目的请求。
///
/// 普通分页仍走纯 SQL 的 [`TimelinePage`]，超出单条预览预算的条目只以
/// [`TimelineItemPreview`] 返回；此请求用 item identity 直接读数据库，返回未经预览压缩的
/// 完整 payload，且不改变身份、ordinal、revision 与读取水位语义。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimelineItemQuery {
    /// canonical item identity，与分页返回的 item id 完全一致。
    pub item_id: String,
}

/// 单条完整条目的直接读取结果。
///
/// 与 [`TimelinePage`] 使用同一数据库身份与水位字段，因此消费者可以把完整条目按 item
/// identity 合并进既有窗口，而无需重新解释 ordinal 或 revision。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineItemRead {
    pub thread_id: String,
    /// 该条目来自哪个 history 数据库实体；数据库重建后此值变化。
    pub database_id: String,
    /// 读取水位：构造本结果时的 applied write sequence。
    pub watermark: u64,
    pub ordinal: u64,
    pub item: ThreadItem,
}

/// 预览标记：追加到被截断的字符串正文末尾，说明完整内容已从该页省略。
const TIMELINE_PREVIEW_MARKER: &str = "…[truncated]";

/// 身份/判别字段名：预览只缩短正文，绝不改写这些键。
const TIMELINE_IDENTITY_KEYS: [&str; 8] = [
    "id", "itemId", "threadId", "turnId", "ordinal", "revision", "kind", "type",
];

/// 把单条条目压缩到 `max_bytes` 以内；未超过预算时原样返回且不产生引用。
///
/// 只有字符串正文会被缩短，条目身份字段、状态判别标签与 ordinal 保持不变，因此预览
/// 不破坏身份和顺序。返回的 `Some(reference)` 给出完整载荷的字节数与省略量。
pub fn preview_timeline_item(
    item: &ThreadItem,
    max_bytes: usize,
) -> (ThreadItem, Option<TimelineItemPreview>) {
    let Ok(total) = serde_json::to_string(item) else {
        return (item.clone(), None);
    };
    if total.len() <= max_bytes {
        return (item.clone(), None);
    }
    let Ok(mut value) = serde_json::to_value(item) else {
        return (item.clone(), None);
    };
    // 逐级收紧每个字符串正文的上限，直到整条序列化大小落入预算，或正文已无法再短。
    for step in [4 * 1024usize, 512, 64, 8] {
        truncate_preview_strings(&mut value, step);
        if serialized_bytes(&value).is_some_and(|bytes| bytes <= max_bytes) {
            break;
        }
    }
    let Ok(preview) = serde_json::from_value::<ThreadItem>(value) else {
        return (item.clone(), None);
    };
    let preview_bytes = serde_json::to_string(&preview).map_or(total.len(), |text| text.len());
    let reference = TimelineItemPreview {
        item_id: item.id.clone(),
        ordinal: item.ordinal,
        revision: item.revision,
        total_bytes: total.len() as u64,
        preview_bytes: preview_bytes as u64,
        omitted_bytes: total.len().saturating_sub(preview_bytes) as u64,
    };
    (preview, Some(reference))
}

fn serialized_bytes(value: &serde_json::Value) -> Option<usize> {
    serde_json::to_string(value).ok().map(|text| text.len())
}

fn truncate_preview_strings(value: &mut serde_json::Value, max_chars: usize) {
    match value {
        serde_json::Value::String(text) => {
            if text.chars().count() > max_chars {
                let kept = text.chars().take(max_chars).collect::<String>();
                *text = format!("{kept}{TIMELINE_PREVIEW_MARKER}");
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                truncate_preview_strings(item, max_chars);
            }
        }
        serde_json::Value::Object(entries) => {
            for (key, entry) in entries.iter_mut() {
                if TIMELINE_IDENTITY_KEYS.contains(&key.as_str()) {
                    continue;
                }
                truncate_preview_strings(entry, max_chars);
            }
        }
        _ => {}
    }
}

/// Turn metadata is independent of whether its admission item is in this page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineTurn {
    pub turn: Turn,
    pub last_item_id: String,
    pub context_disposition: ThreadContextDisposition,
}

crate::impl_labeled_enum!(
    ThreadStatus,
    "ThreadStatus",
    [
        ThreadStatus::Idle => "idle",
        ThreadStatus::Queued => "queued",
        ThreadStatus::Running => "running",
        ThreadStatus::WaitingTool => "waitingTool",
        ThreadStatus::WaitingInteraction => "waitingInteraction",
        ThreadStatus::Cancelling => "cancelling",
        ThreadStatus::Closing => "closing",
        ThreadStatus::Closed => "closed",
        ThreadStatus::Faulted => "faulted",
    ]
);
