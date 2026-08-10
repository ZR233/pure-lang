use pl_core::{
    ModelCatalogId, ProviderConnectionMode, ProviderWireProtocol, builtin_model_catalog,
};
use toml::Value;

use crate::{PureError, Result};

use super::{STUDIO_CONFIG_SCHEMA_VERSION, StudioConfig};

pub(super) const PREVIOUS_STUDIO_CONFIG_SCHEMA_VERSION: u32 = 12;

pub(super) struct ConfigMigration {
    pub(super) config: StudioConfig,
    pub(super) diagnostics: Vec<String>,
}

#[derive(Clone, Copy)]
struct LegacyTransport {
    protocol: ProviderWireProtocol,
    connection_mode: ProviderConnectionMode,
}

pub(super) fn schema_version(content: &str) -> Result<u32> {
    let root = toml::from_str::<toml::Table>(content).map_err(|error| {
        PureError::ConfigError(format!("failed to read Studio config schema: {error}"))
    })?;
    let version = root
        .get("schema_version")
        .and_then(Value::as_integer)
        .ok_or_else(|| {
            PureError::ConfigError("Studio config is missing integer schema_version".to_string())
        })?;
    u32::try_from(version).map_err(|_| {
        PureError::ConfigError(format!(
            "Studio config schema_version is out of range: {version}"
        ))
    })
}

pub(super) fn migrate_v12(content: &str) -> Result<ConfigMigration> {
    let mut root = toml::from_str::<toml::Table>(content).map_err(|error| {
        PureError::ConfigError(format!("failed to parse schema 12 Studio config: {error}"))
    })?;
    root.insert(
        "schema_version".to_string(),
        Value::Integer(i64::from(STUDIO_CONFIG_SCHEMA_VERSION)),
    );
    let providers = root
        .get_mut("models")
        .and_then(Value::as_table_mut)
        .and_then(|models| models.get_mut("providers"))
        .and_then(Value::as_table_mut)
        .ok_or_else(|| {
            PureError::ConfigError(
                "schema 12 Studio config is missing models.providers".to_string(),
            )
        })?;

    let mut diagnostics = Vec::new();
    for (provider_id, provider) in providers {
        migrate_provider(provider_id, provider, &mut diagnostics)?;
    }

    let config: StudioConfig = Value::Table(root).try_into().map_err(|error| {
        PureError::ConfigError(format!("failed to decode migrated Studio config: {error}"))
    })?;
    config.validate()?;
    Ok(ConfigMigration {
        config,
        diagnostics,
    })
}

fn migrate_provider(
    provider_id: &str,
    provider: &mut Value,
    diagnostics: &mut Vec<String>,
) -> Result<()> {
    let provider = provider.as_table_mut().ok_or_else(|| {
        PureError::ConfigError(format!(
            "schema 12 provider {provider_id} must be a TOML table"
        ))
    })?;
    let transport = provider.remove("transport").ok_or_else(|| {
        PureError::ConfigError(format!(
            "schema 12 provider {provider_id} is missing transport"
        ))
    })?;
    let (legacy, preset) = legacy_transport(provider_id, &transport)?;
    if let Some(preset) = preset {
        provider.insert("preset".to_string(), Value::String(preset));
    }

    let catalog = provider
        .get_mut("catalog")
        .and_then(Value::as_table_mut)
        .ok_or_else(|| {
            PureError::ConfigError(format!(
                "schema 12 provider {provider_id} is missing catalog"
            ))
        })?;
    match string_field(catalog, "source", provider_id)? {
        "bundled" => migrate_bundled_catalog(provider_id, catalog, legacy, diagnostics),
        "explicit" => migrate_explicit_models(provider_id, catalog, legacy, diagnostics),
        source => Err(PureError::ConfigError(format!(
            "schema 12 provider {provider_id} has unsupported catalog source: {source}"
        ))),
    }
}

fn legacy_transport(
    provider_id: &str,
    transport: &Value,
) -> Result<(LegacyTransport, Option<String>)> {
    let transport = transport.as_table().ok_or_else(|| {
        PureError::ConfigError(format!(
            "schema 12 provider {provider_id} transport must be a TOML table"
        ))
    })?;
    let connection_mode =
        parse_connection_mode(string_field(transport, "connection_mode", provider_id)?)?;
    match string_field(transport, "source", provider_id)? {
        "preset" => {
            let preset = string_field(transport, "preset", provider_id)?.to_string();
            Ok((
                LegacyTransport {
                    protocol: legacy_preset_protocol(&preset)?,
                    connection_mode,
                },
                Some(preset),
            ))
        }
        "custom" => Ok((
            LegacyTransport {
                protocol: parse_protocol(string_field(transport, "protocol", provider_id)?)?,
                connection_mode,
            },
            None,
        )),
        source => Err(PureError::ConfigError(format!(
            "schema 12 provider {provider_id} has unsupported transport source: {source}"
        ))),
    }
}

