//! Turn 主循环：目录页。
//!
//! `run` 持有模型 step 状态机；`attachments`/`checkpoint`/`compaction`/
//! `completion`/`enabled_tools`/`inference`/`prompt_cache`/`tool_results`/
//! `turn_setup` 是各阶段的支撑边界。

mod attachments;
mod checkpoint;
mod compaction;
mod completion;
pub(super) mod enabled_tools;
mod inference;
mod prompt_cache;
mod run;
mod tool_results;
mod turn_setup;

pub(super) use run::run_turn_with_trace;
