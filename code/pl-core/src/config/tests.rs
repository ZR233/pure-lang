use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use pl_model::{
    OpenAiCompactionMode, ProviderInfo, ZHIPU_CODING_PLAN_BASE_URL, default_models,
    zhipu_default_model_slugs,
};

use super::role::{ModelRole, ReasoningEffort};
use super::runtime::SkillsConfig;
use super::runtime::ToolCapabilityConfig;
use super::store::{ConfigPaths, ConfigStore};
use super::{
    BuiltinMcpServerState, CONFIG_SCHEMA_VERSION, DEFAULT_MODEL, McpServerConfig,
    McpServerStatusKind, McpServerTransport, PureConfig, active_mcp_server_names,
    effective_mcp_servers, zhipu_coding_plan_token,
};
use crate::turn::PermissionMode;

fn temp_home(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    env::temp_dir().join(format!("pure-lang-{name}-{}-{stamp}", std::process::id()))
}

fn without_section(content: &str, section: &str) -> String {
    let mut filtered = Vec::new();
    let mut skipping = false;
    for line in content.lines() {
        if line.trim() == section {
            skipping = true;
            continue;
        }
        if skipping && line.starts_with('[') {
            skipping = false;
        }
        if !skipping {
            filtered.push(line);
        }
    }
    filtered.join("\n")
}

#[test]
fn hosted_container_workspace_capabilities_match_hosted_agent_surface() {
    let capabilities = ToolCapabilityConfig::hosted_container_workspace();

    assert_eq!(
        capabilities,
        ToolCapabilityConfig {
            bash: false,
            workspace_files: true,
            skills: false,
            mcp: true,
            lsp: false,
            subagents: true,
            ask_user: true,
            git: true,
            docker: false,
            container: true,
        }
    );
}

#[test]
fn git_workspace_capabilities_match_git_only_tool_surface() {
    let capabilities = ToolCapabilityConfig::git_workspace();

    assert_eq!(
        capabilities,
        ToolCapabilityConfig {
            bash: false,
            workspace_files: false,
            skills: false,
            mcp: false,
            lsp: false,
            subagents: false,
            ask_user: false,
            git: true,
            docker: false,
            container: false,
        }
    );
}

#[test]
fn default_path_uses_pure_directory_under_home() {
    let paths = ConfigPaths::from_home("C:/Users/example");

    assert!(paths.config_file().ends_with(".pure/config.toml"));
}

#[test]
fn missing_config_loads_default_four_roles() {
    let store = ConfigStore::new(ConfigPaths::from_home(temp_home("missing")));
    let config = store.load_or_default().unwrap();

    assert_eq!(config.role_config(ModelRole::Planner).provider, "deepseek");
    assert_eq!(
        config.role_config(ModelRole::Explorer).effort.as_str(),
        "high"
    );
    assert_eq!(config.providers["deepseek"].models.len(), 2);
    assert!(
        config.providers["deepseek"]
            .models
            .iter()
            .any(|model| model.slug == "deepseek-v4-pro")
    );
    assert_eq!(config.skills, SkillsConfig::default());
}

