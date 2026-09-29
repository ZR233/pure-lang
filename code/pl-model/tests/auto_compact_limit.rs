//! 上下文压缩阈值：模型默认、provider 实例覆盖、实际生效与配置 serde 兼容。

use std::collections::BTreeMap;

use pl_model::config::{
    AgentModelConfig, AgentRoleId, ModelCatalogId, ModelRouteConfig, ProviderConfig, ProviderId,
};
use pl_model::model::{DEFAULT_AUTO_COMPACT_TOKEN_LIMIT, ModelInfo};
use pl_model::provider::ProviderEndpoint;
use serde_json::json;

fn model(slug: &str, context_window: Option<u64>) -> ModelInfo {
    let mut model = ModelInfo::compatible(slug);
    model.context_window = context_window;
    model.max_context_window = context_window;
    model
}

fn explicit_provider(models: Vec<ModelInfo>) -> ProviderConfig {
    ProviderConfig::from_explicit_models(
        ProviderEndpoint::compatible("Test", "https://example.test/v1"),
        models,
    )
}

fn config_with(provider_id: &str, provider: ProviderConfig) -> AgentModelConfig {
    let mut providers = BTreeMap::new();
    providers.insert(ProviderId::new(provider_id).unwrap(), provider);
    AgentModelConfig {
        providers,
        routes: BTreeMap::new(),
    }
}

#[test]
fn undeclared_default_is_258k_when_context_is_large() {
    let model = model("m", Some(400_000));
    assert_eq!(model.default_auto_compact_token_limit(), 258_000);
    assert_eq!(DEFAULT_AUTO_COMPACT_TOKEN_LIMIT, 258_000);
    assert_eq!(model.safe_auto_compact_token_limit(), Some(360_000));
    assert_eq!(model.resolved_auto_compact_limit(), Some(258_000));
}

#[test]
fn declared_default_is_preserved() {
    let mut model = model("m", Some(400_000));
    model.auto_compact_token_limit = Some(150_000);
    assert_eq!(model.default_auto_compact_token_limit(), 150_000);
    assert_eq!(model.resolved_auto_compact_limit(), Some(150_000));
}

#[test]
fn effective_is_capped_by_context_90_percent() {
    let model = model("m", Some(200_000));
    assert_eq!(model.safe_auto_compact_token_limit(), Some(180_000));
    assert_eq!(model.default_auto_compact_token_limit(), 258_000);
    assert_eq!(model.resolved_auto_compact_limit(), Some(180_000));
}

#[test]
fn unknown_context_has_no_auto_compaction() {
    let model = model("m", None);
    assert_eq!(model.safe_auto_compact_token_limit(), None);
    assert_eq!(model.resolved_auto_compact_limit(), None);
    assert_eq!(model.resolved_auto_compact_limit_with(Some(1000)), None);
}

#[test]
fn override_applies_without_mutating_default_metadata() {
    let mut provider = explicit_provider(vec![model("m", Some(400_000))]);
    provider
        .set_model_auto_compact_override("m", Some(300_000))
        .unwrap();
    let model = provider.declared_models().unwrap().remove(0);
    // 默认元信息保持不变，覆盖独立保存。
    assert_eq!(model.default_auto_compact_token_limit(), 258_000);
    assert_eq!(provider.auto_compact_overrides().get("m"), Some(&300_000));
    assert_eq!(provider.resolved_auto_compact_limit(&model), Some(300_000));
}

#[test]
fn override_cannot_bypass_safety_cap() {
    let mut provider = explicit_provider(vec![model("m", Some(200_000))]);
    provider
        .set_model_auto_compact_override("m", Some(500_000))
        .unwrap();
    let model = provider.declared_models().unwrap().remove(0);
    assert_eq!(provider.resolved_auto_compact_limit(&model), Some(180_000));
}

#[test]
fn restore_default_removes_override() {
    let mut provider = explicit_provider(vec![model("m", Some(400_000))]);
    provider
        .set_model_auto_compact_override("m", Some(300_000))
        .unwrap();
    assert_eq!(provider.auto_compact_overrides().len(), 1);
    provider.set_model_auto_compact_override("m", None).unwrap();
    assert!(provider.auto_compact_overrides().is_empty());
    let model = provider.declared_models().unwrap().remove(0);
    assert_eq!(provider.resolved_auto_compact_limit(&model), Some(258_000));
}

#[test]
fn override_equal_to_default_is_not_stored() {
    let mut provider = explicit_provider(vec![model("m", Some(400_000))]);
    provider
        .set_model_auto_compact_override("m", Some(258_000))
        .unwrap();
    assert!(provider.auto_compact_overrides().is_empty());
}

#[test]
fn overrides_are_isolated_between_models() {
    let mut provider =
        explicit_provider(vec![model("a", Some(400_000)), model("b", Some(400_000))]);
    provider
        .set_model_auto_compact_override("a", Some(300_000))
        .unwrap();
    let models = provider.declared_models().unwrap();
    let a = models.iter().find(|m| m.slug == "a").unwrap();
    let b = models.iter().find(|m| m.slug == "b").unwrap();
    assert_eq!(provider.resolved_auto_compact_limit(a), Some(300_000));
    assert_eq!(provider.resolved_auto_compact_limit(b), Some(258_000));
}

#[test]
fn overrides_are_isolated_between_providers() {
    let mut first = explicit_provider(vec![model("m", Some(400_000))]);
    first
        .set_model_auto_compact_override("m", Some(300_000))
        .unwrap();
    let second = explicit_provider(vec![model("m", Some(400_000))]);
    assert!(second.auto_compact_overrides().is_empty());
    let second_model = second.declared_models().unwrap().remove(0);
    assert_eq!(
        second.resolved_auto_compact_limit(&second_model),
        Some(258_000)
    );
}

