//! 当前执行活动的 typed 实时投影契约。
//!
//! 活动与 ChatView 历史窗口是两条正交的事实：窗口决定展示哪些条目，活动只描述“当前在做什么”。
//! 活动快照与活动帧只携带小型 typed 摘要与身份，**不重复传正文**；完整 reasoning、输出正文、
//! 工具参数与流式输出按活动身份通过 [`ThreadActivityDetailQuery`] 按需读取 canonical 事实。

use serde::{Deserialize, Serialize};

/// 活动摘要保留的最大字符数（按字符边界截断，保留行首）。
///
/// 摘要只描述当前执行行，超出本上限时置 [`ThreadActivity::summary_truncated`]，不伪造省略号。
pub const ACTIVITY_SUMMARY_LIMIT: usize = 512;

/// 当前活动的实际事实判别。
///
/// 它是 canonical 事实的投影，不是 Turn 生命周期标签：同一 Turn 内 kind 会随 attempt、工具与
/// 交互事实变化。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThreadActivityKind {
    /// Turn 运行中，但还没有可观察到的前台 **provider** 阶段。
    ///
    /// 有两种来源：
    ///
    /// 1. core 的**显式**执行阶段：准备上下文（可能替换或压缩上下文）、构造请求、模型实现自己的请求
    ///    准备、容量/存储准入。它们都是本地准备事实；
    /// 2. **中性**兜底：此刻没有更精确的阶段事实（没有运行中的 attempt、没有 core 阶段、没有运行中的
    ///    工具、待批准或待回答）。
    ///
    /// 两种情况都**不代表**正在压缩上下文，也不代表已经向供应商派出了请求：只有 provider 调用真正开始
    /// 执行之后的等待才归 [`Self::WaitingApi`]。绝不从缺失字段推断具体上下文操作。
    Preparing,
    /// provider 调用已经真正开始执行，但还没有任何已观测的流式事实：attempt 运行中，或 core 报告
    /// provider 调用已经在执行（`Running`）。
    WaitingApi,
    /// attempt 运行中且只观测到 reasoning。
    Thinking,
    /// attempt 运行中且已观测到输出正文，或最新 attempt 已提交且无 tool call。
    Responding,
    /// 最新 attempt 已提交且携带 tool call，但任务尚未开始。
    Planning,
    /// 存在运行中的工具任务。
    RunningTool,
    /// 运行中的工具任务在等待显式权限决定。
    AwaitingApproval,
    /// 存在等待用户回答的交互请求。
    AwaitingInput,
    /// 当前 Turn 正在被打断，或运行中的任务已请求取消。
    Stopping,
}

impl ThreadActivityKind {
    /// Canonical 稳定标签；构成活动身份的一部分，因此不能随展示文案变化。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::WaitingApi => "waitingApi",
            Self::Thinking => "thinking",
            Self::Responding => "responding",
            Self::Planning => "planning",
            Self::RunningTool => "runningTool",
            Self::AwaitingApproval => "awaitingApproval",
            Self::AwaitingInput => "awaitingInput",
            Self::Stopping => "stopping",
        }
    }
}

/// 活跃工具参数事实的可用程度。
///
/// 命令行摘要只在参数完整且能按工具约定提取时给出；其余情况一律回落工具名称，不拼接不完整的
/// 参数，也不把缺失数据当成空参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThreadActivityArguments {
    /// 参数完整且提取出命令行：摘要即完整命令行。
    CommandLine,
    /// 参数完整但提取不出命令行：摘要是工具名称。
    Opaque,
    /// 参数仍在流式生成：摘要是工具名称。
    Streaming,
    /// 该调用的参数事实当前不可读：摘要是工具名称。
    Unavailable,
}

/// 一条工具调用的执行状态。
///
/// 活动条目（[`ThreadActivityToolEntry`]）只会取到前三个变体（它们描述**正在执行**的工具）；
/// 活动详情（[`ThreadActivityToolDetail`]）还会描述当前 Turn 里已经结束的调用，用
/// [`Self::Finished`] 表示终态，**不会**把终态谎报成仍在运行。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThreadActivityToolState {
    Running,
    AwaitingApproval,
    Cancelling,
    /// 该调用已经进入终态（成功、失败、被拒绝、取消或中断）；详情里的 `output` 才是结果事实。
    Finished,
}

/// 一条活跃工具的身份、摘要与执行状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadActivityToolEntry {
    pub call_id: String,
    /// 运行中任务的稳定身份；该调用尚未进入任务表时为 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// 工具 ID，例如 `exec`。
    pub name: String,
    /// 命令行摘要，或工具名称；取决于 [`Self::arguments`]。
    pub summary: String,
    pub arguments: ThreadActivityArguments,
    /// 该调用当前的执行状态；活跃条目只会是 `running` / `awaitingApproval` / `cancelling`。
    pub state: ThreadActivityToolState,
    /// 该调用的 canonical 接纳序号（durable 条目事实）；内存投影不可读时为 `None`（不是 0）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordinal: Option<u64>,
    /// 该调用 canonical 接纳时间；内存投影不可读时为 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
}

