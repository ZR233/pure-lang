//! Agent 框架可组合的 serde 配置值对象。
//!
//! 本模块不定义产品 schema version、默认角色、文件路径或 TOML/JSON 读写。

mod instruction;
mod mcp;
mod runtime;

pub use crate::model_config::ReasoningEffort;
pub use instruction::*;
pub use mcp::{
    BuiltinMcpServerState, EffectiveMcpServerConfig, McpServerConfig, McpServerMutationPolicy,
    McpServerSourceKind, McpServerStatusKind, McpServerTransport, active_mcp_server_names,
    builtin_mcp_server_ids, effective_mcp_servers, is_builtin_mcp_server_id,
    normalize_builtin_mcp_server_states, validate_builtin_mcp_server_states,
    validate_mcp_identifier, validate_mcp_servers, zhipu_coding_plan_token,
};
pub use runtime::*;