#[test]
fn zero_override_is_rejected() {
    let mut provider = explicit_provider(vec![model("m", Some(400_000))]);
    assert!(
        provider
            .set_model_auto_compact_override("m", Some(0))
            .is_err()
    );
    assert!(provider.auto_compact_overrides().is_empty());
}

#[test]
fn unknown_slug_is_rejected() {
    let mut provider = explicit_provider(vec![model("m", Some(400_000))]);
    assert!(
        provider
            .set_model_auto_compact_override("missing", Some(1000))
            .is_err()
    );
}

#[test]
fn bundled_models_use_default_limit() {
    let provider = ProviderConfig::from_bundled_catalog(
        ProviderEndpoint::deepseek(None),
        ModelCatalogId::new("deepseek").unwrap(),
        Vec::new(),
    );
    let model = provider.declared_models().unwrap().remove(0);
    assert_eq!(model.default_auto_compact_token_limit(), 258_000);
    assert_eq!(provider.resolved_auto_compact_limit(&model), Some(258_000));
}

#[test]
fn resolved_route_carries_effective_limit() {
    let mut provider = explicit_provider(vec![model("m", Some(400_000))]);
    provider
        .set_model_auto_compact_override("m", Some(300_000))
        .unwrap();
    let config = config_with("p", provider);
    config.validate().unwrap();
    let selector = ModelRouteConfig {
        provider: ProviderId::new("p").unwrap(),
        model: "m".to_string(),
        effort: None,
    };
    let route = config
        .resolve_route(AgentRoleId::new("executor").unwrap(), &selector)
        .unwrap();
    assert_eq!(route.auto_compact_limit, Some(300_000));
}

#[test]
fn config_serde_round_trips_overrides_and_omits_empty() {
    // 无覆盖：序列化不写入可选集合。
    let provider = explicit_provider(vec![model("m", Some(400_000))]);
    let value = serde_json::to_value(&provider).unwrap();
    assert!(value["catalog"].get("auto_compact_overrides").is_none());

    // 有覆盖：往返保留。
    let mut provider = explicit_provider(vec![model("m", Some(400_000))]);
    provider
        .set_model_auto_compact_override("m", Some(300_000))
        .unwrap();
    let value = serde_json::to_value(&provider).unwrap();
    assert_eq!(
        value["catalog"]["auto_compact_overrides"]["m"],
        json!(300_000)
    );
    let parsed: ProviderConfig = serde_json::from_value(value).unwrap();
    assert_eq!(parsed.auto_compact_overrides().get("m"), Some(&300_000));
}

#[test]
fn config_deserializes_without_override_field() {
    let provider = explicit_provider(vec![model("m", Some(400_000))]);
    let value = serde_json::to_value(&provider).unwrap();
    // 模拟旧配置：完全没有该集合。
    let mut catalog = value["catalog"].clone();
    catalog
        .as_object_mut()
        .unwrap()
        .remove("auto_compact_overrides");
    let mut legacy = value.clone();
    legacy["catalog"] = catalog;
    let parsed: ProviderConfig = serde_json::from_value(legacy).unwrap();
    assert!(parsed.auto_compact_overrides().is_empty());
    parsed
        .declared_models()
        .expect("legacy config still resolves models");
}

#[test]
fn invalid_stored_overrides_are_rejected_by_validation() {
    let provider = explicit_provider(vec![model("m", Some(400_000))]);

    let mut unknown = serde_json::to_value(&provider).unwrap();
    unknown["catalog"]["auto_compact_overrides"] = json!({ "missing": 1000 });
    let parsed: ProviderConfig = serde_json::from_value(unknown).unwrap();
    assert!(config_with("p", parsed).validate().is_err());

    let mut zero = serde_json::to_value(&provider).unwrap();
    zero["catalog"]["auto_compact_overrides"] = json!({ "m": 0 });
    let parsed: ProviderConfig = serde_json::from_value(zero).unwrap();
    assert!(config_with("p", parsed).validate().is_err());
}

#[test]
fn non_positive_default_limit_is_rejected() {
    // 模型级校验直接拒绝默认阈值 0。
    let mut zero = model("m", Some(400_000));
    zero.auto_compact_token_limit = Some(0);
    assert!(zero.validate().is_err());

    // 显式目录中的存量模型默认 0 在 provider 校验阶段失败。
    let mut explicit_zero = model("m", Some(400_000));
    explicit_zero.auto_compact_token_limit = Some(0);
    let provider = explicit_provider(vec![explicit_zero]);
    assert!(config_with("p", provider).validate().is_err());

    // 内置目录的附加模型默认 0 同样失败。
    let mut additional_zero = model("custom-zero", Some(400_000));
    additional_zero.auto_compact_token_limit = Some(0);
    let provider = ProviderConfig::from_bundled_catalog(
        ProviderEndpoint::deepseek(None),
        ModelCatalogId::new("deepseek").unwrap(),
        vec![additional_zero],
    );
    assert!(config_with("p", provider).validate().is_err());
}

#[test]
fn u64_max_context_uses_exact_90_percent() {
    let model = model("m", Some(u64::MAX));
    // floor(u64::MAX * 90 / 100)，无溢出且非饱和截断。
    assert_eq!(
        model.safe_auto_compact_token_limit(),
        Some(16_602_069_666_338_596_453)
    );
    assert_eq!(model.resolved_auto_compact_limit(), Some(258_000));
    // 超大覆盖被安全上限钳制，不会溢出或饱和。
    assert_eq!(
        model.resolved_auto_compact_limit_with(Some(u64::MAX)),
        Some(16_602_069_666_338_596_453)
    );
}
