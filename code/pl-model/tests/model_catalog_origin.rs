//! Effective catalog origin, membership and catalog-declared discovery.
use pl_model::{
    config::{
        ModelCatalogId, ProviderConfig, ProviderModelCatalogConfig, ProviderModelOrigin,
        builtin_model_catalog, model_descriptor,
    },
    model::{ModelInfo, ModelPricing, ModelTransportProfile, bundled_model_definition},
    provider::{
        ProviderAdapterKind, ProviderEndpoint,
        discovery::{ModelCatalogQuery, ModelCatalogQueryResult},
    },
};
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

struct Server {
    url: String,
    task: JoinHandle<Vec<String>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Server {
    async fn json(body: serde_json::Value) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let payload = body.to_string();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            while !bytes.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).await.unwrap();
                bytes.push(byte[0]);
                assert!(bytes.len() < 16_384);
            }
            let response = format!(
                "HTTP/1.1 200 Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            vec![String::from_utf8(bytes).unwrap()]
        });
        Self { url, task }
    }

    async fn finish(mut self) -> Vec<String> {
        tokio::time::timeout(Duration::from_secs(5), &mut self.task)
            .await
            .unwrap()
            .unwrap()
    }
}

fn openai_provider(url: &str) -> ProviderConfig {
    ProviderConfig::from_bundled_catalog(
        ProviderEndpoint::openai(Some(url.into())),
        ModelCatalogId::new("openai").unwrap(),
        Vec::new(),
    )
}

fn mimo_provider(url: &str) -> ProviderConfig {
    ProviderConfig::from_bundled_catalog(
        ProviderEndpoint::compatible("MiMo API", url).with_adapter(ProviderAdapterKind::MiMo),
        ModelCatalogId::new("mimo").unwrap(),
        Vec::new(),
    )
}

fn updated(result: ModelCatalogQueryResult) -> Vec<ModelInfo> {
    match result {
        ModelCatalogQueryResult::Updated { models, .. } => models,
        _ => panic!("expected updated catalog"),
    }
}

#[tokio::test]
async fn overlay_replaces_bundled_members_and_keeps_manual_priority_with_exact_price_join() {
    let definition = bundled_model_definition("openai").unwrap();
    let known = &definition.models[0];
    let withdrawn = definition
        .models
        .iter()
        .find(|model| model.slug != known.slug)
        .unwrap();
    let server = Server::json(json!({"data":[{"id": known.slug}, {"id":"online-only"}]})).await;
    let mut config = openai_provider(&server.url);
    let mut manual = ModelInfo::compatible("manual");
    manual.pricing = known.pricing.clone();
    if let ProviderModelCatalogConfig::Bundled {
        additional_models, ..
    } = &mut config.catalog
    {
        additional_models.push(manual.clone());
    }
    let observed = updated(
        ModelCatalogQuery::for_provider(&config)
            .unwrap()
            .execute(None)
            .await
            .unwrap(),
    );
    config.set_model_catalog_overlay(observed).unwrap();
    let catalog = config.effective_catalog().unwrap();
    assert!(
        catalog
            .iter()
            .all(|entry| entry.model.slug != withdrawn.slug)
    );
    let known_entry = catalog
        .iter()
        .find(|entry| entry.model.slug == known.slug)
        .unwrap();
    assert_eq!(known_entry.origin, ProviderModelOrigin::Online);
    assert_eq!(known_entry.model.pricing, known.pricing);
    let unknown = catalog
        .iter()
        .find(|entry| entry.model.slug == "online-only")
        .unwrap();
    assert_eq!(unknown.origin, ProviderModelOrigin::Online);
    assert_eq!(unknown.model.pricing, ModelPricing::Unknown);
    let manual_entry = catalog
        .iter()
        .find(|entry| entry.model.slug == "manual")
        .unwrap();
    assert_eq!(manual_entry.origin, ProviderModelOrigin::Manual);
    assert_eq!(manual_entry.model, manual);
    assert_eq!(
        model_descriptor(manual_entry).origin,
        pl_protocol::ModelOriginDto::Manual
    );
    server.finish().await;
}

