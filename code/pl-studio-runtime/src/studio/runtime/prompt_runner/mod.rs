//! StudioRuntime 的 prompt 生命周期实现。
//!
//! 职责划分:
//! - `submit`: prompt/turn 提交入口、内容校验与 root 角色对齐;
//! - `stop`: prompt 停止与按预期 Turn 身份的中断;
//! - `activation`: Thread 对应 agent 的激活、驻留恢复与 canonical owner 读取;
//! - `interaction`: 交互读取、收束与重启后的 pending 交互恢复。

mod activation;
mod interaction;
mod stop;
mod submit;

pub(super) use submit::validate_prompt_content;
