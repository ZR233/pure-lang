//! StudioRuntime 的 MCP 状态发布、reconcile 编排、健康映射与 fingerprint 计算。
//!
//! 职责划分:
//! - `state`: McpStateRuntime 状态 owner 与 publish_* 状态发布;
//! - `reconcile`: reconcile / reset 编排与后台健康 watcher;
//! - `health`: effective 配置与可用性快照到公开健康 DTO 的纯映射;
//! - `fingerprint`: effective / public fingerprint 计算。

mod fingerprint;
mod health;
mod reconcile;
mod state;

pub(super) use state::McpStateRuntime;
