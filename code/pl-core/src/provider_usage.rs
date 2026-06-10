use futures::future::join_all;
use pl_model::{
    DeepSeekBalanceUsage, ZhipuCodingPlanUsage, query_deepseek_balance,
    query_zhipu_coding_plan_usage,
};

use crate::config::{ProviderConfig, PureConfig};
use crate::config_editor::infer_provider_template_kind;
use crate::first_run::ProviderTemplateKind;

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

pub async fn provider_usage_records(config: &PureConfig) -> Vec<ProviderUsageRecord> {
    let futures = config.providers.iter().map(|(provider_id, provider)| {
        provider_usage_record(provider_id.clone(), provider.clone())
    });
    join_all(futures).await
}

async fn provider_usage_record(
    provider_id: String,
    provider: ProviderConfig,
) -> ProviderUsageRecord {
    let updated_at = unix_seconds();
    let template_kind = infer_provider_template_kind(&provider_id, &provider);
    let state = match template_kind {
        ProviderTemplateKind::DeepSeek => provider_usage_data(provider, query_deepseek).await,
        ProviderTemplateKind::ZhipuCodingPlan => provider_usage_data(provider, query_zhipu).await,
        ProviderTemplateKind::OpenAi | ProviderTemplateKind::Zhipu => {
            ProviderUsageState::Unsupported
        }
    };
    ProviderUsageRecord {
        provider_id,
        updated_at,
        state,
    }
}

async fn provider_usage_data(
    provider: ProviderConfig,
    query: impl FnOnce(
        pl_model::ProviderInfo,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = pl_protocol::Result<ProviderUsageData>> + Send>,
    >,
) -> ProviderUsageState {
    if provider
        .bearer_token
        .as_ref()
        .is_none_or(|token| token.trim().is_empty())
    {
        return ProviderUsageState::MissingCredential;
    }
    match query(provider.to_provider_info(&provider.default_model)).await {
        Ok(data) => ProviderUsageState::Ready(data),
        Err(error) => ProviderUsageState::Failed(error.to_string()),
    }
}

fn query_deepseek(
    info: pl_model::ProviderInfo,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = pl_protocol::Result<ProviderUsageData>> + Send>,
> {
    Box::pin(async move {
        query_deepseek_balance(&info)
            .await
            .map(ProviderUsageData::DeepSeekBalance)
    })
}

fn query_zhipu(
    info: pl_model::ProviderInfo,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = pl_protocol::Result<ProviderUsageData>> + Send>,
> {
    Box::pin(async move {
        query_zhipu_coding_plan_usage(&info)
            .await
            .map(ProviderUsageData::ZhipuCodingPlan)
    })
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[tokio::test]
    async fn unsupported_providers_are_marked_without_network_query() {
        let mut config = PureConfig::default_config();
        let mut zhipu = pl_model::ProviderInfo::zhipu(None);
        zhipu.bearer_token = Some("secret".to_string());
        config.providers.insert(
            "zhipu".to_string(),
            ProviderConfig::from_provider_info(
                zhipu,
                ProviderTemplateKind::Zhipu.default_models().unwrap(),
            ),
        );

        let records = provider_usage_records(&config).await;
        let zhipu_record = records
            .iter()
            .find(|record| record.provider_id == "zhipu")
            .expect("zhipu record");

        assert_eq!(zhipu_record.state, ProviderUsageState::Unsupported);
    }

    #[tokio::test]
    async fn supported_provider_without_token_is_missing_credential() {
        let config = PureConfig::default_config();

        let records = provider_usage_records(&config).await;
        let deepseek_record = records
            .iter()
            .find(|record| record.provider_id == "deepseek")
            .expect("deepseek record");

        assert_eq!(deepseek_record.state, ProviderUsageState::MissingCredential);
    }
}
