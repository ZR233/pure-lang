//! Discovery contract through public API and real localhost HTTP. No live credentials or catalog mirror.
use pl_model::{
    config::{
        AgentModelConfig, AgentRoleId, ModelCatalogId, ModelRouteAvailability, ModelRouteConfig,
        ProviderConfig, ProviderId, ReasoningEffort,
    },
    model::{
        BundledModelDefinition, ModelDefinitionError, ModelInfo, ModelModality, ModelPricing,
        ModelTransportProfile, bundled_model_definition,
    },
    provider::{
        ProviderAdapterKind, ProviderEndpoint,
        discovery::{
            ModelCatalogQuery, ModelCatalogQueryCache, ModelCatalogQueryError,
            ModelCatalogQueryResult,
        },
    },
};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{collections::HashMap, time::Duration};
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
    async fn responses(responses: Vec<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for response in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                while !bytes.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    socket.read_exact(&mut byte).await.unwrap();
                    bytes.push(byte[0]);
                    assert!(bytes.len() < 16_384);
                }
                requests.push(String::from_utf8(bytes).unwrap());
                // An oversized client can close before the fixture finishes writing.
                let _ = socket.write_all(response.as_bytes()).await;
            }
            requests
        });
        Self { url, task }
    }
    async fn json(body: Value) -> Self {
        Self::responses(vec![http(200, "", &body.to_string())]).await
    }
    async fn finish(mut self) -> Vec<String> {
        tokio::time::timeout(Duration::from_secs(5), &mut self.task)
            .await
            .unwrap()
            .unwrap()
    }
}
fn http(status: u16, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
        body.len()
    )
}
fn provider(url: &str, adapter: ProviderAdapterKind) -> ProviderConfig {
    let (endpoint, id) = match adapter {
        ProviderAdapterKind::OpenAi => (ProviderEndpoint::openai(Some(url.into())), "openai"),
        ProviderAdapterKind::DeepSeek => (ProviderEndpoint::deepseek(Some(url.into())), "deepseek"),
        _ => unreachable!(),
    };
    ProviderConfig::from_bundled_catalog(endpoint, ModelCatalogId::new(id).unwrap(), Vec::new())
}
fn updated(result: ModelCatalogQueryResult) -> Vec<ModelInfo> {
    match result {
        ModelCatalogQueryResult::Updated { models, .. } => models,
        _ => panic!("expected updated catalog"),
    }
}
fn cached_model(slug: &str, adapter: ProviderAdapterKind) -> ModelInfo {
    let mut model = ModelInfo::compatible(slug);
    model.context_window = None;
    model.max_output_tokens = None;
    model
        .binding
        .set_transport(if adapter == ProviderAdapterKind::OpenAi {
            ModelTransportProfile::responses_websocket()
        } else {
            ModelTransportProfile::responses_http()
        });
    model
}
async fn discover(body: Value, adapter: ProviderAdapterKind) -> Vec<ModelInfo> {
    let server = Server::json(body).await;
    let query = ModelCatalogQuery::for_provider(&provider(&server.url, adapter)).unwrap();
    let result = updated(query.execute(None).await.unwrap());
    server.finish().await;
    result
}

