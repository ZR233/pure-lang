use futures::FutureExt;
use futures::future::BoxFuture;
use futures::future::join_all;
use pl_model::{
    DeepSeekBalanceUsage, ZhipuCodingPlanUsage, query_deepseek_balance,
    query_zhipu_coding_plan_usage,
};

use crate::ProviderConfig;
use crate::config::StudioConfig;
use crate::config_editor::provider_template_kind;

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderUsageRecord {
    pub provider_id: String,
    pub updated_at: i64,
    pub state: ProviderUsageState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderUsageState {
    Unsupported,
    MissingCredential,
    Ready(ProviderUsageData),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderUsageData {
    DeepSeekBalance(DeepSeekBalanceUsage),
    ZhipuCodingPlan(ZhipuCodingPlanUsage),
}

pub async fn provider_usage_records(config: &StudioConfig) -> Vec<ProviderUsageRecord> {
    let futures = config
        .models
        .providers
        .iter()
        .map(|(provider_id, provider)| {
            let selected_model = config
                .models
                .routes
                .values()
                .find(|route| route.provider == *provider_id)
                .map(|route| route.model.clone())
                .or_else(|| {
                    provider
                        .effective_models()
                        .ok()
                        .and_then(|models| models.first().map(|model| model.slug.clone()))
                })
                .unwrap_or_default();
            provider_usage_record(provider_id.to_string(), provider.clone(), selected_model)
        });
    join_all(futures).await
}

async fn provider_usage_record(
    provider_id: String,
    provider: ProviderConfig,
    selected_model: String,
) -> ProviderUsageRecord {
    let updated_at = unix_seconds();
    let state = match provider_template_kind(&provider)
        .as_ref()
        .map(|kind| kind.key())
    {
        Some("deepseek") => provider_usage_data(provider, &selected_model, query_deepseek).await,
        Some("zhipu-coding-plan") => {
            provider_usage_data(provider, &selected_model, query_zhipu).await
        }
        Some(_) | None => ProviderUsageState::Unsupported,
    };
    ProviderUsageRecord {
        provider_id,
        updated_at,
        state,
    }
}

type ProviderUsageQueryFuture = BoxFuture<'static, crate::Result<ProviderUsageData>>;

async fn provider_usage_data(
    provider: ProviderConfig,
    selected_model: &str,
    query: impl FnOnce(pl_model::ProviderInfo) -> ProviderUsageQueryFuture,
) -> ProviderUsageState {
    if provider
        .resolved_bearer_token()
        .as_ref()
        .is_none_or(|token| token.trim().is_empty())
    {
        return ProviderUsageState::MissingCredential;
    }
    let info = match provider.to_provider_info(selected_model) {
        Ok(info) => info,
        Err(error) => return ProviderUsageState::Failed(error.to_string()),
    };
    match query(info).await {
        Ok(data) => ProviderUsageState::Ready(data),
        Err(error) => ProviderUsageState::Failed(error.to_string()),
    }
}

fn query_deepseek(info: pl_model::ProviderInfo) -> ProviderUsageQueryFuture {
    async move {
        query_deepseek_balance(&info)
            .await
            .map(ProviderUsageData::DeepSeekBalance)
    }
    .boxed()
}

fn query_zhipu(info: pl_model::ProviderInfo) -> ProviderUsageQueryFuture {
    async move {
        query_zhipu_coding_plan_usage(&info)
            .await
            .map(ProviderUsageData::ZhipuCodingPlan)
    }
    .boxed()
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
