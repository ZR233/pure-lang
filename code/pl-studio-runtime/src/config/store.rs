use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use pl_core::{
    AgentModelConfig, AgentRoleId, ModelRouteConfig, ProviderConfig, ProviderId,
    ProviderModelCatalogConfig, ProviderPresetId, builtin_provider_catalog,
};
use pl_model::{
    ApplyPatchToolType, ModelInfo, ProviderConnectionMode, ProviderInfo, ProviderWireProtocol,
    ToolWirePolicy,
};
use serde::Deserialize;

use crate::{PureError, Result};

use super::{
    STUDIO_CONFIG_DIR_NAME, STUDIO_CONFIG_FILE_NAME, STUDIO_CONFIG_SCHEMA_VERSION, StudioConfig,
    StudioInstructionsConfig, StudioMcpConfig, StudioRuntimeConfig, StudioSkillsConfig,
    StudioUiConfig,
};

const LEGACY_STUDIO_CONFIG_SCHEMA_VERSION: u32 = 5;
const CATALOG_STUDIO_CONFIG_SCHEMA_VERSION: u32 = 6;
const CONNECTION_STUDIO_CONFIG_SCHEMA_VERSION: u32 = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPaths {
    config_dir: PathBuf,
    config_file: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    paths: ConfigPaths,
}

impl ConfigPaths {
    pub fn for_current_user() -> Result<Self> {
        Ok(Self::from_home(user_home_dir()?))
    }

    pub fn from_home(home: impl Into<PathBuf>) -> Self {
        let config_dir = home.into().join(STUDIO_CONFIG_DIR_NAME);
        let config_file = config_dir.join(STUDIO_CONFIG_FILE_NAME);
        Self {
            config_dir,
            config_file,
        }
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn config_file(&self) -> &Path {
        &self.config_file
    }
}

impl ConfigStore {
    pub fn default_app() -> Result<Self> {
        Ok(Self::new(ConfigPaths::for_current_user()?))
    }

    pub fn new(paths: ConfigPaths) -> Self {
        Self { paths }
    }

    pub fn paths(&self) -> &ConfigPaths {
        &self.paths
    }

    pub fn config_exists(&self) -> bool {
        self.paths.config_file().exists()
    }

    pub fn load_or_default(&self) -> Result<StudioConfig> {
        if !self.config_exists() {
            return Ok(StudioConfig::default_config());
        }
        match self.load() {
            Ok(config) => Ok(config),
            Err(_) => {
                if let Some(config) = self.migrate_catalog_config()? {
                    return Ok(config);
                }
                match self.migrate_v5()? {
                    Some(config) => Ok(config),
                    None => self.reset_to_default(),
                }
            }
        }
    }