#[test]
fn bundled_without_overlay_keeps_bundled_origin_and_manual_members() {
    let mut config = openai_provider("http://127.0.0.1:1");
    let manual = ModelInfo::compatible("manual");
    if let ProviderModelCatalogConfig::Bundled {
        additional_models, ..
    } = &mut config.catalog
    {
        additional_models.push(manual.clone());
    }
    let catalog = config.effective_catalog().unwrap();
    assert!(
        catalog
            .iter()
            .any(|entry| entry.origin == ProviderModelOrigin::Bundled)
    );
    assert!(catalog.iter().any(|entry| {
        entry.origin == ProviderModelOrigin::Manual && entry.model.slug == "manual"
    }));
    assert!(
        catalog
            .iter()
            .all(|entry| entry.origin != ProviderModelOrigin::Online)
    );
}

#[tokio::test]
async fn same_slug_manual_model_hides_online_entry() {
    let slug = "shared-online";
    let server = Server::json(json!({"data":[{"id": slug}]})).await;
    let mut config = openai_provider(&server.url);
    let mut manual = ModelInfo::compatible(slug);
    manual.display_name = "Authoritative manual".into();
    if let ProviderModelCatalogConfig::Bundled {
        additional_models, ..
    } = &mut config.catalog
    {
        additional_models.push(manual.clone());
    }
    let observed = updated(
        ModelCatalogQuery::for_provider(&config)
            .unwrap()
            .execute(None)
            .await
            .unwrap(),
    );
    config.set_model_catalog_overlay(observed).unwrap();
    let catalog = config.effective_catalog().unwrap();
    let matches: Vec<_> = catalog
        .iter()
        .filter(|entry| entry.model.slug == slug)
        .collect();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].origin, ProviderModelOrigin::Manual);
    assert_eq!(matches[0].model.display_name, "Authoritative manual");
    server.finish().await;
}

#[tokio::test]
async fn removing_same_slug_manual_restores_online_entry() {
    let definition = bundled_model_definition("openai").unwrap();
    let known = &definition.models[0];
    let slug = "shared-online";
    let server = Server::json(json!({"data":[{"id": slug}, {"id": known.slug}]})).await;
    let mut config = openai_provider(&server.url);
    let mut manual = ModelInfo::compatible(slug);
    manual.display_name = "Temporary manual".into();
    if let ProviderModelCatalogConfig::Bundled {
        additional_models, ..
    } = &mut config.catalog
    {
        additional_models.push(manual);
    }
    let observed = updated(
        ModelCatalogQuery::for_provider(&config)
            .unwrap()
            .execute(None)
            .await
            .unwrap(),
    );
    config.set_model_catalog_overlay(observed).unwrap();
    let before = config.effective_catalog().unwrap();
    let matches: Vec<_> = before
        .iter()
        .filter(|entry| entry.model.slug == slug)
        .collect();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].origin, ProviderModelOrigin::Manual);

    config.remove_manual_model(slug).unwrap();
    let after = config.effective_catalog().unwrap();
    let restored = after
        .iter()
        .filter(|entry| entry.model.slug == slug)
        .collect::<Vec<_>>();
    // Restored same-slug overlay members are not in the bundled membership/price
    // table, so pricing is Unknown. A bundled priced slug cannot be used as the
    // same-slug manual because catalog_membership rejects additional_models that
    // collide with bundled slugs. Exact price join is asserted on the in-overlay
    // bundled priced slug below.
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].origin, ProviderModelOrigin::Online);
    assert_eq!(restored[0].model.pricing, ModelPricing::Unknown);
    let priced = after
        .iter()
        .find(|entry| entry.model.slug == known.slug)
        .unwrap();
    assert_eq!(priced.origin, ProviderModelOrigin::Online);
    assert_eq!(priced.model.pricing, known.pricing);
    server.finish().await;
}

