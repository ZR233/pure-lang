//! 把已提交的 framework event/trace 投影到 Studio durable event stream。
//!
//! 职责划分:
//! - `observer`: framework 提交事件的入口 observer;
//! - `projector`: runtime event 到产品目录的投影;
//! - `mapping`: agent snapshot 到 Studio 状态标签的纯映射。

mod mapping;
mod observer;
mod projector;

pub(crate) use mapping::{progress_stage_from_label, progress_stage_label};
pub(in crate::studio) use observer::StudioAgentCommitObserver;