#[test]
fn toml_round_trip_preserves_roles_models_and_token() {
    let mut config = PureConfig::default_config();
    config.providers.get_mut("deepseek").unwrap().bearer_token = Some("secret-token".to_string());
    config.runtime.permission_mode = PermissionMode::AutoReview;
    config.runtime.openai_compaction_mode = OpenAiCompactionMode::RemoteLegacy;
    config.runtime.active_skills = vec!["rust".to_string(), "git".to_string()];
    config.runtime.active_mcp_servers = vec!["github".to_string()];
    config.runtime.tool_capabilities.git = true;
    config.runtime.tool_capabilities.docker = true;
    config.runtime.tool_capabilities.mcp = false;
    config.mcp_servers.insert(
        "filesystem".to_string(),
        McpServerConfig {
            command: Some("npx".to_string()),
            args: vec![
                "-y".to_string(),
                "@modelcontextprotocol/server-filesystem".to_string(),
            ],
            ..Default::default()
        },
    );
    config.mcp_servers.insert(
        "github".to_string(),
        McpServerConfig {
            transport: McpServerTransport::StreamableHttp,
            url: Some("https://example.com/mcp".to_string()),
            bearer_token_env_var: Some("GITHUB_MCP_TOKEN".to_string()),
            ..Default::default()
        },
    );
    config.skills.auto_learn = false;
    config.skills.disabled = vec!["old-flow".to_string()];
    let model = &mut config.providers.get_mut("deepseek").unwrap().models[0];
    model.currency = Some("CNY".to_string());
    model.input_price_per_mtok = Some(1.0);
    model.output_price_per_mtok = Some(2.0);
    model.cache_read_price_per_mtok = Some(0.02);

    let toml = config.to_toml_pretty().unwrap();
    let parsed = PureConfig::from_toml(&toml).unwrap();

    assert_eq!(
        parsed.providers["deepseek"].bearer_token.as_deref(),
        Some("secret-token")
    );
    assert_eq!(parsed.role_config(ModelRole::Reviewer).model, DEFAULT_MODEL);
    assert_eq!(
        parsed.providers["deepseek"].models[0].capabilities,
        config.providers["deepseek"].models[0].capabilities
    );
    assert_eq!(parsed.runtime.permission_mode, PermissionMode::AutoReview);
    assert_eq!(
        parsed.runtime.openai_compaction_mode,
        OpenAiCompactionMode::RemoteLegacy
    );
    assert_eq!(
        parsed.runtime.active_skills,
        vec!["rust".to_string(), "git".to_string()]
    );
    assert_eq!(
        parsed.runtime.active_mcp_servers,
        vec!["github".to_string()]
    );
    assert!(parsed.runtime.tool_capabilities.git);
    assert!(parsed.runtime.tool_capabilities.docker);
    assert!(!parsed.runtime.tool_capabilities.mcp);
    assert_eq!(
        active_mcp_server_names(&parsed),
        vec!["filesystem".to_string(), "github".to_string()]
    );
    assert_eq!(
        parsed.mcp_servers["filesystem"].command.as_deref(),
        Some("npx")
    );
    assert_eq!(
        parsed.mcp_servers["github"].transport,
        McpServerTransport::StreamableHttp
    );
    assert!(!parsed.skills.auto_learn);
    assert_eq!(parsed.skills.disabled, vec!["old-flow".to_string()]);
    assert_eq!(
        parsed.providers["deepseek"].models[0].currency.as_deref(),
        Some("CNY")
    );
    assert_eq!(
        parsed.providers["deepseek"].models[0].input_price_per_mtok,
        Some(1.0)
    );
}

#[test]
fn mcp_config_rejects_invalid_server_id() {
    let mut config = PureConfig::default_config();
    config.mcp_servers.insert(
        "bad server".to_string(),
        McpServerConfig {
            command: Some("npx".to_string()),
            ..Default::default()
        },
    );

    let error = config.validate().unwrap_err().to_string();

    assert!(error.contains("MCP server id"));
}

#[test]
fn mcp_config_rejects_enabled_stdio_without_command() {
    let mut config = PureConfig::default_config();
    config
        .mcp_servers
        .insert("filesystem".to_string(), McpServerConfig::default());

    let error = config.validate().unwrap_err().to_string();

    assert!(error.contains("stdio command is required"));
}

#[test]
fn mcp_config_rejects_enabled_http_without_url() {
    let mut config = PureConfig::default_config();
    config.mcp_servers.insert(
        "github".to_string(),
        McpServerConfig {
            transport: McpServerTransport::StreamableHttp,
            ..Default::default()
        },
    );

    let error = config.validate().unwrap_err().to_string();

    assert!(error.contains("streamable HTTP url is required"));
}

#[test]
fn disabled_mcp_server_can_keep_incomplete_draft() {
    let mut config = PureConfig::default_config();
    config.mcp_servers.insert(
        "draft".to_string(),
        McpServerConfig {
            enabled: false,
            ..Default::default()
        },
    );

    config.validate().unwrap();
    assert!(active_mcp_server_names(&config).is_empty());
}

#[test]
fn default_config_does_not_serialize_builtin_mcp_servers() {
    let toml = PureConfig::default_config().to_toml_pretty().unwrap();

    assert!(!toml.contains("builtin_mcp_servers"));
    assert!(!toml.contains("zhipu_search"));
}