#[test]
fn explicit_catalog_is_entirely_manual() {
    let models = vec![ModelInfo::compatible("one"), ModelInfo::compatible("two")];
    let config =
        ProviderConfig::from_explicit_models(ProviderEndpoint::openai(None), models.clone());
    let catalog = config.effective_catalog().unwrap();
    assert_eq!(
        catalog
            .iter()
            .map(|entry| (entry.model.slug.as_str(), entry.origin))
            .collect::<Vec<_>>(),
        vec![
            ("one", ProviderModelOrigin::Manual),
            ("two", ProviderModelOrigin::Manual)
        ]
    );
    assert!(config.discovery_profile().unwrap().is_none());
}

#[test]
fn remove_manual_model_clears_overrides_and_rejects_non_manual_slugs() {
    let mut config = openai_provider("http://127.0.0.1:1");
    let bundled_slug = config.effective_models().unwrap()[0].slug.clone();
    let mut keep = ModelInfo::compatible("keep-manual");
    keep.display_name = "Keep".into();
    let mut remove = ModelInfo::compatible("remove-manual");
    remove.display_name = "Remove".into();
    if let ProviderModelCatalogConfig::Bundled {
        additional_models,
        connection_overrides,
        auto_compact_overrides,
        ..
    } = &mut config.catalog
    {
        additional_models.extend([keep.clone(), remove.clone()]);
        connection_overrides.insert(
            "remove-manual".into(),
            pl_model::provider::ProviderConnectionMode::Http,
        );
        connection_overrides.insert(
            "keep-manual".into(),
            pl_model::provider::ProviderConnectionMode::Http,
        );
        auto_compact_overrides.insert("remove-manual".into(), 1_234);
        auto_compact_overrides.insert("keep-manual".into(), 2_000);
    }
    let before = config.clone();
    config.remove_manual_model("remove-manual").unwrap();
    let catalog = config.effective_catalog().unwrap();
    assert!(
        catalog
            .iter()
            .all(|entry| entry.model.slug != "remove-manual")
    );
    assert!(catalog.iter().any(|entry| {
        entry.origin == ProviderModelOrigin::Manual && entry.model.slug == "keep-manual"
    }));
    assert!(!config.connection_overrides().contains_key("remove-manual"));
    assert!(
        !config
            .auto_compact_overrides()
            .contains_key("remove-manual")
    );
    assert_eq!(
        config.connection_overrides()["keep-manual"],
        pl_model::provider::ProviderConnectionMode::Http
    );
    assert_eq!(config.auto_compact_overrides()["keep-manual"], 2_000);

    let after_remove = config.clone();
    let rejected = config.remove_manual_model(&bundled_slug).unwrap_err();
    assert!(matches!(
        rejected,
        pl_protocol::PureError::ConfigError(message)
            if message == format!("unknown model: {bundled_slug}")
    ));
    assert_eq!(config, after_remove);

    let missing = config.remove_manual_model("does-not-exist").unwrap_err();
    assert!(matches!(
        missing,
        pl_protocol::PureError::ConfigError(message) if message == "unknown model: does-not-exist"
    ));
    assert_eq!(config, after_remove);
    assert_ne!(config, before);
}

#[test]
fn remove_manual_model_applies_to_explicit_catalog() {
    let keep = ModelInfo::compatible("keep");
    let remove = ModelInfo::compatible("remove");
    let mut config = ProviderConfig::from_explicit_models(
        ProviderEndpoint::openai(None),
        vec![keep.clone(), remove.clone()],
    );
    if let ProviderModelCatalogConfig::Explicit {
        connection_overrides,
        auto_compact_overrides,
        ..
    } = &mut config.catalog
    {
        connection_overrides.insert(
            "remove".into(),
            pl_model::provider::ProviderConnectionMode::Http,
        );
        auto_compact_overrides.insert("remove".into(), 4_321);
        auto_compact_overrides.insert("keep".into(), 3_000);
    }
    config.remove_manual_model("remove").unwrap();
    let catalog = config.effective_catalog().unwrap();
    assert_eq!(
        catalog
            .iter()
            .map(|entry| entry.model.slug.as_str())
            .collect::<Vec<_>>(),
        vec!["keep"]
    );
    assert!(!config.connection_overrides().contains_key("remove"));
    assert!(!config.auto_compact_overrides().contains_key("remove"));
    assert_eq!(config.auto_compact_overrides()["keep"], 3_000);
    let missing = config.remove_manual_model("remove").unwrap_err();
    assert!(matches!(
        missing,
        pl_protocol::PureError::ConfigError(message) if message == "unknown model: remove"
    ));
}

