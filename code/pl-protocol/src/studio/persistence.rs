//! Thread/process persistence watermarks and queue pressure.
//!
//! These are observation facts for diagnostics and backpressure: they never carry authoritative
//! Thread state, and a missing value is unknown rather than zero.
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum HistoryFault {
    QueueFull,
    WriteFailed,
    WriterUnavailable,
    NoProgress,
    CheckpointFailed,
    BlobFailed,
}

/// 存储安全间隙下 Thread 的执行阶段。
///
/// 它是 core 存储执行阶段的 typed 投影：暂停不是失败，也不是 Turn 终态，只说明新的推理准入在
/// 安全间隙上停住，直到存储重新可用。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ThreadStorageExecution {
    #[default]
    Running,
    Pausing,
    Paused,
}

/// Thread 的存储状态：typed 故障、水位与恢复阶段。
///
/// 每个字段都必须来自 typed 事实（runtime 持久化协调器的 typed fault/水位，或 core 的存储执行
/// 阶段）；**不得**从错误文本推断故障类别。`None` 一律表示未知，不表示零，也不表示健康。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStorageState {
    /// 当前已记录的存储故障类别；缺失表示没有已记录的故障事实。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault: Option<HistoryFault>,
    /// 手动恢复命令需要匹配的故障代数。
    ///
    /// 保存重试（`retry_thread_history`）与显式继续（`resume_thread_history`）都按它命名自己要
    /// 恢复的那次故障；代数不匹配即拒绝，旧决定不能解除新故障。
    #[serde(default)]
    pub fault_generation: u64,
    /// 已接纳的提交水位；缺失表示未知。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_sequence: Option<u64>,
    /// 已落盘的提交水位；缺失表示未知。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durable_sequence: Option<u64>,
    /// 存储安全间隙下的执行阶段。
    #[serde(default)]
    pub execution: ThreadStorageExecution,
    /// 是否因存储压力暂停了新推理准入。
    #[serde(default)]
    pub pressure_paused: bool,
    /// 是否必须由用户显式继续（`resume_thread_history`）才恢复新模型/工具准入。
    ///
    /// `pressure_paused` 是自动恢复的短暂背压；本字段是**硬故障**留下的显式闸门：即使重试保存后
    /// 故障类别已清空、也不再有压力，`true` 仍表示准入被闩住，只有一次核验过的显式继续才解除。
    /// `execution == Paused` 且 `resume_required == false` 且 `pressure_paused == false` 表示已恢复正常。
    #[serde(default)]
    pub resume_required: bool,
    /// 现在显式继续是否真的会被后端接受。
    ///
    /// 与 `resume_required` 的区别是**就绪性**：`resume_required` 只说明硬故障闩住了准入；本字段还要
    /// 求恢复已被核验（保存重试的目标水位已 durable、没有仍在报告的硬故障、没有未上交的批次、也没有
    /// 字节阈值挡住）。只有一个计算点：core 安全点从自己的 typed 事实算出这份就绪性，后端命名代数时
    /// 还要求该后端自己核验过同一代次（该核验经 `StoragePressure::recovered_generation` 进入 owner，
    /// 不是并列的第二份状态）；投影直接采用它，writer 无从提供可被 OR 进来的第二份就绪位，否则旧代次
    /// writer 的成功会在 core 仍有新故障时代替 owner 点亮继续入口。它不是错误文本推断，也不等同于
    /// “没有故障”。UI 只能用本字段启用继续按钮；后端仍会重查以防竞态。
    #[serde(default)]
    pub can_resume: bool,
    /// core 提供的原始错误文本（若存在）。它**不是**故障类别的来源，仅用于诊断显示。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// One Thread's persistence watermarks and queue pressure.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThreadPersistenceSnapshot {
    pub thread_id: String,
    /// Generation required by the per-Thread manual recovery command.
    #[serde(default)]
    pub fault_generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault: Option<HistoryFault>,
    /// Newest checkpoint revision admitted for publication; absent when no writer ever reported.
    ///
    /// A `None` is *unknown*, not measured zero: a Thread that only appears through a recovery
    /// diagnostic has no reporting writer, so its watermark is not a fact about it. Consumers must
    /// render unknown distinctly instead of substituting `0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_dirty_revision: Option<u64>,
    /// Checkpoint revision being serialized or synced; absent when nothing is in flight or unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_saving_revision: Option<u64>,
    /// Newest checkpoint revision already published as `state.toml`; absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_durable_revision: Option<u64>,
    /// Newest effect sequence admitted for the history write; absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_admitted_sequence: Option<u64>,
    /// Newest effect sequence whose history write completed; absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_durable_sequence: Option<u64>,
    /// Newest effect sequence admitted for the call write; absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calls_admitted_sequence: Option<u64>,
    /// Newest effect sequence whose call write completed; absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calls_durable_sequence: Option<u64>,
    /// Queued effects plus one for a retained checkpoint publication.
    pub pending_operations: u64,
    /// Encoded bytes of the queued effects.
    pub pending_bytes: u64,
    /// Age of the oldest queued operation; absent when nothing is queued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_pending_age_millis: Option<u64>,
    /// Encoded bytes of the operation currently being written.
    pub in_flight_bytes: u64,
    /// Last typed persistence error, kept until a later write succeeds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Whether this Thread paused new inference admission under storage pressure.
    pub pressure_paused: bool,
}

/// Process-wide persistence pressure across every loaded Thread.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PersistenceQueueSnapshot {
    /// Call statistics were lost or the writer failed; pending writes are not a gap.
    #[serde(default)]
    pub statistics_gap: bool,
    /// Total queued operations across all Threads.
    pub pending_operations: u64,
    /// Total queued bytes across all Threads.
    pub pending_bytes: u64,
    /// Total in-flight bytes across all Threads.
    pub in_flight_bytes: u64,
    /// Age of the oldest queued operation anywhere; absent when nothing is queued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_pending_age_millis: Option<u64>,
    /// Last typed error across Thread or call persistence, kept until a durable write succeeds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Whether any Thread paused admission under storage pressure.
    pub pressure_paused: bool,
    /// Per-Thread watermarks, ordered by Thread identity.
    #[serde(default)]
    pub threads: Vec<ThreadPersistenceSnapshot>,
}