#[tokio::test]
async fn current_endpoint_prefix_query_auth_and_safe_identity_are_used() {
    let server = Server::json(json!({"data":[{"id":"unregistered-Future-ID"}]})).await;
    let mut config = provider(
        &format!("{}/tenant/proxy/?region=synthetic-secret", server.url),
        ProviderAdapterKind::OpenAi,
    );
    config.bearer_token = Some("synthetic-token".into());
    config.http_headers = Some(HashMap::from([(
        "x-account".into(),
        "synthetic-account".into(),
    )]));
    let query = ModelCatalogQuery::for_provider(&config).unwrap();
    let identity = query.identity().to_owned();
    assert_eq!(identity.len(), 64);
    assert!(!format!("{query:?}").contains("synthetic"));
    let models = updated(query.execute(None).await.unwrap());
    let headers = server.finish().await;
    assert!(
        headers[0].starts_with("GET /tenant/proxy/models?region=synthetic-secret HTTP/1.1\r\n")
    );
    let lower = headers[0].to_ascii_lowercase();
    assert!(lower.contains("authorization: bearer synthetic-token\r\n"));
    assert!(lower.contains("x-account: synthetic-account\r\n"));
    let model = &models[0];
    assert_eq!(model.slug, "unregistered-Future-ID");
    assert_eq!(model.resolved_context_window(), None);
    assert_eq!(model.max_output_tokens, None);
    assert!(model.supported_efforts().is_empty());
    assert!(!model.capabilities.temperature);
    assert!(!model.capabilities.tools.function_calling);
    assert!(
        !model
            .capabilities
            .supports_input_modality(ModelModality::Image)
    );
    assert_eq!(
        model.binding.transport,
        ModelTransportProfile::responses_websocket()
    );
    assert_eq!(model.pricing, ModelPricing::Unknown);
    for change in 0..4 {
        let mut changed = config.clone();
        match change {
            0 => changed.base_url.push_str("&other=1"),
            1 => changed.bearer_token = Some("new-token".into()),
            2 => {
                changed
                    .http_headers
                    .as_mut()
                    .unwrap()
                    .insert("x-account".into(), "new-account".into());
            }
            _ => changed.base_url = format!("{}/different/prefix", server_url_for_identity()),
        }
        assert_ne!(
            ModelCatalogQuery::for_provider(&changed)
                .unwrap()
                .identity(),
            identity
        );
    }
    config.http_headers = Some(HashMap::from([(
        "X-Account".into(),
        "synthetic-account".into(),
    )]));
    assert_eq!(
        ModelCatalogQuery::for_provider(&config).unwrap().identity(),
        identity
    );
    let explicit = ProviderConfig::from_explicit_models(
        ProviderEndpoint::openai(None),
        vec![ModelInfo::compatible("manual")],
    );
    assert!(matches!(
        ModelCatalogQuery::for_provider(&explicit),
        Err(ModelCatalogQueryError::Unsupported)
    ));
    let mut unsupported = config.clone();
    unsupported.adapter = ProviderAdapterKind::OpenAiCompatible;
    assert!(matches!(
        ModelCatalogQuery::for_provider(&unsupported),
        Err(ModelCatalogQueryError::Unsupported)
    ));
    config.http_headers = Some(HashMap::from([
        ("x-account".into(), "one".into()),
        ("X-Account".into(), "two".into()),
    ]));
    assert!(matches!(
        ModelCatalogQuery::for_provider(&config),
        Err(ModelCatalogQueryError::Configuration)
    ));
}
fn server_url_for_identity() -> &'static str {
    "http://127.0.0.1:1"
}

