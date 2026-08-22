//! 外部资源观察状态；每个 variant 只持有当前阶段合法的字段。

mod degraded;
mod failed;
mod loading;
mod ready;
mod refreshing;
mod stale;
mod stopped;
mod uninitialized;

pub use degraded::DegradedResource;
pub use failed::FailedResource;
pub use loading::LoadingResource;
pub use ready::ReadyResource;
pub use refreshing::RefreshingResource;
pub use stale::StaleResource;
pub use stopped::StoppedResource;
pub use uninitialized::UninitializedResource;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{StateError, StateOperation};

/// 一个可刷新外部资源的 canonical 生命周期。
///
/// 无可用值的阶段与保留 last-known value 的阶段在类型层分离。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum ObservedResource<T> {
    Uninitialized(UninitializedResource),
    Loading(LoadingResource),
    Ready(ReadyResource<T>),
    Refreshing(RefreshingResource<T>),
    Stale(StaleResource<T>),
    Degraded(DegradedResource<T>),
    Failed(FailedResource),
    Stopped(StoppedResource),
}

/// 外部资源的穷尽状态种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservedResourceKind {
    Uninitialized,
    Loading,
    Ready,
    Refreshing,
    Stale,
    Degraded,
    Failed,
    Stopped,
}

/// 可以改变外部资源生命周期的领域命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObservedResourceCommand<T> {
    Begin {
        expected_revision: u64,
        operation: StateOperation,
        operation_id: String,
        started_at: i64,
    },
    Succeed {
        expected_revision: u64,
        updated_at: i64,
        last_checked_at: Option<i64>,
        value: T,
    },
    Observe {
        expected_revision: u64,
        observed_at: i64,
        last_checked_at: Option<i64>,
        value: T,
    },
    MarkStale {
        expected_revision: u64,
        stale_at: i64,
        value: T,
    },
    Fail {
        expected_revision: u64,
        failed_at: i64,
        error: StateError,
    },
    Stop {
        expected_revision: u64,
        stopped_at: i64,
    },
}

impl<T> ObservedResourceCommand<T> {
    fn expected_revision(&self) -> u64 {
        match self {
            Self::Begin {
                expected_revision, ..
            }
            | Self::Succeed {
                expected_revision, ..
            }
            | Self::Observe {
                expected_revision, ..
            }
            | Self::MarkStale {
                expected_revision, ..
            }
            | Self::Fail {
                expected_revision, ..
            }
            | Self::Stop {
                expected_revision, ..
            } => *expected_revision,
        }
    }
}

/// 外部资源状态机的纯转换结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedResourceTransitionDecision<T> {
    pub next_state: ObservedResource<T>,
    pub changed: bool,
}

/// 非法外部资源状态转换。
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ObservedResourceTransitionError<T: std::fmt::Debug> {
    #[error("observed resource revision is stale: expected {expected}, actual {actual}")]
    StaleRevision {
        expected: u64,
        actual: u64,
        command: ObservedResourceCommand<T>,
    },
    #[error("observed resource in {current:?} rejects command {command:?}")]
    IllegalTransition {
        current: ObservedResourceKind,
        command: ObservedResourceCommand<T>,
    },
}

impl<T> ObservedResource<T> {
    /// 创建尚未加载且没有可用值的资源。
    pub fn uninitialized(updated_at: i64) -> Self {
        Self::Uninitialized(UninitializedResource::new(updated_at))
    }

    /// 创建已就绪的资源。
    pub fn ready(revision: u64, updated_at: i64, value: T) -> Self {
        Self::Ready(ReadyResource::new(revision, updated_at, value))
    }

    /// 从已持久化或 canonical 查询结果构造已就绪资源。
    pub fn ready_observed(
        revision: u64,
        updated_at: i64,
        last_checked_at: Option<i64>,
        value: T,
    ) -> Self {
        Self::Ready(ReadyResource::new_observed(
            revision,
            updated_at,
            last_checked_at,
            value,
        ))
    }

