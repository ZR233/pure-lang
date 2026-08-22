mod resource;

use serde::{Deserialize, Serialize};

pub use resource::{
    DegradedResource, FailedResource, LoadingResource, ObservedResource, ObservedResourceCommand,
    ObservedResourceKind, ObservedResourceTransitionDecision, ObservedResourceTransitionError,
    ReadyResource, RefreshingResource, StaleResource, StoppedResource, UninitializedResource,
};

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
