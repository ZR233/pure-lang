//! 从已提交的模型 attempt 事实构造稳定的计费记录。
//!
//! `pl-core` 只负责保存 opaque attempt 事实；模型 receipt 的解释属于本 crate。把这段解码
//! 放在模型边界，Studio、其它宿主和历史查询都能复用同一套语义，而无需复制 provider
//! receipt 解析或读取存储表。

use pl_core::model::ModelUsage;
use pl_core::thread::{AttemptOutcome, journal::AttemptUpdate};
use pl_protocol::{InferenceAccounting, InferenceBillingRecord};

use super::{model_failure_receipt, model_request_receipt, model_response_receipt};

/// 从一个已终态的模型 attempt 事实读取完整 billing 记录。
///
/// 请求 receipt 提供绑定、上下文窗口和 prompt cache 诊断；响应或失败 receipt 提供真实
/// provider 用量、模型观测、时延和编排数据。运行中或没有模型 receipt 的 attempt 返回
/// `None`，不从当前配置猜测历史信息。
///
/// # Errors
/// 返回 receipt 格式、版本或内容损坏错误。调用方应把这类错误作为历史数据损坏处理，不能
/// 用零用量替代。
pub fn model_attempt_billing(
    attempt: &AttemptUpdate,
    recorded_at: i64,
) -> Result<Option<InferenceBillingRecord>, crate::runtime::ModelError> {
    let output = match &attempt.outcome {
        AttemptOutcome::Running | AttemptOutcome::Interrupted => return Ok(None),
        AttemptOutcome::Committed(output) | AttemptOutcome::Rejected { output, .. } => Ok(output),
        AttemptOutcome::Failed(error) => Err(error.as_ref()),
        AttemptOutcome::Cancelled { result } => result.as_ref().map_err(std::sync::Arc::as_ref),
    };
    let request = attempt
        .request_metadata
        .as_ref()
        .map(model_request_receipt)
        .transpose()?;
    let (binding, accounting, model, model_observation, timing, orchestration) = match output {
        Ok(output) => match model_response_receipt(output)? {
            Some(receipt) => (
                Some(receipt.binding),
                receipt.response.accounting,
                receipt.response.model,
                receipt.response.model_observation,
                receipt.response.timing,
                receipt.response.orchestration,
            ),
            None => (
                request.as_ref().map(|request| request.binding.clone()),
                unknown(&output.usage),
                String::new(),
                None,
                None,
                Default::default(),
            ),
        },
        Err(error) => match model_failure_receipt(error)? {
            Some(receipt) => (
                Some(receipt.binding),
                receipt.accounting,
                String::new(),
                receipt.model_observation,
                None,
                Default::default(),
            ),
            None => (
                request.as_ref().map(|request| request.binding.clone()),
                unknown(&error.usage),
                String::new(),
                None,
                None,
                Default::default(),
            ),
        },
    };
    let model = if let Some(observation) = &model_observation {
        observation.sent_model.clone()
    } else if model.is_empty() {
        binding
            .as_ref()
            .map_or_else(String::new, |binding| binding.requested_model.clone())
    } else {
        model
    };
    let provider = binding
        .as_ref()
        .map_or_else(String::new, |binding| binding.provider_instance_id.clone());
    Ok(Some(InferenceBillingRecord {
        inference_id: attempt.attempt_id.clone(),
        purpose: binding.as_ref().map(|binding| binding.purpose.clone()),
        provider_instance_id: provider.clone(),
        provider,
        model,
        model_observation,
        reasoning_effort: request
            .as_ref()
            .and_then(|request| request.reasoning.as_ref())
            .and_then(|reasoning| reasoning.effort.clone()),
        context_window: binding.as_ref().and_then(|binding| binding.context_window),
        accounting,
        prompt_generation: request
            .as_ref()
            .and_then(|request| request.prompt.as_ref())
            .map(|prompt| prompt.generation),
        prompt_cache_policy: request
            .as_ref()
            .and_then(|request| request.prompt.as_ref())
            .map(|prompt| prompt.prompt_cache_policy.clone()),
        prefix_changed_reason: request
            .as_ref()
            .and_then(|request| request.prompt.as_ref())
            .map(|prompt| prompt.prefix_changed_reason),
        orchestration,
        timing,
        recorded_at,
    }))
}

fn unknown(usage: &ModelUsage) -> InferenceAccounting {
    InferenceAccounting {
        usage: pl_protocol::UsageReport {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            cache_write_tokens: usage.cache_write_tokens,
            reasoning_tokens: usage.reasoning_tokens,
            total_tokens: usage
                .input_tokens
                .zip(usage.output_tokens)
                .and_then(|(input, output)| input.checked_add(output)),
        },
        ..Default::default()
    }
}