#[tokio::test]
async fn rich_envelopes_drive_real_inference_wire_and_unknown_pricing_preserves_usage() {
    use pl_model::{
        completion::{CompletionRequest, Message, MessageContent, MessageRole, ReasoningConfig},
        runtime::{ModelInvocationContext, ModelRuntime},
    };
    use pl_protocol::{PricingOutcome, UnpricedReason};
    use pl_provider_fixture::{FixtureServer, Protocol, Reply, Step, responses_text};
    for (adapter, body, effort_path) in [
        (
            ProviderAdapterKind::DeepSeek,
            json!({"data":[{"id":"future-model","name":"Future","context_window":64000,"max_output_tokens":8000,"input_modalities":["text","image"],"effort":{"supported_levels":["gentle","new-ultra"],"default_level":"new-ultra"},"pricing":{"input":0},"supports_function_calling":true}]}),
            "reasoning_effort",
        ),
        (
            ProviderAdapterKind::OpenAi,
            json!({"models":[{"slug":"future-model","display_name":"Future","description":"independent metadata","context_window":64000,"max_context_window":128000,"auto_compact_token_limit":12000,"supported_reasoning_levels":[{"effort":"gentle","description":"less"},{"effort":"new-ultra","description":"more"}],"default_reasoning_level":"new-ultra","input_modalities":["text","image"],"base_instructions":"DO NOT IMPORT","shell_type":"powershell","supports_temperature":false}]}),
            "reasoning",
        ),
    ] {
        let models = discover(body, adapter).await;
        let mut model = models[0].clone();
        assert_eq!(model.supported_efforts(), ["gentle", "new-ultra"]);
        assert_eq!(model.default_effort().as_deref(), Some("gentle"));
        assert_eq!(model.context_window, Some(64000));
        assert!(
            model
                .capabilities
                .supports_input_modality(ModelModality::Image)
        );
        assert!(model.base_instructions.is_empty());
        let fixture = FixtureServer::start(vec![
            Step::prompt(
                Protocol::ResponsesHttp,
                "hello",
                0,
                Reply::Sse(responses_text("answer", "discovery-wire", "future-model")),
            ),
            Step::prompt(
                Protocol::ResponsesHttp,
                "hello",
                1,
                Reply::Sse(responses_text(
                    "answer",
                    "discovery-disabled",
                    "future-model",
                )),
            ),
        ])
        .await
        .unwrap();
        let endpoint = match adapter {
            ProviderAdapterKind::OpenAi => ProviderEndpoint::openai(Some(fixture.base_url())),
            _ => ProviderEndpoint::deepseek(Some(fixture.base_url())),
        };
        model.binding.transport.default_connection_mode =
            pl_model::provider::ProviderConnectionMode::Http;
        let runtime = ModelRuntime::new(endpoint, model).unwrap();
        let request = CompletionRequest::builder()
            .messages(vec![Message {
                role: MessageRole::User,
                content: MessageContent::text("hello"),
                reasoning_content: None,
                tool_calls: None,
                tool_result: None,
                metadata: Default::default(),
                presentation: Default::default(),
            }])
            .reasoning(Some(ReasoningConfig {
                effort: Some("new-ultra".into()),
                summary: None,
            }))
            .build();
        let response = runtime
            .complete(request.clone(), ModelInvocationContext::default())
            .await
            .unwrap();
        assert_eq!(response.content.as_deref(), Some("answer"));
        assert_eq!(
            response.accounting.pricing,
            PricingOutcome::Unpriced {
                reason: UnpricedReason::MissingPrice
            }
        );
        assert!(response.accounting.usage.input_tokens.is_some());
        assert!(response.accounting.usage.output_tokens.is_some());
        let disabled = runtime
            .with_pricing_mode(pl_model::model::PricingMode::Disabled)
            .complete(request, ModelInvocationContext::default())
            .await
            .unwrap();
        assert_eq!(disabled.accounting.pricing, PricingOutcome::Disabled);
        assert_eq!(disabled.accounting.usage, response.accounting.usage);
        let records = fixture.finish().await.unwrap();
        if effort_path == "reasoning" {
            assert_eq!(records[0].body[effort_path]["effort"], "new-ultra");
        } else {
            assert_eq!(records[0].body[effort_path], "new-ultra");
            assert_eq!(records[0].body["thinking"]["type"], "enabled");
        }
        assert!(records[0].body.get("temperature").is_none());
    }
}

#[tokio::test]
async fn missing_metadata_uses_exact_success_then_default_but_explicit_empty_and_false_win() {
    let definition = bundled_model_definition("openai").unwrap();
    let known = definition
        .models
        .iter()
        .find(|m| {
            !m.supported_efforts().is_empty()
                && m.capabilities.supports_input_modality(ModelModality::Image)
        })
        .unwrap();
    let server = Server::responses(vec![
        http(200,"", &json!({"data":[{"id":known.slug}]}).to_string()),
        http(200,"", &json!({"models":[{"slug":known.slug,"supported_reasoning_levels":[],"input_modalities":[],"output_modalities":[],"supports_streaming":false,"supports_function_calling":false}]}).to_string()),
        http(200,"", &json!({"data":[{"id":"cached-only"}]}).to_string()),
    ]).await;
    let query =
        ModelCatalogQuery::for_provider(&provider(&server.url, ProviderAdapterKind::OpenAi))
            .unwrap();
    let first = updated(query.execute(None).await.unwrap());
    assert_eq!(first[0].supported_efforts(), known.supported_efforts());
    assert_eq!(first[0].context_window, known.context_window);
    let second = updated(query.execute(None).await.unwrap());
    assert!(second[0].supported_efforts().is_empty());
    assert!(second[0].capabilities.input.is_empty());
    assert!(second[0].capabilities.output.is_empty());
    assert!(second[0].binding.request.media.is_empty());
    assert!(!second[0].capabilities.streaming);
    assert!(!second[0].capabilities.tools.function_calling);
    let mut cached = first[0].clone();
    cached.slug = "cached-only".into();
    cached.context_window = Some(12345);
    let cache = ModelCatalogQueryCache {
        identity: query.identity().into(),
        etag: None,
        models: vec![cached],
    };
    let third = updated(query.execute(Some(&cache)).await.unwrap());
    assert_eq!(third[0].context_window, Some(12345));
    assert_eq!(third[0].pricing, ModelPricing::Unknown);
    server.finish().await;
}

