//! Studio 关闭结果的 typed 报告。
//!
//! 这是 desktop 应用退出与进程内严格关闭共用的最小报告形状：只表达退出/诊断结果，
//! 不伪造 `Stopped` 生命周期终态，也不宣称持久化成功。wire 为 camelCase，内部为
//! snake_case；`code` 是稳定诊断码（字符串），不是 typed 错误 enum。

use serde::{Deserialize, Serialize};

/// 关闭结果分类。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioShutdownOutcome {
    /// runtime 从未安装；不触发初始化。
    NotStarted,
    /// 所有必要阶段成功、待保存排空且 store 已实际关闭。
    Clean,
    /// 仍有阶段失败、超时或持久化未收束。
    Degraded,
}

/// 关闭观察到的待保存持久化水位。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioPendingPersistence {
    /// 无法确定待保存数量。
    Unknown,
    /// 仍有 `count` 条事实没有 durable 确认。
    Pending { count: u64 },
    /// 已确认排空。
    Drained,
}

/// 单个关闭阶段的聚合诊断。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioShutdownIssue {
    /// 已知阶段的稳定诊断标签；只用于诊断边界，不替代既有 typed progress。
    pub stage: String,
    /// 稳定诊断码（字符串）。
    pub code: String,
    /// 脱敏后的可读说明；不包含凭据或正文。
    pub message: String,
    pub retryable: bool,
    /// 关联同步日志与临时应急诊断。
    pub correlation_id: String,
}

/// 应用退出的最小关闭报告。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioShutdownReport {
    pub outcome: StudioShutdownOutcome,
    pub issues: Vec<StudioShutdownIssue>,
    pub persistence: StudioPendingPersistence,
}

impl StudioShutdownReport {
    /// 未安装 runtime 的退出报告：`NotStarted` 且无待保存事实。
    pub fn not_started() -> Self {
        Self {
            outcome: StudioShutdownOutcome::NotStarted,
            issues: Vec::new(),
            persistence: StudioPendingPersistence::Drained,
        }
    }

    /// 预算耗尽兜底报告：`Degraded`，待保存状态未知。
    pub fn deadline_exceeded(stage: &str, correlation_id: String) -> Self {
        Self {
            outcome: StudioShutdownOutcome::Degraded,
            issues: vec![StudioShutdownIssue {
                stage: stage.to_string(),
                code: "deadlineExceeded".to_string(),
                message: "shutdown deadline elapsed before the runtime finished cleanup"
                    .to_string(),
                retryable: true,
                correlation_id,
            }],
            persistence: StudioPendingPersistence::Unknown,
        }
    }

    /// 关闭编排任务自身 panic 的兜底报告：`Degraded`，待保存状态未知。
    ///
    /// 关闭阶段在独立 owned task 中运行；即使阶段 panic，也必须给出可诊断的报告而不是
    /// 让等待方永久挂起，也绝不伪报 `Clean`。
    pub fn panicked() -> Self {
        let correlation_id = pl_protocol::studio::StudioError::internal().correlation_id;
        Self {
            outcome: StudioShutdownOutcome::Degraded,
            issues: vec![StudioShutdownIssue {
                stage: "shutdown".to_string(),
                code: "internal".to_string(),
                message: "shutdown orchestration task panicked".to_string(),
                retryable: false,
                correlation_id,
            }],
            persistence: StudioPendingPersistence::Unknown,
        }
    }

    pub fn is_clean(&self) -> bool {
        matches!(self.outcome, StudioShutdownOutcome::Clean)
    }
}
