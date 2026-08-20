//! Pure Studio 产品配置。
//!
//! `pl-core` 只定义可 serde 的模型路由值对象；本模块组合 Studio 的运行时、
//! instructions、skills、MCP 与 UI 配置，并独占文件格式、schema 版本和默认角色。

mod credential;
mod mode;
mod runtime;
mod store;

use std::collections::BTreeMap;

use crate::{PureError, Result};
use pl_core::config::{
    BuiltinMcpServerState, InstructionsConfig, McpServerConfig, RuntimeConfig, SkillsConfig,
};
use pl_core::{AgentModelConfig, ProviderConfig};
use pl_model::WebSearchConfig;
use serde::{Deserialize, Serialize};

pub use mode::StudioMode;
pub use pl_core::config::{
    BuiltinMcpServerState as StudioBuiltinMcpServerState, EffectiveMcpServerConfig,
    McpServerConfig as StudioMcpServerEntry, McpServerMutationPolicy, McpServerSourceKind,
    McpServerStatusKind, McpServerTransport, builtin_mcp_server_ids, is_builtin_mcp_server_id,
    validate_mcp_identifier, zhipu_coding_plan_token,
};
pub use pl_core::{AgentRoleId, ModelRouteConfig, ProviderId, ReasoningEffort};
pub use runtime::{ConfigRuntime, ConfigRuntimeError, ConfigRuntimeSnapshot};
pub use store::{ConfigPaths, ConfigStore};

pub const STUDIO_CONFIG_SCHEMA_VERSION: u32 = 14;
pub const STUDIO_CONFIG_DIR_NAME: &str = ".pure";
pub const STUDIO_CONFIG_FILE_NAME: &str = "config.toml";
pub const CONFIG_DIR_NAME: &str = STUDIO_CONFIG_DIR_NAME;

const DEFAULT_PROVIDER_ID: &str = "deepseek";
const DEFAULT_MODEL_ID: &str = "deepseek-v4-flash";
const STUDIO_USER_SKILLS_DIR: &str = "~/.pure/skills";
const STUDIO_ROLES: [&str; 4] = ["explorer", "planner", "executor", "reviewer"];

/// Studio 产品定义的固定角色；框架层仍通过动态 `AgentRoleId` 接收它们。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StudioRole {
    Explorer,
    Planner,
    Executor,
    Reviewer,
}

impl StudioRole {
    pub const fn all() -> [Self; 4] {
        [
            Self::Explorer,
            Self::Planner,
            Self::Executor,
            Self::Reviewer,
        ]
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::Explorer => "explorer",
            Self::Planner => "planner",
            Self::Executor => "executor",
            Self::Reviewer => "reviewer",
        }
    }

    pub fn from_key(value: &str) -> Option<Self> {
        match value {
            "explorer" => Some(Self::Explorer),
            "planner" => Some(Self::Planner),
            "executor" => Some(Self::Executor),
            "reviewer" => Some(Self::Reviewer),
            _ => None,
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Explorer => "探索者",
            Self::Planner => "计划者",
            Self::Executor => "执行者",
            Self::Reviewer => "审查者",
        }
    }

    pub fn id(self) -> AgentRoleId {
        AgentRoleId::new(self.key()).expect("Studio 内置角色 id 必须有效")
    }
}

pub type StudioRuntimeConfig = RuntimeConfig;
pub type StudioInstructionsConfig = InstructionsConfig;
pub type StudioSkillsConfig = SkillsConfig;
pub type StudioWebSearchConfig = WebSearchConfig;
pub use pl_model::{WebSearchContextSize, WebSearchLocation, WebSearchMode};

/// Studio 自有的 MCP 配置段。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioMcpConfig {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub servers: BTreeMap<String, McpServerConfig>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub builtin_servers: BTreeMap<String, BuiltinMcpServerState>,
}

