use crate::api::studio::bridge_runtime::{active_bridge, installed_bridge};
use crate::api::studio::convert::settings::{
    bridge_deepseek_web_search_settings, bridge_model_catalog_snapshot, bridge_settings_snapshot,
    bridge_web_search_settings, provider_settings_request,
};
use crate::api::studio::types::{
    BridgeDeepSeekWebSearchSettingsDto, BridgeError, BridgeModelCatalogSnapshotDto,
    BridgeProviderCatalogSnapshot, BridgeSettingsStateResponse, BridgeWebSearchSettingsDto,
    ProviderSettingsInput, RemoveProviderInput, SettingsFieldInput,
};
// ── Settings ──

pub fn load_provider_catalog() -> Result<BridgeProviderCatalogSnapshot, BridgeError> {
    Ok(installed_bridge()?.studio.load_provider_catalog()?.into())
}

pub async fn read_web_search_settings() -> Result<BridgeWebSearchSettingsDto, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_web_search_settings(
        bridge.studio.read_settings()?.config.settings.web_search,
    ))
}

pub async fn read_deepseek_web_search_settings()
-> Result<BridgeDeepSeekWebSearchSettingsDto, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_deepseek_web_search_settings(
        bridge
            .studio
            .read_settings()?
            .config
            .settings
            .deepseek_web_search,
    ))
}

/// Applies one typed settings field/resource mutation. Sibling values are
/// resolved by the runtime from its canonical desired configuration.
pub async fn apply_settings_field(
    expected_settings_revision: u64,
    input: SettingsFieldInput,
) -> Result<BridgeSettingsStateResponse, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_settings_snapshot(
        bridge
            .studio
            .apply_settings_field(pl_protocol::studio::UpdateSettingsFieldRequest {
                expected_revision: expected_settings_revision,
                update: input.into(),
            })
            .await?,
    ))
}

pub async fn read_settings_state() -> Result<BridgeSettingsStateResponse, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_settings_snapshot(bridge.studio.read_settings()?))
}

pub async fn refresh_model_catalog(
    provider_id: String,
) -> Result<BridgeModelCatalogSnapshotDto, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_model_catalog_snapshot(
        bridge
            .studio
            .refresh_model_catalog(pl_protocol::studio::RefreshModelCatalogRequest { provider_id })
            .await?,
    ))
}

pub async fn reload_settings_from_disk(
    expected_settings_revision: u64,
) -> Result<BridgeSettingsStateResponse, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_settings_snapshot(
        bridge
            .studio
            .reload_settings(expected_settings_revision)
            .await?,
    ))
}

pub async fn save_runtime_permission_mode(
    expected_settings_revision: u64,
    mode: String,
) -> Result<BridgeSettingsStateResponse, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_settings_snapshot(
        bridge.studio.save_permission_settings(
            pl_protocol::studio::UpdatePermissionSettingsRequest {
                expected_revision: expected_settings_revision,
                mode,
            },
        )?,
    ))
}

pub async fn save_provider(
    expected_settings_revision: u64,
    input: ProviderSettingsInput,
) -> Result<BridgeSettingsStateResponse, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_settings_snapshot(
        bridge
            .studio
            .save_provider(provider_settings_request(expected_settings_revision, input))
            .await?,
    ))
}

pub async fn set_default_provider(
    expected_settings_revision: u64,
    provider_id: String,
) -> Result<BridgeSettingsStateResponse, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_settings_snapshot(
        bridge
            .studio
            .set_default_provider(pl_protocol::studio::SetDefaultProviderRequest {
                expected_revision: expected_settings_revision,
                provider_id,
            })?,
    ))
}

pub async fn remove_provider(
    expected_settings_revision: u64,
    provider_id: String,
    input: RemoveProviderInput,
) -> Result<BridgeSettingsStateResponse, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_settings_snapshot(bridge.studio.remove_provider(
        pl_protocol::studio::RemoveProviderRequest {
            expected_revision: expected_settings_revision,
            provider_id,
            replacement_provider_id: input.replacement_provider_id,
        },
    )?))
}
