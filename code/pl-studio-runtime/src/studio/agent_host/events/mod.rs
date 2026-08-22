//! 把已提交的 framework event/trace 投影到 Studio durable event stream。
//!
//! 职责划分:
//! - `observer`: framework 提交事件的入口 observer 与 attach/detach 恢复;
//! - `projector`: runtime/trace 事件到 store 与产品事件的投影;
//! - `continuation`: executor budget continuation 的提交与恢复;
//! - `planner_wake`: Task Planner wake 的 materialize;
//! - `mapping`: agent snapshot 到 Studio 状态标签的纯映射。

mod continuation;
mod mapping;
mod observer;
mod planner_wake;
mod projector;

pub(crate) use mapping::{progress_stage_from_label, progress_stage_label};
pub(in crate::studio) use observer::StudioAgentCommitObserver;
pub(in crate::studio) use planner_wake::{
    materialize_pending_task_planner_wakes, materialize_task_planner_wake,
};