#[test]
fn effective_mcp_servers_include_builtin_servers_without_token_as_missing_credential() {
    let config = PureConfig::default_config();

    let servers = effective_mcp_servers(&config);

    assert_eq!(
        servers["zhipu_search"].status_kind,
        McpServerStatusKind::MissingCredential
    );
    assert_eq!(
        servers["zhipu_vision"].status_kind,
        McpServerStatusKind::MissingCredential
    );
    assert!(active_mcp_server_names(&config).is_empty());
}

#[test]
fn effective_mcp_servers_enable_builtin_servers_with_zhipu_token() {
    let mut config = PureConfig::default_config();
    let mut info = ProviderInfo::zhipu(None);
    info.bearer_token = Some("zhipu-coding-plan-key".to_string());
    let slugs = zhipu_default_model_slugs();
    let models = default_models()
        .into_iter()
        .filter(|model| slugs.contains(&model.slug.as_str()))
        .collect();
    config.providers.insert(
        "zhipu".to_string(),
        super::ProviderConfig::from_provider_info(info, models),
    );

    let servers = effective_mcp_servers(&config);

    assert_eq!(
        servers["zhipu_search"].status_kind,
        McpServerStatusKind::Enabled
    );
    assert_eq!(
        servers["zhipu_search"].bearer_token.as_deref(),
        Some("zhipu-coding-plan-key")
    );
    assert_eq!(
        servers["zhipu_vision"].config.env.get("Z_AI_API_KEY"),
        Some(&"zhipu-coding-plan-key".to_string())
    );
    assert_eq!(
        servers["zhipu_vision"].config.env.get("Z_AI_MODE"),
        Some(&"ZHIPU".to_string())
    );
    assert_eq!(
        servers["zhipu_vision"].config.command.as_deref(),
        Some(zhipu_vision_command())
    );
    assert_eq!(
        active_mcp_server_names(&config),
        vec![
            "zhipu_reader".to_string(),
            "zhipu_search".to_string(),
            "zhipu_vision".to_string(),
            "zhipu_zread".to_string()
        ]
    );
}

#[test]
fn effective_mcp_servers_respect_disabled_builtin_state() {
    let mut config = PureConfig::default_config();
    let mut info = ProviderInfo::zhipu(None);
    info.bearer_token = Some("zhipu-coding-plan-key".to_string());
    let slugs = zhipu_default_model_slugs();
    let models = default_models()
        .into_iter()
        .filter(|model| slugs.contains(&model.slug.as_str()))
        .collect();
    config.providers.insert(
        "zhipu".to_string(),
        super::ProviderConfig::from_provider_info(info, models),
    );
    config.builtin_mcp_servers.insert(
        "zhipu_search".to_string(),
        BuiltinMcpServerState { enabled: false },
    );

    let servers = effective_mcp_servers(&config);

    assert_eq!(
        servers["zhipu_search"].status_kind,
        McpServerStatusKind::Disabled
    );
    assert_eq!(
        active_mcp_server_names(&config),
        vec![
            "zhipu_reader".to_string(),
            "zhipu_vision".to_string(),
            "zhipu_zread".to_string()
        ]
    );
}

fn zhipu_vision_command() -> &'static str {
    if cfg!(windows) { "npx.cmd" } else { "npx" }
}

#[test]
fn builtin_mcp_servers_prefer_zhipu_coding_plan_token() {
    let mut config = PureConfig::default_config();
    let slugs = zhipu_default_model_slugs();
    let models = default_models()
        .into_iter()
        .filter(|model| slugs.contains(&model.slug.as_str()))
        .collect::<Vec<_>>();
    let mut zhipu = ProviderInfo::zhipu(None);
    zhipu.bearer_token = Some("normal-zhipu-key".to_string());
    let mut coding_plan = ProviderInfo::zhipu_coding_plan(None);
    coding_plan.bearer_token = Some("coding-plan-key".to_string());
    config.providers.insert(
        "zhipu".to_string(),
        super::ProviderConfig::from_provider_info(zhipu, models.clone()),
    );
    config.providers.insert(
        "coding-plan".to_string(),
        super::ProviderConfig::from_provider_info(coding_plan, models),
    );

    let servers = effective_mcp_servers(&config);

    assert_eq!(
        zhipu_coding_plan_token(&config).as_deref(),
        Some("coding-plan-key")
    );
    assert_eq!(
        servers["zhipu_search"].bearer_token.as_deref(),
        Some("coding-plan-key")
    );
    assert_eq!(
        servers["zhipu_vision"].config.env.get("Z_AI_API_KEY"),
        Some(&"coding-plan-key".to_string())
    );
    assert_eq!(
        config.providers["coding-plan"].base_url,
        ZHIPU_CODING_PLAN_BASE_URL
    );
}