#[tokio::test]
async fn excluded_slugs_are_filtered_before_inventory_validation() {
    let server = Server::json(json!({
        "data":[
            {"id":"mimo-v2.5-pro"},
            {"id":"mimo-v2.5-asr"},
            {"id":"mimo-v2.5-tts"},
            {"id":"mimo-v2.5-tts-voiceclone"},
            {"id":"mimo-v2.5-tts-voicedesign"}
        ]
    }))
    .await;
    let config = mimo_provider(&server.url);
    let discovery = config.discovery_profile().unwrap().unwrap();
    assert_eq!(
        discovery.transport,
        ModelTransportProfile::chat_completions_http()
    );
    let models = updated(
        ModelCatalogQuery::for_provider(&config)
            .unwrap()
            .execute(None)
            .await
            .unwrap(),
    );
    assert_eq!(
        models
            .iter()
            .map(|model| model.slug.as_str())
            .collect::<Vec<_>>(),
        vec!["mimo-v2.5-pro"]
    );
    server.finish().await;
}

#[tokio::test]
async fn excluded_only_inventory_is_an_empty_success() {
    let server = Server::json(json!({
        "data":[
            {"id":"mimo-v2.5-asr"},
            {"id":"mimo-v2.5-tts"}
        ]
    }))
    .await;
    let mut config = mimo_provider(&server.url);
    let models = updated(
        ModelCatalogQuery::for_provider(&config)
            .unwrap()
            .execute(None)
            .await
            .unwrap(),
    );
    assert!(models.is_empty());
    config.set_model_catalog_overlay(models).unwrap();
    assert!(config.effective_catalog().unwrap().is_empty());
    server.finish().await;
}

#[tokio::test]
async fn chat_discovery_profile_accepts_chat_transport_and_joins_exact_prices() {
    let catalog = builtin_model_catalog(&ModelCatalogId::new("mimo").unwrap()).unwrap();
    let priced = catalog
        .models
        .iter()
        .find(|model| model.slug == "mimo-v2.5-pro")
        .unwrap();
    let server =
        Server::json(json!({"data":[{"id":"mimo-v2.5-pro"},{"id":"mimo-chat-only"}]})).await;
    let mut config = mimo_provider(&server.url);
    let models = updated(
        ModelCatalogQuery::for_provider(&config)
            .unwrap()
            .execute(None)
            .await
            .unwrap(),
    );
    assert_eq!(
        models[0].binding.transport,
        ModelTransportProfile::chat_completions_http()
    );
    config.set_model_catalog_overlay(models).unwrap();
    let catalog = config.effective_catalog().unwrap();
    let joined = catalog
        .iter()
        .find(|entry| entry.model.slug == "mimo-v2.5-pro")
        .unwrap();
    assert_eq!(joined.origin, ProviderModelOrigin::Online);
    assert_eq!(joined.model.pricing, priced.pricing);
    let unknown = catalog
        .iter()
        .find(|entry| entry.model.slug == "mimo-chat-only")
        .unwrap();
    assert_eq!(unknown.model.pricing, ModelPricing::Unknown);
    server.finish().await;
}

#[test]
fn unknown_online_price_is_not_zero_and_descriptor_keeps_origin() {
    let mut config = openai_provider("http://127.0.0.1:1");
    let mut observed = ModelInfo::compatible("online-only");
    observed
        .binding
        .set_transport(ModelTransportProfile::responses_websocket());
    observed.context_window = None;
    observed.max_output_tokens = None;
    config.set_model_catalog_overlay(vec![observed]).unwrap();
    let entry = &config.effective_catalog().unwrap()[0];
    assert_eq!(entry.origin, ProviderModelOrigin::Online);
    assert_eq!(entry.model.pricing, ModelPricing::Unknown);
    let descriptor = model_descriptor(entry);
    assert_eq!(descriptor.origin, pl_protocol::ModelOriginDto::Online);
    assert_eq!(descriptor.pricing, None);
}