#[tokio::test]
async fn overlay_replaces_inventory_manual_wins_and_prices_join_only_local_exact_ids() {
    let definition = bundled_model_definition("openai").unwrap();
    let known = &definition.models[0];
    let slug = known.slug.clone();
    let foreign = bundled_model_definition("deepseek").unwrap().models[0]
        .slug
        .clone();
    let api = discover(json!({"data":[{"id":slug},{"id":slug.to_uppercase()},{"id":format!("{slug}-variant")},{"id":foreign},{"id":"manual"}]}), ProviderAdapterKind::OpenAi).await;
    let mut config = provider("http://127.0.0.1:1", ProviderAdapterKind::OpenAi);
    let mut manual = ModelInfo::compatible("manual");
    manual.display_name = "Authoritative manual model".into();
    manual.pricing = known.pricing.clone();
    if let pl_model::config::ProviderModelCatalogConfig::Bundled {
        additional_models, ..
    } = &mut config.catalog
    {
        additional_models.push(manual.clone());
    }
    config.set_model_catalog_overlay(api).unwrap();
    let effective = config.effective_models().unwrap();
    assert_eq!(
        effective.iter().find(|m| m.slug == slug).unwrap().pricing,
        known.pricing
    );
    for model in effective
        .iter()
        .filter(|m| m.slug != slug && m.slug != "manual")
    {
        assert_eq!(model.pricing, ModelPricing::Unknown);
        assert_eq!(model.context_window, None);
    }
    assert_eq!(
        effective.iter().find(|m| m.slug == "manual").unwrap(),
        &manual
    );
    let persisted = serde_json::to_value(&config).unwrap();
    let reread: ProviderConfig = serde_json::from_value(persisted).unwrap();
    assert!(!reread.has_model_catalog_overlay());
    config.set_model_catalog_overlay(Vec::new()).unwrap();
    assert_eq!(config.effective_models().unwrap(), vec![manual]);
    if let pl_model::config::ProviderModelCatalogConfig::Bundled {
        additional_models, ..
    } = &mut config.catalog
    {
        additional_models.push(known.clone());
    }
    assert!(config.declared_models().is_err());
    let empty = discover(json!({"data":[]}), ProviderAdapterKind::DeepSeek).await;
    assert!(empty.is_empty());
    let mut fresh = provider("http://127.0.0.1:1", ProviderAdapterKind::DeepSeek);
    fresh.set_model_catalog_overlay(empty).unwrap();
    assert!(fresh.effective_models().unwrap().is_empty());
}

