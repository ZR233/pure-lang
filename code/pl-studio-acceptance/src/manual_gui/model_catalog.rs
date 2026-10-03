//! Optional discovery instances for directed native GUI observation.
use anyhow::{Result, ensure};
use pl_model::config::{ProviderId, builtin_provider_catalog};
use pl_studio_runtime::config::StudioConfig;
use std::{fs, path::Path};

pub(super) fn configure(home: &Path, base_url: &str) -> Result<()> {
    // Accept only an explicit IPv4 loopback endpoint; never consult real credentials.
    let remainder = base_url
        .strip_prefix("http://127.0.0.1:")
        .ok_or_else(|| anyhow::anyhow!("model catalog fixture must use IPv4 loopback HTTP"))?;
    let port = remainder.split('/').next().unwrap_or_default();
    ensure!(
        port.parse::<u16>().is_ok_and(|port| port != 0),
        "invalid model catalog fixture port"
    );
    let file = home.join("config.toml");
    let mut config: StudioConfig = toml::from_str(&fs::read_to_string(&file)?)?;
    let catalog = builtin_provider_catalog()?;
    for (id, preset, suffix) in [
        ("catalog-openai", "openai", "openai"),
        ("catalog-openai-second", "openai", "openai"),
        ("catalog-deepseek", "deepseek", "deepseek"),
    ] {
        let preset = catalog
            .presets
            .iter()
            .find(|candidate| candidate.id.as_str() == preset)
            .ok_or_else(|| anyhow::anyhow!("model catalog fixture preset is missing"))?;
        let mut provider = preset.provider.clone();
        provider.name = id.to_owned();
        provider.base_url = format!("{}/{suffix}", base_url.trim_end_matches('/'));
        provider.bearer_token_env = None;
        provider.bearer_token = None;
        config
            .models
            .providers
            .insert(ProviderId::new(id)?, provider);
    }
    config.validate()?;
    fs::write(file, toml::to_string_pretty(&config)?)?;
    Ok(())
}