    /// 返回领域 revision。
    pub fn revision(&self) -> u64 {
        match self {
            Self::Uninitialized(state) => state.revision(),
            Self::Loading(state) => state.revision(),
            Self::Ready(state) => state.revision(),
            Self::Refreshing(state) => state.revision(),
            Self::Stale(state) => state.revision(),
            Self::Degraded(state) => state.revision(),
            Self::Failed(state) => state.revision(),
            Self::Stopped(state) => state.revision(),
        }
    }

    /// 返回当前穷尽状态种类。
    pub fn kind(&self) -> ObservedResourceKind {
        match self {
            Self::Uninitialized(_) => ObservedResourceKind::Uninitialized,
            Self::Loading(_) => ObservedResourceKind::Loading,
            Self::Ready(_) => ObservedResourceKind::Ready,
            Self::Refreshing(_) => ObservedResourceKind::Refreshing,
            Self::Stale(_) => ObservedResourceKind::Stale,
            Self::Degraded(_) => ObservedResourceKind::Degraded,
            Self::Failed(_) => ObservedResourceKind::Failed,
            Self::Stopped(_) => ObservedResourceKind::Stopped,
        }
    }

    /// 返回最近一次状态变化时间。
    pub fn updated_at(&self) -> i64 {
        match self {
            Self::Uninitialized(state) => state.updated_at(),
            Self::Loading(state) => state.started_at(),
            Self::Ready(state) => state.updated_at(),
            Self::Refreshing(state) => state.started_at(),
            Self::Stale(state) => state.stale_at(),
            Self::Degraded(state) => state.failed_at(),
            Self::Failed(state) => state.failed_at(),
            Self::Stopped(state) => state.stopped_at(),
        }
    }

    /// 返回适用状态中最近一次完成观察的时间。
    pub fn last_checked_at(&self) -> Option<i64> {
        match self {
            Self::Ready(state) => state.last_checked_at(),
            Self::Refreshing(state) => state.last_checked_at(),
            Self::Stale(state) => state.last_checked_at(),
            Self::Degraded(state) => state.last_checked_at(),
            Self::Uninitialized(_) | Self::Loading(_) | Self::Failed(_) | Self::Stopped(_) => None,
        }
    }

    /// 返回当前仍可展示的 canonical 或 last-known value。
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Ready(state) => Some(state.value()),
            Self::Refreshing(state) => Some(state.value()),
            Self::Stale(state) => Some(state.value()),
            Self::Degraded(state) => Some(state.value()),
            Self::Uninitialized(_) | Self::Loading(_) | Self::Failed(_) | Self::Stopped(_) => None,
        }
    }

    /// 消费资源并返回当前仍可用的值。
    pub fn into_value(self) -> Option<T> {
        match self {
            Self::Ready(state) => Some(state.into_value()),
            Self::Refreshing(state) => Some(state.into_value()),
            Self::Stale(state) => Some(state.into_value()),
            Self::Degraded(state) => Some(state.into_value()),
            Self::Uninitialized(_) | Self::Loading(_) | Self::Failed(_) | Self::Stopped(_) => None,
        }
    }

    /// 在不改变生命周期元数据的前提下转换资源值。
    pub fn map<U>(self, map_value: impl FnOnce(T) -> U) -> ObservedResource<U> {
        match self {
            Self::Uninitialized(state) => {
                ObservedResource::Uninitialized(UninitializedResource::new(state.updated_at()))
            }
            Self::Loading(state) => ObservedResource::Loading(LoadingResource::new(
                state.revision(),
                state.operation(),
                state.operation_id().to_string(),
                state.started_at(),
            )),
            Self::Ready(state) => {
                let revision = state.revision();
                let updated_at = state.updated_at();
                let last_checked_at = state.last_checked_at();
                ObservedResource::Ready(ReadyResource::new_observed(
                    revision,
                    updated_at,
                    last_checked_at,
                    map_value(state.into_value()),
                ))
            }
            Self::Refreshing(state) => {
                let revision = state.revision();
                let operation = state.operation();
                let operation_id = state.operation_id().to_string();
                let started_at = state.started_at();
                let last_checked_at = state.last_checked_at();
                ObservedResource::Refreshing(RefreshingResource::new(
                    revision,
                    operation,
                    operation_id,
                    started_at,
                    last_checked_at,
                    map_value(state.into_value()),
                ))
            }
            Self::Stale(state) => {
                let revision = state.revision();
                let stale_at = state.stale_at();
                let last_checked_at = state.last_checked_at();
                ObservedResource::Stale(StaleResource::new(
                    revision,
                    stale_at,
                    last_checked_at,
                    map_value(state.into_value()),
                ))
            }
            Self::Degraded(state) => {
                let revision = state.revision();
                let failed_at = state.failed_at();
                let last_checked_at = state.last_checked_at();
                let operation = state.operation();
                let error = state.error().clone();
                ObservedResource::Degraded(DegradedResource::new(
                    revision,
                    failed_at,
                    last_checked_at,
                    operation,
                    error,
                    map_value(state.into_value()),
                ))
            }
            Self::Failed(state) => ObservedResource::Failed(FailedResource::new(
                state.revision(),
                state.failed_at(),
                state.operation(),
                state.error().clone(),
            )),
            Self::Stopped(state) => ObservedResource::Stopped(StoppedResource::new(
                state.revision(),
                state.stopped_at(),
            )),
        }
    }
}