/// Studio 自有的 LSP 自定义 server 配置段（`[lsp.servers.<id>]`）。
///
/// 声明在 catalog 之外启动的命令式语言服务器；与内置 catalog 合并时冲突 fail-loud。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioLspConfig {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub servers: BTreeMap<String, pl_lsp::LspUserServerConfig>,
}

impl StudioLspConfig {
    fn is_default(&self) -> bool {
        self.servers.is_empty()
    }
}

/// Studio UI 本地偏好，由 [`ConfigRuntime`] 与其余 desired settings 一起持有。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioUiConfig {
    #[serde(default = "default_true")]
    pub follow_system_theme: bool,
    #[serde(default = "default_true")]
    pub follow_active_turn: bool,
    #[serde(default)]
    pub compact_timeline: bool,
}

impl Default for StudioUiConfig {
    fn default() -> Self {
        Self {
            follow_system_theme: true,
            follow_active_turn: true,
            compact_timeline: false,
        }
    }
}

impl StudioUiConfig {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

const fn default_true() -> bool {
    true
}

/// Pure Studio 的完整配置文档。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StudioConfig {
    pub schema_version: u32,
    pub models: AgentModelConfig,
    #[serde(default)]
    pub web_search: StudioWebSearchConfig,
    #[serde(default, skip_serializing_if = "RuntimeConfig::is_empty")]
    pub runtime: StudioRuntimeConfig,
    #[serde(default, skip_serializing_if = "InstructionsConfig::is_default")]
    pub instructions: StudioInstructionsConfig,
    #[serde(default, skip_serializing_if = "SkillsConfig::is_default")]
    pub skills: StudioSkillsConfig,
    #[serde(default)]
    pub mcp: StudioMcpConfig,
    #[serde(default, skip_serializing_if = "StudioLspConfig::is_default")]
    pub lsp: StudioLspConfig,
    #[serde(default, skip_serializing_if = "StudioUiConfig::is_default")]
    pub ui: StudioUiConfig,
}

