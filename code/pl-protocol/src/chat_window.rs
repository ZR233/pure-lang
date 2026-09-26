//! ChatView 内容窗口的 wire 契约。
//!
//! 内容窗口是**内存优先**的窗口：它由共享 `ChatSession` 的实时投影发布者写入，窗口里既有已落盘
//! 条目，也有尚未落盘的流式条目。因此它与 `/timeline`（纯 SQL 的 durable history 分页）职责不同：
//! 窗口回答“现在展示哪些条目、它们现在的版本是多少”，历史分页回答“数据库里已落盘的长历史”。
//!
//! 窗口携带 canonical [`crate::ThreadItem`]（身份、ordinal、结构化字段与已落盘条目完全一致），不在
//! wire 上做 JSON 字符串拼接：同一身份的内容变化统一用 [`ChatWindowChange::UpdateItem`] 表达——一个
//! `expectedRevision`/`revision` 提交点，加上按字段身份的 [`ThreadFieldChange`]。翻页统一用
//! **锚点 + 方向**表达（见 [`ChatWindowQuery`]），由 core 的窗口分页推进，因此窗口始终有界。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::ThreadItem;

/// 窗口锚点：`latest` 是最新窗口；给 item identity 则以该条目为中心。
///
/// 只有“跳到某条已知历史”才用 `around`；上翻/下翻历史用 [`ChatWindowQuery::direction`]，不去
/// 假装某个条目是窗口中心。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ChatWindowFocus {
    Latest,
    Around { item_id: String },
}

/// 窗口相对锚点移动的方向。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ChatWindowDirection {
    /// 更旧的一页。
    Older,
    /// 更新的一页。
    Newer,
}

/// 建立（或订阅）一个内容窗口的请求：**锚点 + 方向**。
///
/// * `anchor` 缺省或 `latest`、`direction` 缺省：最新窗口。
/// * `anchor` = canonical item identity、`direction` 缺省：**跳转**到该条目，得到以它为中心的有界
///   窗口；响应里的 `focus` 如实给出 `around{...}`，不把跳转说成分页。
/// * `anchor` = 当前窗口**首条** identity、`direction=older`：向更旧移动一页；`anchor` = 当前窗口
///   **末条** identity、`direction=newer`：向更新移动一页。两端都由 core 的窗口分页推进（每次移动
///   都按方向换入一页、换出相反方向的一页），窗口条目数恒有界，因此反复翻页一定前进而不会停在原地。
/// * `anchor` 缺省、`direction=older`：从最新窗口向更旧移动一页（首屏之后直接上翻）。
///
/// 换 anchor 或方向就是换窗口：订阅方重新建立窗口并收到一次权威 `Reset`，不存在跨窗口的增量拼接。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatWindowQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<ChatWindowDirection>,
}

/// 条目的执行终态事实，与保存水位 `saved` 严格独立。
///
/// 生产者不再推进该身份的内容时为 `Terminal`；它与 `saved`（writer 已确认该 revision 落盘）互不推导，
/// 因此“已终态但未落盘”和“已落盘但仍在流式”都能如实表达。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ChatWindowLifecycle {
    Streaming,
    Terminal,
}

/// 窗口里的一条 canonical 条目。
///
/// `item` 就是 canonical 条目本身；`saved` / `lifecycle` / `omitted_bytes` 是共享会话对该条目的原始
/// 事实（是否已落盘、是否已终态、以及有界窗口为控制内存而省略的字节数），不是窗口自造的标记。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatWindowItem {
    pub item: ThreadItem,
    pub saved: bool,
    pub lifecycle: ChatWindowLifecycle,
    #[serde(default)]
    pub omitted_bytes: u64,
}

/// 一个一致的内容窗口：同一版本下的一组 canonical 条目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatWindowSnapshot {
    pub thread_id: String,
    pub focus: ChatWindowFocus,
    /// 窗口自己的版本。
    ///
    /// 它与状态流水位互相独立：窗口版本只在窗口事实（条目集合或某条目内容）变化时前进。客户端比较
    /// 窗口版本判断新旧，不能拿它当状态流的 envelope `revision`。
    pub version: u64,
    pub items: Vec<ChatWindowItem>,
    pub has_older: bool,
    pub has_newer: bool,
}

/// 一帧是否需要立即交付，还是普通文本可以合帧。
///
/// 由 core 依据权威窗口事实给出（新身份首字、执行终态准入、保存确认、字段删除、权威替换、结构变化
/// 都是 `Immediate`），宿主不得自行分类或 `sleep`/throttle 生产者。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ChatWindowPriority {
    Immediate,
    Coalesced,
}

/// 内容窗口的一次更新。
///
/// `Reset` 是权威整窗：换锚点/方向、重连、一次无法用 patch 词汇表达的变化都用它收束，客户端直接替换
/// 本地窗口。`Patch` 相对**客户端自己的基线**连续：`from` 必须等于本地窗口版本，否则客户端应重新建立
/// 窗口而不是拼接。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChatWindowUpdate {
    Reset {
        window: ChatWindowSnapshot,
    },
    Patch {
        from: u64,
        to: u64,
        changes: Vec<ChatWindowChange>,
        has_newer: bool,
        priority: ChatWindowPriority,
    },
}

/// 内容增量落在哪个字段上。
///
/// 变体身份与宿主字段域一一对应：字段类别决定 Dart 侧把 `text` 应用到条目结构的哪里，而不是让消费者
/// 重新解析整条 JSON。映射只由字段身份决定，不从内容字符串推断。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(
    rename_all = "camelCase",
    tag = "kind",
    rename_all_fields = "camelCase"
)]
pub enum ThreadContentField {
    Text,
    ThinkingSummary { chunk_index: u32 },
    ThinkingContent { chunk_index: u32 },
    ToolArguments,
    ToolResult,
}

/// 一个内容字段的变化。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ThreadFieldChange {
    /// 正文不变，但所属 item 的 revision/omitted 仍要提交（版本帧不是空 Patch）。
    Unchanged,
    /// 该字段仍以前缀方式扩展本地已交付末尾；`text` 是新增字节。
    Append { text: String },
    /// 本地基线已失效（权威替换、预览升级）；`text` 是整段权威正文。
    Replace { text: String },
    /// 该字段已从条目移除，消费者丢弃本地副本。
    Remove,
}

/// 同一 item 的一个字段变化。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThreadFieldUpdate {
    pub field: ThreadContentField,
    pub change: ThreadFieldChange,
}

/// 窗口内的一次变化。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChatWindowChange {
    /// 窗口某个位置的条目替换：`index` 是窗口下标，`remove` 是要移除的条目数。
    Splice {
        index: u64,
        remove: u64,
        items: Vec<ChatWindowItem>,
    },
    /// 同一身份的全部内容字段，在**一次** `expectedRevision`/`revision` 提交里原子应用。
    ///
    /// 应用条件：本地该 `item_id` 的 content revision 等于 `expected_revision`；否则基线不连续，按宿主
    /// 策略重同步（**不是**逐字段升 revision）。全部字段应用完成后**一次**提交 `revision`、
    /// `omitted_bytes` 与保存水位 `saved`——即使字段全为 `Unchanged`（仅版本推进、仅保存确认）也必须
    /// 提交，不能当成空 Patch 丢弃。`saved` 与内容 revision、执行终态都相互独立。
    UpdateItem {
        item_id: String,
        expected_revision: u64,
        revision: u64,
        omitted_bytes: u64,
        saved: bool,
        fields: Vec<ThreadFieldUpdate>,
    },
}