impl<T> ObservedResource<T>
where
    T: Clone + PartialEq + std::fmt::Debug,
{
    /// 纯计算一个命令的下一状态，不执行 IO。
    ///
    /// # Errors
    ///
    /// revision 过期或当前状态不接受命令时返回具体转换错误。
    pub fn decide(
        &self,
        command: ObservedResourceCommand<T>,
    ) -> Result<ObservedResourceTransitionDecision<T>, ObservedResourceTransitionError<T>> {
        if command.expected_revision() != self.revision() {
            return Err(ObservedResourceTransitionError::StaleRevision {
                expected: command.expected_revision(),
                actual: self.revision(),
                command,
            });
        }
        let revision = self.revision().saturating_add(1);
        let next_state = match (self, &command) {
            (
                Self::Uninitialized(_) | Self::Failed(_),
                ObservedResourceCommand::Begin {
                    operation,
                    operation_id,
                    started_at,
                    ..
                },
            ) => Self::Loading(LoadingResource::new(
                revision,
                *operation,
                operation_id.clone(),
                *started_at,
            )),
            (
                Self::Ready(current),
                ObservedResourceCommand::Begin {
                    operation,
                    operation_id,
                    started_at,
                    ..
                },
            ) => Self::Refreshing(RefreshingResource::new(
                revision,
                *operation,
                operation_id.clone(),
                *started_at,
                current.last_checked_at(),
                current.value().clone(),
            )),
            (
                Self::Stale(current),
                ObservedResourceCommand::Begin {
                    operation,
                    operation_id,
                    started_at,
                    ..
                },
            ) => Self::Refreshing(RefreshingResource::new(
                revision,
                *operation,
                operation_id.clone(),
                *started_at,
                current.last_checked_at(),
                current.value().clone(),
            )),
            (
                Self::Degraded(current),
                ObservedResourceCommand::Begin {
                    operation,
                    operation_id,
                    started_at,
                    ..
                },
            ) => Self::Refreshing(RefreshingResource::new(
                revision,
                *operation,
                operation_id.clone(),
                *started_at,
                current.last_checked_at(),
                current.value().clone(),
            )),
            (
                Self::Loading(_) | Self::Refreshing(_),
                ObservedResourceCommand::Succeed {
                    updated_at,
                    last_checked_at,
                    value,
                    ..
                },
            ) => Self::Ready(ReadyResource::new_observed(
                revision,
                *updated_at,
                *last_checked_at,
                value.clone(),
            )),
            (
                Self::Ready(_) | Self::Stale(_) | Self::Degraded(_),
                ObservedResourceCommand::Observe {
                    observed_at,
                    last_checked_at,
                    value,
                    ..
                },
            ) => Self::Ready(ReadyResource::new_observed(
                revision,
                *observed_at,
                *last_checked_at,
                value.clone(),
            )),
            (
                Self::Ready(_) | Self::Stale(_) | Self::Degraded(_),
                ObservedResourceCommand::MarkStale {
                    stale_at, value, ..
                },
            ) => Self::Stale(StaleResource::new(
                revision,
                *stale_at,
                self.last_checked_at(),
                value.clone(),
            )),
            (
                Self::Loading(current),
                ObservedResourceCommand::Fail {
                    failed_at, error, ..
                },
            ) => Self::Failed(FailedResource::new(
                revision,
                *failed_at,
                current.operation(),
                error.clone(),
            )),
            (
                Self::Refreshing(current),
                ObservedResourceCommand::Fail {
                    failed_at, error, ..
                },
            ) => Self::Degraded(DegradedResource::new(
                revision,
                *failed_at,
                current.last_checked_at(),
                current.operation(),
                error.clone(),
                current.value().clone(),
            )),
            (
                Self::Uninitialized(_)
                | Self::Loading(_)
                | Self::Ready(_)
                | Self::Refreshing(_)
                | Self::Stale(_)
                | Self::Degraded(_)
                | Self::Failed(_),
                ObservedResourceCommand::Stop { stopped_at, .. },
            ) => Self::Stopped(StoppedResource::new(revision, *stopped_at)),
            _ => {
                return Err(ObservedResourceTransitionError::IllegalTransition {
                    current: self.kind(),
                    command,
                });
            }
        };
        Ok(ObservedResourceTransitionDecision {
            changed: next_state != *self,
            next_state,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn begin(revision: u64) -> ObservedResourceCommand<String> {
        ObservedResourceCommand::Begin {
            expected_revision: revision,
            operation: StateOperation::Discover,
            operation_id: "discover-1".to_string(),
            started_at: 2,
        }
    }

    #[test]
    fn first_failure_has_no_value_and_refresh_failure_is_degraded() {
        let empty = ObservedResource::<String>::uninitialized(1);
        let loading = empty.decide(begin(0)).unwrap().next_state;
        let failed = loading
            .decide(ObservedResourceCommand::Fail {
                expected_revision: 1,
                failed_at: 3,
                error: StateError {
                    code: "offline".to_string(),
                    message: "offline".to_string(),
                    retryable: true,
                },
            })
            .unwrap()
            .next_state;
        assert_eq!(failed.kind(), ObservedResourceKind::Failed);
        assert_eq!(failed.value(), None);

        let loading = failed.decide(begin(2)).unwrap().next_state;
        let ready = loading
            .decide(ObservedResourceCommand::Succeed {
                expected_revision: 3,
                updated_at: 4,
                last_checked_at: Some(4),
                value: "catalog".to_string(),
            })
            .unwrap()
            .next_state;
        let refreshing = ready.decide(begin(4)).unwrap().next_state;
        let degraded = refreshing
            .decide(ObservedResourceCommand::Fail {
                expected_revision: 5,
                failed_at: 6,
                error: StateError {
                    code: "offline".to_string(),
                    message: "offline".to_string(),
                    retryable: true,
                },
            })
            .unwrap()
            .next_state;
        assert_eq!(degraded.kind(), ObservedResourceKind::Degraded);
        assert_eq!(degraded.value().map(String::as_str), Some("catalog"));
    }

    #[test]
    fn serde_is_tagged_and_legacy_meta_is_rejected() {
        let state = ObservedResource::ready(1, 2, "value".to_string());
        let encoded = serde_json::to_string(&state).unwrap();
        assert_eq!(
            serde_json::from_str::<ObservedResource<String>>(&encoded).unwrap(),
            state
        );
        assert!(
            serde_json::from_str::<ObservedResource<String>>(
                r#"{"revision":1,"phase":"ready","stale":false}"#
            )
            .is_err()
        );
    }

    #[test]
    fn stale_revision_and_terminal_transition_are_rejected() {
        let ready = ObservedResource::ready(2, 1, "value".to_string());
        assert!(matches!(
            ready.decide(begin(1)),
            Err(ObservedResourceTransitionError::StaleRevision { .. })
        ));
        let stopped = ready
            .decide(ObservedResourceCommand::Stop {
                expected_revision: 2,
                stopped_at: 3,
            })
            .unwrap()
            .next_state;
        assert!(matches!(
            stopped.decide(begin(3)),
            Err(ObservedResourceTransitionError::IllegalTransition { .. })
        ));
    }
}