#[test]
fn availability_preserves_removed_choice_without_weakening_strict_edits_or_structure_validation() {
    let mut provider = provider("http://127.0.0.1:1", ProviderAdapterKind::OpenAi);
    let mut selected = provider.effective_models().unwrap()[0].clone();
    selected.slug = "remote-only-choice".into();
    provider
        .set_model_catalog_overlay(vec![selected.clone()])
        .unwrap();
    provider
        .set_model_connection_mode(
            &selected.slug,
            pl_model::provider::ProviderConnectionMode::Http,
        )
        .unwrap();
    provider
        .set_model_auto_compact_override(&selected.slug, Some(1234))
        .unwrap();
    provider.set_model_catalog_overlay(Vec::new()).unwrap();
    let id = ProviderId::new("instance-a").unwrap();
    let role = AgentRoleId::new("explorer").unwrap();
    let route = ModelRouteConfig {
        provider: id.clone(),
        model: selected.slug.clone(),
        effort: selected.default_effort().map(ReasoningEffort::new),
    };
    let mut config = AgentModelConfig {
        providers: [(id.clone(), provider)].into(),
        routes: [(role.clone(), route.clone())].into(),
    };
    config.validate_declarations().unwrap();
    assert_eq!(
        config.route_availability(&route).unwrap(),
        ModelRouteAvailability::ModelUnavailable {
            provider: id.clone(),
            model: selected.slug.clone()
        }
    );
    assert!(config.validate().is_err());
    assert!(config.resolve(&role).is_err());
    assert_eq!(config.routes[&role], route);
    let from_disk: AgentModelConfig =
        serde_json::from_value(serde_json::to_value(&config).unwrap()).unwrap();
    from_disk.validate_declarations().unwrap();
    assert!(matches!(
        from_disk.route_availability(&route).unwrap(),
        ModelRouteAvailability::ModelUnavailable { .. }
    ));
    assert!(from_disk.validate().is_err());
    let mut current = selected.clone();
    current.parameters.clear();
    current.binding.request.body.clear();
    config
        .providers
        .get_mut(&id)
        .unwrap()
        .set_model_catalog_overlay(vec![current])
        .unwrap();
    assert!(matches!(
        config.route_availability(&route).unwrap(),
        ModelRouteAvailability::EffortUnavailable { .. }
    ));
    let mut manual = ModelInfo::compatible("manual-choice");
    manual.parameters.clear();
    if let pl_model::config::ProviderModelCatalogConfig::Bundled {
        additional_models, ..
    } = &mut config.providers.get_mut(&id).unwrap().catalog
    {
        additional_models.push(manual);
    }
    let manual_route = ModelRouteConfig {
        provider: id.clone(),
        model: "manual-choice".into(),
        effort: Some(ReasoningEffort::new("undeclared")),
    };
    assert!(matches!(
        config.route_availability(&manual_route),
        Err(pl_protocol::PureError::ConfigError(_))
    ));
    config.routes.insert(role, manual_route);
    assert!(matches!(
        config.validate_declarations(),
        Err(pl_protocol::PureError::ConfigError(_))
    ));
    config.providers.get_mut(&id).unwrap().name.clear();
    assert!(config.route_availability(&route).is_err());
}

#[tokio::test]
async fn invalid_declaration_rejects_whole_response_without_leaking_response_or_mutating_cache() {
    let invalid_profile = json!({"transport":{"protocol":"chat_completions","supported_connection_modes":["web_socket"],"default_connection_mode":"web_socket"},"request":{"protocol":{"api":"chatCompletions","parallelToolCalls":false,"maxTokensField":"max_tokens","includeUsage":false,"toolStream":false}}});
    for body in [
        json!({"data":[{"id":"ok"},{"id":"ok"}]}),
        json!({"data":[{"id":" "}]}),
        json!({"data":[{"id":"ok","context_window":0}]}),
        json!({"data":[{"id":"ok","max_output_tokens":-1}]}),
        json!({"data":[{"id":"ok","effort":{"supported_levels":["one"],"default_level":"absent"}}]}),
        json!({"data":[{"id":"ok","effort":{"supported_levels":["one"],"default_level":"one"},"default_reasoning_level":"absent"}]}),
        json!({"models":[{"slug":"ok","supported_reasoning_levels":[{"effort":"one"},{"effort":"one"}]}]}),
        json!({"data":[{"id":"ok","binding":invalid_profile}]}),
        json!({"data":[{"id":"ok","input_modalities":["video"]}]}),
        json!({"data":[{"id":"ok","parameters":[{"name":"effort","candidates":["a"],"wire":{}}]}]}),
        json!({"error":{"message":"synthetic-secret-response"},"data":[]}),
        json!({"success":false,"data":[]}),
        json!({"models":[],"data":[]}),
        json!({"not-models":[]}),
    ] {
        let server = Server::json(body).await;
        let query =
            ModelCatalogQuery::for_provider(&provider(&server.url, ProviderAdapterKind::OpenAi))
                .unwrap();
        let cache = ModelCatalogQueryCache {
            identity: query.identity().into(),
            etag: None,
            models: vec![cached_model("last-good", ProviderAdapterKind::OpenAi)],
        };
        let error = query.execute(Some(&cache)).await.unwrap_err();
        assert_eq!(error, ModelCatalogQueryError::Protocol);
        assert!(!format!("{error:?}: {error}").contains("synthetic-secret-response"));
        assert_eq!(cache.models[0].slug, "last-good");
        server.finish().await;
    }
}

