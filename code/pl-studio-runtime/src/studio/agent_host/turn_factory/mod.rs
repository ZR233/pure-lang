//! 把 Studio 配置、project/session 与产品工具组装成一次 framework turn。
//!
//! 职责划分:
//! - `factory`: `StudioAgentTurnFactory` 的定义与构造;
//! - `prepare`: `AgentTurnFactory::prepare_turn` 的编排实现;
//! - `instructions`: 指令快照的组装上下文;
//! - `attachments`: prompt 消息内容与附件运行时;
//! - `interactions`: 交互事件发射器;
//! - `routing`: 冻结 Profile 路由解析与 Thread Mode 模型校验;
//! - `tools`: LSP 工具组构建;
//! - `errors`: turn 准备阶段的错误包装。

mod attachments;
mod errors;
mod factory;
mod instructions;
mod interactions;
mod prepare;
mod routing;
mod tools;

pub(in crate::studio) use factory::StudioAgentTurnFactory;