    pub fn load(&self) -> Result<StudioConfig> {
        let content = fs::read_to_string(self.paths.config_file())?;
        let config: StudioConfig = toml::from_str(&content).map_err(|error| {
            PureError::ConfigError(format!("failed to parse Studio config: {error}"))
        })?;
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, config: &StudioConfig) -> Result<()> {
        config.validate()?;
        let content = toml::to_string_pretty(config).map_err(|error| {
            PureError::ConfigError(format!("failed to serialize Studio config: {error}"))
        })?;
        fs::create_dir_all(self.paths.config_dir())?;
        let temporary = temporary_path(self.paths.config_file());
        let result = (|| -> Result<()> {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temporary, self.paths.config_file())?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    pub fn init_default(&self) -> Result<StudioConfig> {
        if self.config_exists() {
            return Err(PureError::ConfigError(format!(
                "config already exists: {}",
                self.paths.config_file().display()
            )));
        }
        let config = StudioConfig::default_config();
        self.save(&config)?;
        Ok(config)
    }

    fn reset_to_default(&self) -> Result<StudioConfig> {
        if self.config_exists() {
            fs::remove_file(self.paths.config_file())?;
        }
        let config = StudioConfig::default_config();
        self.save(&config)?;
        Ok(config)
    }

    fn migrate_v5(&self) -> Result<Option<StudioConfig>> {
        let content = fs::read_to_string(self.paths.config_file())?;
        let legacy: LegacyStudioConfig = match toml::from_str(&content) {
            Ok(config) => config,
            Err(_) => return Ok(None),
        };
        if legacy.schema_version != LEGACY_STUDIO_CONFIG_SCHEMA_VERSION {
            return Ok(None);
        }

        let providers = legacy
            .models
            .providers
            .into_iter()
            .map(|(id, provider)| Ok((id.clone(), migrate_provider(&id, provider)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        let config = StudioConfig {
            schema_version: STUDIO_CONFIG_SCHEMA_VERSION,
            models: AgentModelConfig {
                providers,
                routes: legacy.models.routes,
            },
            runtime: legacy.runtime,
            instructions: legacy.instructions,
            skills: legacy.skills,
            mcp: legacy.mcp,
            ui: legacy.ui,
        };
        config.validate()?;
        self.backup_v5()?;
        self.save(&config)?;
        Ok(Some(config))
    }

    fn migrate_catalog_config(&self) -> Result<Option<StudioConfig>> {
        let content = fs::read_to_string(self.paths.config_file())?;
        let legacy: LegacyCatalogStudioConfig = match toml::from_str(&content) {
            Ok(config) => config,
            Err(_) => return Ok(None),
        };
        if ![
            CATALOG_STUDIO_CONFIG_SCHEMA_VERSION,
            CONNECTION_STUDIO_CONFIG_SCHEMA_VERSION,
        ]
        .contains(&legacy.schema_version)
        {
            return Ok(None);
        }
        let version = legacy.schema_version;
        let providers = legacy
            .models
            .providers
            .into_iter()
            .map(|(id, provider)| {
                migrate_catalog_provider(&id, provider).map(|provider| (id, provider))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let config = StudioConfig {
            schema_version: STUDIO_CONFIG_SCHEMA_VERSION,
            models: AgentModelConfig {
                providers,
                routes: legacy.models.routes,
            },
            runtime: legacy.runtime,
            instructions: legacy.instructions,
            skills: legacy.skills,
            mcp: legacy.mcp,
            ui: legacy.ui,
        };
        config.validate()?;
        self.backup_schema(version)?;
        self.save(&config)?;
        Ok(Some(config))
    }

    fn backup_v5(&self) -> Result<()> {
        self.backup_schema(LEGACY_STUDIO_CONFIG_SCHEMA_VERSION)
    }

    fn backup_schema(&self, version: u32) -> Result<()> {
        let backup = schema_backup_path(self.paths.config_file(), version);
        if !backup.exists() {
            fs::copy(self.paths.config_file(), backup)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct LegacyStudioConfig {
    schema_version: u32,
    models: LegacyAgentModelConfig,
    #[serde(default)]
    runtime: StudioRuntimeConfig,
    #[serde(default)]
    instructions: StudioInstructionsConfig,
    #[serde(default)]
    skills: StudioSkillsConfig,
    #[serde(default)]
    mcp: StudioMcpConfig,
    #[serde(default)]
    ui: StudioUiConfig,
}

#[derive(Debug, Deserialize)]
struct LegacyAgentModelConfig {
    providers: BTreeMap<ProviderId, LegacyProviderConfig>,
    routes: BTreeMap<AgentRoleId, ModelRouteConfig>,
}

#[derive(Debug, Deserialize)]
struct LegacyProviderConfig {
    #[serde(default)]
    preset: Option<ProviderPresetId>,
    provider_kind: LegacyProviderKind,
    name: String,
    base_url: String,
    #[serde(default)]
    bearer_token: Option<String>,
    #[serde(default)]
    bearer_token_env: Option<String>,
    #[serde(default)]
    http_headers: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    tool_wire_policy: ToolWirePolicy,
    #[serde(default)]
    apply_patch_tool_type: Option<ApplyPatchToolType>,
    #[serde(default)]
    models: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyProviderKind {
    OpenAi,
    OpenAiCompatibleChat,
    DeepSeek,
    Zhipu,
}

#[derive(Debug, Deserialize)]
struct LegacyCatalogStudioConfig {
    schema_version: u32,
    models: LegacyCatalogAgentModelConfig,
    #[serde(default)]
    runtime: StudioRuntimeConfig,
    #[serde(default)]
    instructions: StudioInstructionsConfig,
    #[serde(default)]
    skills: StudioSkillsConfig,
    #[serde(default)]
    mcp: StudioMcpConfig,
    #[serde(default)]
    ui: StudioUiConfig,
}

#[derive(Debug, Deserialize)]
struct LegacyCatalogAgentModelConfig {
    providers: BTreeMap<ProviderId, LegacyCatalogProviderConfig>,
    routes: BTreeMap<AgentRoleId, ModelRouteConfig>,
}

#[derive(Debug, Deserialize)]
struct LegacyCatalogProviderConfig {
    #[serde(default)]
    preset: Option<ProviderPresetId>,
    provider_kind: LegacyProviderKind,
    #[serde(default)]
    connection_mode: Option<ProviderConnectionMode>,
    name: String,
    base_url: String,
    #[serde(default)]
    bearer_token: Option<String>,
    #[serde(default)]
    bearer_token_env: Option<String>,
    #[serde(default)]
    http_headers: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    tool_wire_policy: ToolWirePolicy,
    #[serde(default)]
    apply_patch_tool_type: Option<ApplyPatchToolType>,
    catalog: ProviderModelCatalogConfig,
}

fn migrate_provider(id: &ProviderId, legacy: LegacyProviderConfig) -> Result<ProviderConfig> {
    let info = ProviderInfo {
        protocol: legacy_protocol(legacy.provider_kind),
        connection_mode: default_connection_mode(legacy.preset.as_ref()),
        name: legacy.name,
        base_url: legacy.base_url,
        default_model: String::new(),
        bearer_token: legacy.bearer_token,
        http_headers: legacy.http_headers,
        tool_wire_policy: legacy.tool_wire_policy,
        apply_patch_tool_type: legacy.apply_patch_tool_type,
    };
    let registry = builtin_provider_catalog();
    let preset = registry.presets.into_iter().find(|preset| {
        legacy.preset.as_ref() == Some(&preset.id)
            || preset.id.as_str() == id.as_str()
            || (preset.protocol == info.protocol
                && normalized_url(&preset.provider.base_url) == normalized_url(&info.base_url))
    });
    let Some(preset) = preset else {
        let mut provider = ProviderConfig::from_explicit_models(info, legacy.models);
        provider.bearer_token_env = legacy.bearer_token_env;
        return Ok(provider);
    };

    let bundled_slugs = preset
        .provider
        .effective_models()?
        .into_iter()
        .map(|model| model.slug)
        .collect::<std::collections::BTreeSet<_>>();
    let additional_models = legacy
        .models
        .into_iter()
        .filter(|model| !bundled_slugs.contains(&model.slug))
        .collect();
    let mut provider = preset.provider;
    provider.name = info.name;
    provider.base_url = info.base_url;
    provider.bearer_token = info.bearer_token;
    provider.bearer_token_env = legacy.bearer_token_env.or(provider.bearer_token_env);
    provider.http_headers = info.http_headers;
    provider.tool_wire_policy = info.tool_wire_policy;
    provider.apply_patch_tool_type = info.apply_patch_tool_type;
    if let ProviderModelCatalogConfig::Bundled {
        additional_models: configured,
        ..
    } = &mut provider.catalog
    {
        *configured = additional_models;
    }
    Ok(provider)
}

fn migrate_catalog_provider(
    id: &ProviderId,
    legacy: LegacyCatalogProviderConfig,
) -> Result<ProviderConfig> {
    let protocol = legacy_protocol(legacy.provider_kind);
    let connection_mode = legacy
        .connection_mode
        .unwrap_or_else(|| default_connection_mode(legacy.preset.as_ref()));
    let info = ProviderInfo {
        protocol,
        connection_mode,
        name: legacy.name,
        base_url: legacy.base_url,
        default_model: String::new(),
        bearer_token: legacy.bearer_token,
        http_headers: legacy.http_headers,
        tool_wire_policy: legacy.tool_wire_policy,
        apply_patch_tool_type: legacy.apply_patch_tool_type,
    };
    let mut provider = match legacy.catalog {
        ProviderModelCatalogConfig::Bundled {
            catalog,
            additional_models,
        } => ProviderConfig::from_bundled_catalog(info, catalog, additional_models),
        ProviderModelCatalogConfig::Explicit { models } => {
            ProviderConfig::from_explicit_models(info, models)
        }
    };
    provider.bearer_token_env = legacy.bearer_token_env;
    let preset = legacy.preset.or_else(|| {
        builtin_provider_catalog()
            .presets
            .into_iter()
            .find(|preset| {
                preset.id.as_str() == id.as_str()
                    || (preset.protocol == protocol
                        && normalized_url(&preset.provider.base_url)
                            == normalized_url(&provider.base_url))
            })
            .map(|preset| preset.id)
    });
    if let Some(preset) = preset {
        provider = provider.with_preset(preset);
    }
    Ok(provider)
}

fn legacy_protocol(kind: LegacyProviderKind) -> ProviderWireProtocol {
    match kind {
        LegacyProviderKind::OpenAi => ProviderWireProtocol::Responses,
        LegacyProviderKind::OpenAiCompatibleChat
        | LegacyProviderKind::DeepSeek
        | LegacyProviderKind::Zhipu => ProviderWireProtocol::ChatCompletions,
    }
}

fn default_connection_mode(preset: Option<&ProviderPresetId>) -> ProviderConnectionMode {
    if preset.is_some_and(|preset| preset.as_str() == "openai") {
        ProviderConnectionMode::WebSocket
    } else {
        ProviderConnectionMode::Http
    }
}

fn normalized_url(value: &str) -> &str {
    value.trim().trim_end_matches('/')
}

fn schema_backup_path(path: &Path, version: u32) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(STUDIO_CONFIG_FILE_NAME);
    path.with_file_name(format!("{file_name}.schema{version}.bak"))
}

fn temporary_path(path: &Path) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    path.with_extension(format!("toml.tmp.{}.{stamp}", std::process::id()))
}

fn user_home_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    const HOME_VARS: &[&str] = &["USERPROFILE", "HOME"];
    #[cfg(not(windows))]
    const HOME_VARS: &[&str] = &["HOME", "USERPROFILE"];

    HOME_VARS
        .iter()
        .filter_map(env::var_os)
        .map(PathBuf::from)
        .find(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| PureError::ConfigError("could not resolve user home directory".to_string()))
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use pretty_assertions::assert_eq;

    use super::*;

    fn temp_home(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!(
            "pl-studio-config-{name}-{}-{stamp}",
            std::process::id()
        ))
    }

    #[test]
    fn save_uses_current_document_and_round_trips() {
        let store = ConfigStore::new(ConfigPaths::from_home(temp_home("round-trip")));
        let config = StudioConfig::default_config();

        store.save(&config).unwrap();

        assert_eq!(store.load().unwrap(), config);
    }

    #[test]
    fn old_document_is_deleted_and_recreated() {
        let store = ConfigStore::new(ConfigPaths::from_home(temp_home("reset")));
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        fs::write(
            store.paths().config_file(),
            "schema_version = 4\n[providers]\n",
        )
        .unwrap();

        let config = store.load_or_default().unwrap();

        assert_eq!(config.schema_version, STUDIO_CONFIG_SCHEMA_VERSION);
        assert_eq!(store.load().unwrap(), config);
    }

    #[test]
    fn schema_v5_document_is_backed_up_and_migrated_to_bundled_catalog() {
        let store = ConfigStore::new(ConfigPaths::from_home(temp_home("migrate-v5")));
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        let mut current = StudioConfig::default_config();
        let openai = builtin_provider_catalog()
            .presets
            .into_iter()
            .find(|preset| preset.id.as_str() == "openai")
            .unwrap()
            .provider;
        current
            .models
            .providers
            .insert(ProviderId::new("openai").unwrap(), openai);
        let provider_models = current.models.providers[&ProviderId::new("deepseek").unwrap()]
            .effective_models()
            .unwrap();
        let mut document = toml::Value::try_from(&current).unwrap();
        let root = document.as_table_mut().unwrap();
        root.insert(
            "schema_version".to_string(),
            toml::Value::Integer(i64::from(LEGACY_STUDIO_CONFIG_SCHEMA_VERSION)),
        );
        let providers = root["models"].as_table_mut().unwrap()["providers"]
            .as_table_mut()
            .unwrap();
        for (provider_id, provider) in providers.iter_mut() {
            let provider = provider.as_table_mut().unwrap();
            let transport = provider.remove("transport").unwrap();
            let transport = transport.as_table().unwrap();
            if transport["source"].as_str() == Some("preset") {
                provider.insert("preset".to_string(), transport["preset"].clone());
            }
            let provider_kind = match provider_id.as_str() {
                "openai" => "open_ai",
                "deepseek" => "deep_seek",
                "zhipu" => "zhipu",
                other => panic!("unexpected built-in provider in v5 fixture: {other}"),
            };
            provider.insert(
                "provider_kind".to_string(),
                toml::Value::String(provider_kind.to_string()),
            );
        }
        let provider = providers["deepseek"].as_table_mut().unwrap();
        provider.remove("catalog");
        provider.insert(
            "models".to_string(),
            toml::Value::try_from(provider_models).unwrap(),
        );
        fs::write(
            store.paths().config_file(),
            toml::to_string_pretty(&document).unwrap(),
        )
        .unwrap();

        let migrated = store.load_or_default().unwrap();

        assert_eq!(migrated.schema_version, STUDIO_CONFIG_SCHEMA_VERSION);
        assert!(schema_backup_path(store.paths().config_file(), 5).exists());
        assert_eq!(
            migrated.models.providers[&ProviderId::new("openai").unwrap()].connection_mode(),
            ProviderConnectionMode::WebSocket
        );
        assert!(matches!(
            migrated.models.providers[&ProviderId::new("deepseek").unwrap()].catalog,
            ProviderModelCatalogConfig::Bundled { .. }
        ));
        assert!(
            migrated.models.providers[&ProviderId::new("deepseek").unwrap()]
                .editable_models()
                .is_empty()
        );
    }
}
