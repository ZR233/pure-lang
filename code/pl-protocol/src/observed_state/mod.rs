use serde::{Deserialize, Serialize};

/// 外部可观察领域状态的公共元数据。
///
/// payload 由领域 snapshot 持有；失败时 owner 保留最后一次成功 payload，并通过
/// [`Self::stale`] 表达该 payload 不再与 desired state 一致。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ObservedStateMeta {
    pub revision: u64,
    pub phase: ObservedStatePhase,
    pub updated_at: i64,
    pub last_checked_at: Option<i64>,
    pub stale: bool,
}

impl ObservedStateMeta {
    /// 创建尚未执行领域初始化的 authoritative empty 元数据。
    pub fn uninitialized(updated_at: i64) -> Self {
        Self {
            revision: 0,
            phase: ObservedStatePhase::Uninitialized,
            updated_at,
            last_checked_at: None,
            stale: false,
        }
    }

    /// 创建已经就绪的领域元数据。
    pub fn ready(revision: u64, updated_at: i64) -> Self {
        Self {
            revision,
            phase: ObservedStatePhase::Ready,
            updated_at,
            last_checked_at: None,
            stale: false,
        }
    }
}

/// 领域 owner 的公开生命周期阶段。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ObservedStatePhase {
    Uninitialized,
    Ready,
    Running {
        operation: StateOperation,
        operation_id: String,
    },
    Failed {
        operation: StateOperation,
        error: StateError,
    },
    Stopped,
}

/// 可以改变或重新观察领域状态的明确命令。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StateOperation {
    Initialize,
    Activate,
    Reload,
    Reconcile,
    Discover,
    Check,
    Probe,
    Repair,
    Reset,
    Shutdown,
}

/// 可跨 Bridge 展示和判断是否允许重试的结构化领域错误。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StateError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_state_wire_format_is_camel_case_and_typed() {
        let value = serde_json::to_value(ObservedStateMeta {
            revision: 2,
            phase: ObservedStatePhase::Running {
                operation: StateOperation::Probe,
                operation_id: "probe-2".to_string(),
            },
            updated_at: 3,
            last_checked_at: Some(1),
            stale: true,
        })
        .unwrap();

        assert_eq!(value["lastCheckedAt"], 1);
        assert_eq!(value["phase"]["running"]["operation"], "probe");
        assert_eq!(value["phase"]["running"]["operationId"], "probe-2");
    }
}
