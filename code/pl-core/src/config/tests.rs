use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::role::{ModelRole, ReasoningEffort};
use super::runtime::SkillsConfig;
use super::store::{ConfigPaths, ConfigStore};
use super::{
    CONFIG_SCHEMA_VERSION, DEFAULT_MODEL, McpServerConfig, McpServerTransport, PureConfig,
    active_mcp_server_names,
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
    config.runtime.active_skills = vec!["rust".to_string(), "git".to_string()];
    config.runtime.active_mcp_servers = vec!["github".to_string()];
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
        parsed.runtime.active_skills,
        vec!["rust".to_string(), "git".to_string()]
    );
    assert_eq!(
        parsed.runtime.active_mcp_servers,
        vec!["github".to_string()]
    );
    assert_eq!(
        active_mcp_server_names(&parsed.mcp_servers),
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
    assert!(active_mcp_server_names(&config.mcp_servers).is_empty());
}

#[test]
fn missing_runtime_defaults_to_empty_lists() {
    let toml = PureConfig::default_config().to_toml_pretty().unwrap();
    let parsed = PureConfig::from_toml(&toml).unwrap();

    assert!(parsed.runtime.active_skills.is_empty());
    assert!(parsed.runtime.active_mcp_servers.is_empty());
    assert_eq!(
        parsed.runtime.permission_mode,
        PermissionMode::RequestApproval
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
        .reasoning_efforts
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
