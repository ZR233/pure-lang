use pl_studio_runtime::StudioModelPerformanceSnapshot;

use crate::api::studio::types::{
    BridgeModelMatchState, BridgeModelPerformanceSample, BridgeModelPerformanceSnapshot,
    BridgeModelPerformanceSummary, BridgePurposeCostSnapshot, BridgeRuntimeCostAmount,
    BridgeSessionCostSnapshot,
};

pub(crate) fn bridge_model_performance(
    value: StudioModelPerformanceSnapshot,
) -> BridgeModelPerformanceSnapshot {
    BridgeModelPerformanceSnapshot {
        revision: value.revision,
        updated_at: value.updated_at,
        statistics_pending: value.statistics_pending,
        statistics_gap: value.statistics_gap,
        read_failed: value.read_failed,
        session_costs: value
            .session_costs
            .into_iter()
            .map(|session| BridgeSessionCostSnapshot {
                root_thread_id: session.root_thread_id,
                purpose_costs: session
                    .purpose_costs
                    .into_iter()
                    .map(|cost| BridgePurposeCostSnapshot {
                        purpose: cost.purpose,
                        estimated_costs: cost
                            .estimated_costs
                            .into_iter()
                            .map(|amount| BridgeRuntimeCostAmount {
                                currency: amount.currency,
                                amount: amount.amount,
                            })
                            .collect(),
                        has_unpriced_usage: cost.has_unpriced_usage,
                    })
                    .collect(),
                estimated_costs: session
                    .estimated_costs
                    .into_iter()
                    .map(|cost| BridgeRuntimeCostAmount {
                        currency: cost.currency,
                        amount: cost.amount,
                    })
                    .collect(),
                has_unpriced_usage: session.has_unpriced_usage,
            })
            .collect(),
        summaries: value
            .summaries
            .into_iter()
            .map(|summary| BridgeModelPerformanceSummary {
                provider_instance_id: summary.provider_instance_id,
                provider_display_name: summary.provider_display_name,
                model: summary.model,
                reasoning_effort: summary.reasoning_effort,
                sample_count: summary.sample_count,
                completion_tokens: summary.completion_tokens,
                total_ttft_millis: summary.total_ttft_millis,
                total_decode_millis: summary.total_decode_millis,
                total_response_millis: summary.total_response_millis,
                tokens_per_second: summary.tokens_per_second,
                average_ttft_millis: summary.average_ttft_millis,
                average_response_millis: summary.average_response_millis,
            })
            .collect(),
        history: value
            .history
            .into_iter()
            .map(|sample| BridgeModelPerformanceSample {
                completed_at: sample.completed_at,
                provider_instance_id: sample.provider_instance_id,
                provider_display_name: sample.provider_display_name,
                model: sample.model,
                configured_model: sample.configured_model,
                sent_model: sample.sent_model,
                reported_model: sample.reported_model,
                model_match_state: match sample.model_match_state {
                    pl_protocol::ModelMatchState::Matched => BridgeModelMatchState::Matched,
                    pl_protocol::ModelMatchState::Mismatched => BridgeModelMatchState::Mismatched,
                    pl_protocol::ModelMatchState::Unreported => BridgeModelMatchState::Unreported,
                    pl_protocol::ModelMatchState::LegacyUnknown => {
                        BridgeModelMatchState::LegacyUnknown
                    }
                },
                reasoning_effort: sample.reasoning_effort,
                completion_tokens: sample.completion_tokens,
                ttft_millis: sample.ttft_millis,
                decode_millis: sample.decode_millis,
                total_response_millis: sample.total_response_millis,
                tokens_per_second: sample.tokens_per_second,
            })
            .collect(),
    }
}