impl StudioConfig {
    pub fn default_config() -> Self {
        let provider_id =
            ProviderId::new(DEFAULT_PROVIDER_ID).expect("Studio 内置 provider id 必须有效");
        let provider = ProviderConfig::deepseek_preset();
        let route = ModelRouteConfig {
            provider: provider_id.clone(),
            model: DEFAULT_MODEL_ID.to_string(),
            effort: Some(ReasoningEffort::new("high")),
        };
        let routes = STUDIO_ROLES
            .into_iter()
            .map(|role| {
                (
                    AgentRoleId::new(role).expect("Studio 内置角色 id 必须有效"),
                    route.clone(),
                )
            })
            .collect();
        let skills = StudioSkillsConfig {
            user_dir: STUDIO_USER_SKILLS_DIR.to_string(),
            ..StudioSkillsConfig::default()
        };

        Self {
            schema_version: STUDIO_CONFIG_SCHEMA_VERSION,
            models: AgentModelConfig {
                providers: BTreeMap::from([(provider_id, provider)]),
                routes,
            },
            web_search: StudioWebSearchConfig::default(),
            runtime: StudioRuntimeConfig::default(),
            instructions: StudioInstructionsConfig::default(),
            skills,
            mcp: StudioMcpConfig::default(),
            lsp: StudioLspConfig::default(),
            ui: StudioUiConfig::default(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != STUDIO_CONFIG_SCHEMA_VERSION {
            return Err(PureError::ConfigError(format!(
                "unsupported Studio config schema version: {}",
                self.schema_version
            )));
        }
        self.models.validate()?;
        for role in STUDIO_ROLES {
            self.models.resolve(&AgentRoleId::new(role)?)?;
        }
        pl_core::skill::validate_skills_config(&self.skills)?;
        pl_core::config::validate_mcp_servers(&self.mcp.servers)?;
        pl_core::config::validate_builtin_mcp_server_states(&self.mcp.builtin_servers)?;
        validate_lsp_servers(&self.lsp.servers)?;
        Ok(())
    }

    pub fn resolve_role(&self, role: StudioRole) -> Result<pl_core::ResolvedModelRoute> {
        self.models.resolve(&role.id())
    }
}

impl Default for StudioConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

/// 返回合并用户配置和 Studio 内置服务后的 MCP 运行时视图。
pub fn effective_mcp_servers(config: &StudioConfig) -> BTreeMap<String, EffectiveMcpServerConfig> {
    pl_core::config::effective_mcp_servers(
        &config.mcp.servers,
        &config.mcp.builtin_servers,
        &config.models,
    )
}

/// 返回当前可启动的 MCP server id。
pub fn active_mcp_server_names(config: &StudioConfig) -> Vec<String> {
    effective_mcp_servers(config)
        .into_iter()
        .filter(|(_, server)| server.status_kind == McpServerStatusKind::Enabled)
        .map(|(server_id, _)| server_id)
        .collect()
}

/// 移除内置 MCP 的冗余默认状态。
pub fn normalize_builtin_mcp_server_states(config: &mut StudioConfig) {
    pl_core::config::normalize_builtin_mcp_server_states(
        &mut config.mcp.builtin_servers,
        &config.models,
    );
}

/// 校验自定义 LSP server 与内置 catalog 的合并结果；冲突 fail-loud。
fn validate_lsp_servers(servers: &BTreeMap<String, pl_lsp::LspUserServerConfig>) -> Result<()> {
    pl_lsp::LspServerCatalog::with_user_servers(servers)
        .map(|_| ())
        .map_err(|error| {
            PureError::ConfigError(format!("invalid [lsp.servers] configuration: {error}"))
        })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn old_schema_is_rejected() {
        let mut config = StudioConfig::default_config();
        config.schema_version = 4;

        let error = config.validate().unwrap_err().to_string();

        assert!(error.contains("schema version"));
    }

    /// 旧 config（无 `[lsp]` 段）在 schema 14 下仍可加载与保存。
    #[test]
    fn legacy_config_without_lsp_section_still_loads() {
        let mut config = StudioConfig::default_config();
        config.lsp = StudioLspConfig::default();
        let content = toml::to_string_pretty(&config).unwrap();
        assert!(!content.contains("[lsp"));

        let parsed: StudioConfig = toml::from_str(&content).unwrap();

        assert_eq!(parsed.lsp, StudioLspConfig::default());
        parsed.validate().unwrap();
    }

    #[test]
    fn lsp_user_servers_section_round_trips() {
        let base = toml::to_string_pretty(&StudioConfig::default_config()).unwrap();
        let content = format!(
            "{base}\n[lsp.servers.purelang]\ncommand = \"purelang-lsp\"\nargs = [\"--stdio\"]\n\
             language_ids = [\"purelang\"]\ndetection = [\"pure.toml\"]\n\
             extensions = [\".purelang\"]\n"
        );
        let parsed: StudioConfig = toml::from_str(&content).unwrap();

        assert_eq!(parsed.lsp.servers.len(), 1);
        let server = &parsed.lsp.servers["purelang"];
        assert_eq!(server.command, "purelang-lsp");
        assert_eq!(server.language_ids, vec!["purelang".to_string()]);
        assert_eq!(server.detection, vec!["pure.toml".to_string()]);
        parsed.validate().unwrap();
    }

    #[test]
    fn lsp_language_conflict_fails_validation() {
        let mut config = StudioConfig::default_config();
        config.lsp.servers.insert(
            "custom-rust".to_string(),
            pl_lsp::LspUserServerConfig {
                command: "other-rust-server".to_string(),
                args: Vec::new(),
                language_ids: vec!["rust".to_string()],
                detection: Vec::new(),
                extensions: Vec::new(),
                display_name: None,
                operations: Vec::new(),
            },
        );

        let error = config.validate().unwrap_err().to_string();

        assert!(error.contains("[lsp.servers]"), "{error}");
        assert!(error.contains("rust"), "{error}");
    }
}