fn migrate_bundled_catalog(
    provider_id: &str,
    catalog: &mut toml::map::Map<String, Value>,
    legacy: LegacyTransport,
    diagnostics: &mut Vec<String>,
) -> Result<()> {
    let catalog_id = string_field(catalog, "catalog", provider_id)?;
    let catalog_id = ModelCatalogId::new(catalog_id)?;
    let bundled = builtin_model_catalog(&catalog_id)?;
    let mut overrides = toml::map::Map::new();
    for model in bundled.models {
        if model
            .transport
            .supported_connection_modes
            .contains(&legacy.connection_mode)
        {
            if model.transport.default_connection_mode != legacy.connection_mode {
                overrides.insert(
                    model.slug,
                    Value::String(connection_mode_label(legacy.connection_mode).to_string()),
                );
            }
        } else {
            diagnostics.push(format!(
                "schema 12 provider {provider_id} model {} does not support {}; using {}",
                model.slug,
                connection_mode_label(legacy.connection_mode),
                connection_mode_label(model.transport.default_connection_mode),
            ));
        }
    }
    if !overrides.is_empty() {
        catalog.insert("connection_overrides".to_string(), Value::Table(overrides));
    }
    if let Some(models) = catalog.get_mut("additional_models") {
        inject_legacy_transport(provider_id, models, legacy, diagnostics)?;
    }
    Ok(())
}

fn migrate_explicit_models(
    provider_id: &str,
    catalog: &mut toml::map::Map<String, Value>,
    legacy: LegacyTransport,
    diagnostics: &mut Vec<String>,
) -> Result<()> {
    let models = catalog.get_mut("models").ok_or_else(|| {
        PureError::ConfigError(format!(
            "schema 12 provider {provider_id} explicit catalog is missing models"
        ))
    })?;
    inject_legacy_transport(provider_id, models, legacy, diagnostics)
}

fn inject_legacy_transport(
    provider_id: &str,
    models: &mut Value,
    legacy: LegacyTransport,
    diagnostics: &mut Vec<String>,
) -> Result<()> {
    let models = models.as_array_mut().ok_or_else(|| {
        PureError::ConfigError(format!(
            "schema 12 provider {provider_id} custom models must be an array"
        ))
    })?;
    let effective = compatible_legacy_transport(provider_id, legacy, diagnostics);
    for model in models {
        let model = model.as_table_mut().ok_or_else(|| {
            PureError::ConfigError(format!(
                "schema 12 provider {provider_id} contains a non-table model"
            ))
        })?;
        if !model.contains_key("transport") {
            model.insert(
                "transport".to_string(),
                model_transport_value(effective.protocol, effective.connection_mode),
            );
        }
    }
    Ok(())
}

fn compatible_legacy_transport(
    provider_id: &str,
    legacy: LegacyTransport,
    diagnostics: &mut Vec<String>,
) -> LegacyTransport {
    if legacy.protocol == ProviderWireProtocol::ChatCompletions
        && legacy.connection_mode == ProviderConnectionMode::WebSocket
    {
        diagnostics.push(format!(
            "schema 12 provider {provider_id} used chat_completions with web_socket; using http"
        ));
        return LegacyTransport {
            connection_mode: ProviderConnectionMode::Http,
            ..legacy
        };
    }
    legacy
}

fn model_transport_value(
    protocol: ProviderWireProtocol,
    default_connection_mode: ProviderConnectionMode,
) -> Value {
    let supported = match protocol {
        ProviderWireProtocol::Responses => vec![
            Value::String("web_socket".to_string()),
            Value::String("http".to_string()),
        ],
        ProviderWireProtocol::ChatCompletions => vec![Value::String("http".to_string())],
    };
    Value::Table(toml::map::Map::from_iter([
        (
            "protocol".to_string(),
            Value::String(protocol_label(protocol).to_string()),
        ),
        (
            "supported_connection_modes".to_string(),
            Value::Array(supported),
        ),
        (
            "default_connection_mode".to_string(),
            Value::String(connection_mode_label(default_connection_mode).to_string()),
        ),
    ]))
}

fn string_field<'a>(
    table: &'a toml::map::Map<String, Value>,
    field: &str,
    provider_id: &str,
) -> Result<&'a str> {
    table.get(field).and_then(Value::as_str).ok_or_else(|| {
        PureError::ConfigError(format!(
            "schema 12 provider {provider_id} is missing string field {field}"
        ))
    })
}

fn legacy_preset_protocol(preset: &str) -> Result<ProviderWireProtocol> {
    match preset {
        "openai" => Ok(ProviderWireProtocol::Responses),
        "deepseek" | "zhipu" | "zhipu-coding-plan" | "mimo-api" | "mimo-token-plan" => {
            Ok(ProviderWireProtocol::ChatCompletions)
        }
        _ => Err(PureError::ConfigError(format!(
            "schema 12 config references unknown provider preset: {preset}"
        ))),
    }
}

fn parse_protocol(value: &str) -> Result<ProviderWireProtocol> {
    match value {
        "responses" => Ok(ProviderWireProtocol::Responses),
        "chat_completions" => Ok(ProviderWireProtocol::ChatCompletions),
        _ => Err(PureError::ConfigError(format!(
            "unsupported schema 12 provider protocol: {value}"
        ))),
    }
}

fn parse_connection_mode(value: &str) -> Result<ProviderConnectionMode> {
    match value {
        "web_socket" => Ok(ProviderConnectionMode::WebSocket),
        "http" => Ok(ProviderConnectionMode::Http),
        _ => Err(PureError::ConfigError(format!(
            "unsupported schema 12 provider connection mode: {value}"
        ))),
    }
}

fn protocol_label(protocol: ProviderWireProtocol) -> &'static str {
    match protocol {
        ProviderWireProtocol::Responses => "responses",
        ProviderWireProtocol::ChatCompletions => "chat_completions",
    }
}

fn connection_mode_label(mode: ProviderConnectionMode) -> &'static str {
    match mode {
        ProviderConnectionMode::WebSocket => "web_socket",
        ProviderConnectionMode::Http => "http",
    }
}
