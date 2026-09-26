//! Typed FRB mirror of the canonical current-activity projection.
//!
//! 活动只携带小型 typed 摘要与身份；完整 reasoning/正文/工具参数由
//! `read_thread_activity_detail` 按身份按需读取。

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeThreadActivity {
    pub thread_id: String,
    pub identity: String,
    pub revision: u64,
    pub turn_id: String,
    pub input_id: Option<String>,
    pub attempt_id: Option<String>,
    pub kind: BridgeThreadActivityKind,
    pub summary: String,
    pub summary_truncated: bool,
    pub tools: BridgeThreadActivityTools,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeThreadActivityKind {
    Preparing,
    WaitingApi,
    Thinking,
    Responding,
    Planning,
    RunningTool,
    AwaitingApproval,
    AwaitingInput,
    Stopping,
}

/// 活跃工具参数事实的可用程度：命令行摘要在参数不完整或无法提取时回落工具名称。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeThreadActivityArguments {
    CommandLine,
    Opaque,
    Streaming,
    Unavailable,
}

/// 一条工具调用的执行状态；活动条目只会取到前三个变体，终态调用是 `finished`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeThreadActivityToolState {
    Running,
    AwaitingApproval,
    Cancelling,
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeThreadActivityToolEntry {
    pub call_id: String,
    pub task_id: Option<String>,
    pub name: String,
    pub summary: String,
    pub arguments: BridgeThreadActivityArguments,
    pub state: BridgeThreadActivityToolState,
    pub ordinal: Option<u64>,
    pub started_at: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BridgeThreadActivityTools {
    pub count: u32,
    /// 其中处于后台的活跃工具数量；前台就是工具本身时为 0。
    pub background: u32,
    pub active: Vec<BridgeThreadActivityToolEntry>,
    pub latest_started: Option<BridgeThreadActivityToolEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeThreadActivityContentPart {
    pub item_id: String,
    /// canonical item revision；`None` 表示这段内容来自纯内存推导，没有可证明的 revision。
    pub revision: Option<u64>,
    /// 该内容段是否已终态；`false` 表示仍在流式追加。
    pub complete: bool,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeThreadActivityToolDetail {
    pub call_id: String,
    pub task_id: Option<String>,
    pub name: String,
    pub state: BridgeThreadActivityToolState,
    /// canonical item 的原始调用参数；不可读时为 `None`。
    pub arguments: Option<String>,
    /// 目前观测到的流式或终态输出；暂无输出为 `None`。
    pub output: Option<String>,
    pub ordinal: Option<u64>,
    pub started_at: Option<i64>,
}

/// 活动详情的生命周期结果：只有 `Current` 携带正文。
#[derive(Debug, Clone, PartialEq)]
pub enum BridgeThreadActivityDetail {
    Current {
        activity: BridgeThreadActivity,
        reasoning: Vec<BridgeThreadActivityContentPart>,
        response: Vec<BridgeThreadActivityContentPart>,
        tools: Vec<BridgeThreadActivityToolDetail>,
    },
    Superseded {
        activity: BridgeThreadActivity,
        requested_activity_id: String,
    },
    Ended {
        thread_id: String,
        activity_id: String,
    },
}

/// Thread 存储状态；所有字段都是 typed 事实，缺失表示未知而不是零。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeThreadStorageState {
    pub fault: Option<BridgeHistoryFault>,
    pub fault_generation: u64,
    pub accepted_sequence: Option<u64>,
    pub durable_sequence: Option<u64>,
    pub execution: BridgeThreadStorageExecution,
    pub pressure_paused: bool,
    /// 硬故障是否仍闩住新准入，必须由用户显式 `resume_thread_history` 继续。
    ///
    /// `pressure_paused` 是自动恢复的背压；本字段为 `true` 时重试保存成功也不会自动继续。
    pub resume_required: bool,
    /// 现在显式继续是否真的会被后端接受。
    ///
    /// `resume_required` 只说明故障闩住了准入；本字段还要求恢复已被核验（保存重试的目标水位已
    /// durable、没有仍在报告的硬故障、没有未上交批次、也没有字节阈值挡住）。UI 只能用本字段启用
    /// “继续”；后端 resume 仍会重查，所以它不是前端推断。
    pub can_resume: bool,
    /// core 提供的原始错误文本；它不是故障类别的来源。
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeHistoryFault {
    QueueFull,
    WriteFailed,
    WriterUnavailable,
    NoProgress,
    CheckpointFailed,
    BlobFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeThreadStorageExecution {
    Running,
    Pausing,
    Paused,
}