#[tokio::test]
async fn conditional_304_requires_same_identity_success_and_returns_etag() {
    let server = Server::responses(vec![
        http(200, "ETag: \"version-a\"\r\n", "{\"data\":[]}"),
        http(304, "", ""),
        http(304, "", ""),
    ])
    .await;
    let query =
        ModelCatalogQuery::for_provider(&provider(&server.url, ProviderAdapterKind::OpenAi))
            .unwrap();
    let ModelCatalogQueryResult::Updated {
        identity,
        etag,
        models,
    } = query.execute(None).await.unwrap()
    else {
        panic!()
    };
    assert_eq!(etag.as_deref(), Some("\"version-a\""));
    let cache = ModelCatalogQueryCache {
        identity: identity.clone(),
        etag: etag.clone(),
        models,
    };
    assert_eq!(
        query.execute(Some(&cache)).await.unwrap(),
        ModelCatalogQueryResult::NotModified { identity, etag }
    );
    assert_eq!(
        query.execute(None).await.unwrap_err(),
        ModelCatalogQueryError::UnexpectedNotModified
    );
    let mut corrupt = cache.clone();
    let mut priced = cached_model("cached-price", ProviderAdapterKind::OpenAi);
    priced.pricing = ModelPricing::published(
        "USD",
        vec![pl_model::model::TokenPriceTier::flat(1.0, 2.0, None, None)],
        "https://synthetic.invalid/pricing",
    );
    corrupt.models.push(priced);
    assert_eq!(
        query.execute(Some(&corrupt)).await.unwrap_err(),
        ModelCatalogQueryError::Protocol
    );
    let mut changed = cache;
    changed.identity = "different-query".into();
    assert_eq!(
        query.execute(Some(&changed)).await.unwrap_err(),
        ModelCatalogQueryError::CacheIdentity
    );
    let requests = server.finish().await;
    assert!(
        requests[1]
            .to_ascii_lowercase()
            .contains("if-none-match: \"version-a\"\r\n")
    );
    assert!(!requests[2].to_ascii_lowercase().contains("if-none-match:"));
}

#[tokio::test]
async fn discovery_profile_validity_is_independent_of_connection_mode_order() {
    let mut model = cached_model("profile-order", ProviderAdapterKind::OpenAi);
    model.binding.transport.supported_connection_modes.reverse();
    let models = discover(
        json!({"data":[{"id":model.slug,"binding":model.binding}]}),
        ProviderAdapterKind::OpenAi,
    )
    .await;
    assert_eq!(
        models[0].binding.transport,
        ModelTransportProfile::responses_websocket()
    );
    let mut config = provider("http://127.0.0.1:1", ProviderAdapterKind::OpenAi);
    config.set_model_catalog_overlay(vec![model]).unwrap();
    config
        .set_model_connection_mode(
            "profile-order",
            pl_model::provider::ProviderConnectionMode::Http,
        )
        .unwrap();
    assert_eq!(
        config.effective_models().unwrap()[0]
            .binding
            .transport
            .default_connection_mode,
        pl_model::provider::ProviderConnectionMode::Http
    );
}