/// 当前并行执行的工具集合。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadActivityTools {
    /// 活跃工具数量，等于 [`Self::active`] 的长度。
    pub count: u32,
    /// 其中处于后台的活跃工具数量。
    ///
    /// 前台正在等待模型输出或仍在准备时，正在跑的工具都是后台工具；前台就是工具本身时该值为 0。
    /// 它是“前台阶段”的正交补充，因此模型继续回复时客户仍能看到后台还有多少工具在跑。
    #[serde(default, skip_serializing_if = "background_is_zero")]
    pub background: u32,
    /// 活跃工具，按接纳序号升序。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active: Vec<ThreadActivityToolEntry>,
    /// 最近启动的活跃工具。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_started: Option<ThreadActivityToolEntry>,
}

fn background_is_zero(value: &u32) -> bool {
    *value == 0
}

/// Thread 当前执行活动的权威小摘要。
///
/// `identity` 绑定**当前这一步活动**：Turn、当前 attempt（准备阶段为 `preparing`）与实际前台
/// `kind` 一起决定身份。于是同一活动内流式正文增长不会换身份，而 API/思考/工具/交互之间的真实
/// 前台切换会换身份——客户端因此可以按身份固定活动行并在真实切换时重置展开状态。活动结束以
/// `None` 表达（见 [`crate::ThreadNotification::ActivityChanged`]），迟到帧不会恢复旧活动。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadActivity {
    pub thread_id: String,
    /// 稳定活动身份：`activity:{turn_id}:{step}:{kind}`。
    pub identity: String,
    /// 该活动自其身份建立以来的单调递增版本。
    ///
    /// 身份不变时每次活动事实变化递增（含只有流式进展、尚未提交的更新）；身份变化即新活动，版本
    /// 重新从 0 开始。它**不是** canonical 提交水位，也不是状态流 envelope 的 `revision`。
    pub revision: u64,
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_id: Option<String>,
    /// 当前 attempt；`preparing` 阶段为 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    pub kind: ThreadActivityKind,
    /// 最新非空逻辑行；见 [`ACTIVITY_SUMMARY_LIMIT`]。
    pub summary: String,
    /// 摘要是否因长度上限被截断。
    #[serde(default)]
    pub summary_truncated: bool,
    #[serde(default)]
    pub tools: ThreadActivityTools,
}

impl ThreadActivity {
    /// 一步活动（Turn + 当前步骤 + 前台 kind）的稳定身份。
    ///
    /// `step` 是当前 attempt 的身份，准备阶段用 `preparing` 占位。同一活动内重复调用得到同一身份；
    /// 只有活动真实切换（attempt 推进或前台 kind 变化）才产生新身份。
    pub fn identity(turn_id: &str, step: &str, kind: ThreadActivityKind) -> String {
        format!("activity:{turn_id}:{step}:{}", kind.label())
    }
}

/// 按活动身份读取完整内容事实的请求。
///
/// 只读：不激活 owner、不 flush writer、不改变任何 revision。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadActivityDetailQuery {
    /// canonical 活动身份，与 [`ThreadActivity::identity`] 完全一致。
    pub activity_id: String,
}

/// 一段完整内容事实。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadActivityContentPart {
    /// canonical item identity。
    pub item_id: String,
    /// 该内容段当前的 canonical item revision。
    ///
    /// `None` 表示这段内容来自纯内存推导，没有可证明的 canonical item revision（观察者提交该活动的
    /// canonical 条目事实后才会给出）。客户端遇到 `None` 应整段替换，不按 revision 合并，也不要把
    /// `None` 当成 0。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    /// 该内容段是否已终态；`false` 表示仍在流式追加。
    pub complete: bool,
    /// 完整文本（未经预览压缩）。
    pub text: String,
}

/// 一条活跃工具的完整调用与输出事实。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadActivityToolDetail {
    pub call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub name: String,
    /// 该调用当前的执行状态；已经结束的调用为 [`ThreadActivityToolState::Finished`]。
    pub state: ThreadActivityToolState,
    /// canonical item 的原始调用参数；`None` 表示该调用的参数事实当前不可读。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
    /// 目前已观测的流式或终态输出；`None` 表示暂无输出事实。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordinal: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
}

/// 活动详情的生命周期结果。
///
/// 只有请求身份**当前仍然成立**时才返回正文；活动结束后不再提供旧活动的完整内容，因此迟到的
/// 展开请求不会把已结束的活动恢复到 UI。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    tag = "state",
    rename_all_fields = "camelCase"
)]
pub enum ThreadActivityDetail {
    /// 请求身份仍是当前活动。
    Current {
        activity: ThreadActivity,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        reasoning: Vec<ThreadActivityContentPart>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        response: Vec<ThreadActivityContentPart>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tools: Vec<ThreadActivityToolDetail>,
    },
    /// 请求身份已被同一 Thread 的更新活动取代。
    Superseded {
        activity: ThreadActivity,
        requested_activity_id: String,
    },
    /// 请求身份所在活动已结束（当前没有活动）。
    Ended {
        thread_id: String,
        activity_id: String,
    },
}