#[test]
fn zhipu_token_restores_builtin_mcp_state_on_load() {
    let mut config = PureConfig::default_config();
    let mut info = ProviderInfo::zhipu(None);
    info.bearer_token = Some("zhipu-coding-plan-key".to_string());
    let slugs = zhipu_default_model_slugs();
    let models = default_models()
        .into_iter()
        .filter(|model| slugs.contains(&model.slug.as_str()))
        .collect();
    config.providers.insert(
        "zhipu".to_string(),
        super::ProviderConfig::from_provider_info(info, models),
    );
    config.builtin_mcp_servers.insert(
        "zhipu_search".to_string(),
        BuiltinMcpServerState { enabled: false },
    );

    let parsed = PureConfig::from_toml(&config.to_toml_pretty().unwrap()).unwrap();

    assert_eq!(
        parsed.builtin_mcp_servers["zhipu_search"],
        BuiltinMcpServerState { enabled: false }
    );
    assert_eq!(
        parsed.builtin_mcp_servers["zhipu_reader"],
        BuiltinMcpServerState { enabled: true }
    );
}

#[test]
fn mcp_config_rejects_builtin_reserved_id() {
    let mut config = PureConfig::default_config();
    config.mcp_servers.insert(
        "zhipu_search".to_string(),
        McpServerConfig {
            transport: McpServerTransport::StreamableHttp,
            url: Some("https://example.com/mcp".to_string()),
            ..Default::default()
        },
    );

    let error = config.validate().unwrap_err().to_string();

    assert!(error.contains("reserved"));
}

#[test]
fn missing_runtime_defaults_to_empty_lists() {
    let toml = PureConfig::default_config().to_toml_pretty().unwrap();
    let parsed = PureConfig::from_toml(&toml).unwrap();

    assert!(parsed.runtime.active_skills.is_empty());
    assert!(parsed.runtime.active_mcp_servers.is_empty());
    assert_eq!(
        parsed.runtime.tool_capabilities,
        ToolCapabilityConfig::default()
    );
    assert_eq!(
        parsed.runtime.permission_mode,
        PermissionMode::RequestApproval
    );
    assert_eq!(
        parsed.runtime.openai_compaction_mode,
        OpenAiCompactionMode::RemoteV2
    );
    assert_eq!(parsed.skills, SkillsConfig::default());
}

#[test]
fn skills_config_rejects_workspace_escape_project_dir() {
    let mut config = PureConfig::default_config();
    config.skills.project_dir = "../skills".to_string();

    let error = config.validate().unwrap_err().to_string();

    assert!(error.contains("project_dir"));
}

#[test]
fn missing_single_role_uses_first_provider_default_model() {
    let mut config = PureConfig::default_config();
    config.roles.reviewer.model = "deepseek-v4-pro".to_string();
    let toml = without_section(&config.to_toml_pretty().unwrap(), "[roles.reviewer]");

    let parsed = PureConfig::from_toml(&toml).unwrap();

    assert_eq!(parsed.roles.reviewer.provider, "deepseek");
    assert_eq!(parsed.roles.reviewer.model, "deepseek-v4-flash");
    assert_eq!(parsed.roles.reviewer.effort.as_str(), "high");
}