#[tokio::test]
async fn failed_http_redirect_and_body_caps_are_typed_and_never_expose_arbitrary_text() {
    for (response, expected) in [
        (
            http(401, "", "synthetic-token arbitrary body"),
            ModelCatalogQueryError::Http { status: 401 },
        ),
        (
            http(302, "Location: http://127.0.0.1:1/secret\r\n", ""),
            ModelCatalogQueryError::Http { status: 302 },
        ),
        (
            http(503, "Retry-After: 30\r\n", "provider internal secret"),
            ModelCatalogQueryError::Http { status: 503 },
        ),
        (
            http(200, "", "not json synthetic-secret"),
            ModelCatalogQueryError::Protocol,
        ),
        (
            http(200, "", &"x".repeat(4 * 1024 * 1024 + 1)),
            ModelCatalogQueryError::TooLarge,
        ),
        (
            format!(
                "HTTP/1.1 200 Test\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",
                4 * 1024 * 1024 + 1,
                "x".repeat(4 * 1024 * 1024 + 1)
            ),
            ModelCatalogQueryError::TooLarge,
        ),
    ] {
        let server = Server::responses(vec![response]).await;
        let query =
            ModelCatalogQuery::for_provider(&provider(&server.url, ProviderAdapterKind::OpenAi))
                .unwrap();
        let error = query.execute(None).await.unwrap_err();
        assert_eq!(error, expected);
        assert!(!format!("{error:?}: {error}").contains("secret"));
        assert_eq!(server.finish().await.len(), 1);
    }
}

#[tokio::test]
async fn whole_body_timeout_and_future_drop_release_borrow_without_changing_last_good() {
    for cancel in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                let mut b = [0];
                socket.read_exact(&mut b).await.unwrap();
                headers.push(b[0]);
            }
            socket
                .write_all(b"HTTP/1.1 200 Test\r\nContent-Length: 1000\r\n\r\n{")
                .await
                .unwrap();
            ready_tx.send(()).unwrap();
            release_rx.await.unwrap();
            // Finish the HTTP frame for library-level connection cleanup. Dropping the
            // application future is not a guarantee of immediate TCP FIN from reqwest/Hyper.
            let suffix = "\"data\":[{\"id\":\"late-result\"}]}";
            let remainder = format!("{}{}", " ".repeat(999 - suffix.len()), suffix);
            let _ = socket.write_all(remainder.as_bytes()).await;
            let mut b = [0];
            socket.read(&mut b).await.unwrap_or(0)
        });
        let query = ModelCatalogQuery::for_provider(&provider(&url, ProviderAdapterKind::DeepSeek))
            .unwrap();
        let cache = ModelCatalogQueryCache {
            identity: query.identity().into(),
            etag: None,
            models: vec![cached_model("last-good", ProviderAdapterKind::DeepSeek)],
        };
        let mut future = Box::pin(query.execute(Some(&cache)));
        tokio::select! { result = &mut future => panic!("completed before body: {result:?}"), result = ready_rx => result.unwrap() }
        if cancel {
            drop(future);
        } else {
            assert_eq!(future.await.unwrap_err(), ModelCatalogQueryError::Timeout);
        }
        // Moving the cache proves the query no longer retains the borrowed snapshot.
        let retained_cache = cache;
        release_tx.send(()).unwrap();
        let closed = tokio::time::timeout(Duration::from_secs(3), task).await;
        assert!(
            closed.is_ok(),
            "fixture connection was not reclaimed after cancel={cancel}"
        );
        assert_eq!(closed.unwrap().unwrap(), 0);
        assert_eq!(retained_cache.models[0].slug, "last-good");
    }
}

#[test]
fn malformed_bundled_definition_is_packaging_failure_not_user_config_reset() {
    let json = json!({"schemaVersion":9,"catalog":"synthetic","suggestedModel":"one","suggestedEffort":null,"models":[]}).to_string();
    assert_eq!(
        BundledModelDefinition::parse(&json, "synthetic").unwrap_err(),
        ModelDefinitionError::Identity
    );
    let error: pl_protocol::PureError = ModelDefinitionError::Declaration.into();
    assert!(matches!(error, pl_protocol::PureError::Provider(_)));
}