#[test]
fn missing_all_roles_uses_first_provider_default_model() {
    let mut toml = PureConfig::default_config().to_toml_pretty().unwrap();
    for section in [
        "[roles.explorer]",
        "[roles.planner]",
        "[roles.executor]",
        "[roles.reviewer]",
    ] {
        toml = without_section(&toml, section);
    }

    let parsed = PureConfig::from_toml(&toml).unwrap();

    for role in ModelRole::all() {
        assert_eq!(parsed.role_config(role).provider, "deepseek");
        assert_eq!(parsed.role_config(role).model, "deepseek-v4-flash");
    }
}

#[test]
fn complete_roles_do_not_require_default_model_effort_for_fallback() {
    let mut config = PureConfig::default_config();
    for role in ModelRole::all() {
        match role {
            ModelRole::Explorer => config.roles.explorer.model = "deepseek-v4-pro".to_string(),
            ModelRole::Planner => config.roles.planner.model = "deepseek-v4-pro".to_string(),
            ModelRole::Executor => config.roles.executor.model = "deepseek-v4-pro".to_string(),
            ModelRole::Reviewer => config.roles.reviewer.model = "deepseek-v4-pro".to_string(),
        }
    }
    config.providers.get_mut("deepseek").unwrap().models[0]
        .parameters
        .clear();

    let parsed = PureConfig::from_toml(&config.to_toml_pretty().unwrap()).unwrap();

    assert_eq!(
        parsed.role_config(ModelRole::Planner).model,
        "deepseek-v4-pro"
    );
}

#[test]
fn role_rejects_missing_model() {
    let mut config = PureConfig::default_config();
    config.roles.planner.model = "missing-model".to_string();

    let error = config.validate().unwrap_err().to_string();

    assert!(error.contains("missing model"));
}

#[test]
fn role_rejects_unsupported_effort() {
    let mut config = PureConfig::default_config();
    config.roles.planner.effort = ReasoningEffort::new("xhigh");

    let error = config.validate().unwrap_err().to_string();

    assert!(error.contains("unsupported effort"));
}

#[test]
fn init_default_writes_config_file() {
    let home = temp_home("init");
    let store = ConfigStore::new(ConfigPaths::from_home(&home));

    store.init_default().unwrap();

    assert!(home.join(".pure").join("config.toml").exists());
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn old_schema_toml_is_rejected_without_migration() {
    let mut old_config = PureConfig::default_config();
    old_config.schema_version = 2;

    let error = PureConfig::from_toml(&old_config.to_toml_pretty().unwrap())
        .unwrap_err()
        .to_string();

    assert!(error.contains("unsupported config schema version: 2"));
}

#[test]
fn legacy_model_capability_array_is_rejected() {
    let toml = r#"
schema_version = 3

[providers.deepseek]
provider_kind = "deep_seek"
name = "DeepSeek"
base_url = "https://api.deepseek.com"
default_model = "deepseek-v4-flash"

[[providers.deepseek.models]]
slug = "deepseek-v4-flash"
display_name = "DeepSeek V4 Flash"
reasoning_efforts = ["high"]
capabilities = ["streaming"]
input_modalities = ["text"]
truncation_policy = { mode = "tokens", limit = 10000 }
"#;

    let error = PureConfig::from_toml(toml).unwrap_err().to_string();

    assert!(error.contains("invalid type"));
}

#[test]
fn invalid_existing_config_is_backed_up_and_replaced_with_default() {
    let home = temp_home("old-schema");
    let store = ConfigStore::new(ConfigPaths::from_home(&home));
    let mut old_config = PureConfig::default_config();
    old_config.schema_version = 2;
    let old_toml = old_config.to_toml_pretty().unwrap();
    fs::create_dir_all(store.paths().config_dir()).unwrap();
    fs::write(store.paths().config_file(), &old_toml).unwrap();

    let config = store.load_or_default().unwrap();
    let repaired_toml = fs::read_to_string(store.paths().config_file()).unwrap();
    let backups = fs::read_dir(store.paths().config_dir())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("config.invalid.backup."))
        })
        .collect::<Vec<_>>();

    assert_eq!(config.schema_version, CONFIG_SCHEMA_VERSION);
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read_to_string(&backups[0]).unwrap(), old_toml);
    assert_eq!(
        PureConfig::from_toml(&repaired_toml).unwrap(),
        PureConfig::default_config()
    );
    fs::remove_dir_all(home).unwrap();
}
