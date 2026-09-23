//! Studio wording for model-owned context compaction.
use pl_model::{
    completion::OpenAiCompactionMode,
    runtime::{ThreadCompactionOptions, ThreadCompactionStrategy},
};

/// Selects the product summary policy without changing provider capabilities.
pub(crate) fn policy(mode: OpenAiCompactionMode) -> ThreadCompactionOptions {
    ThreadCompactionOptions {
        strategy: match mode {
            OpenAiCompactionMode::Local => ThreadCompactionStrategy::TextSummary,
            OpenAiCompactionMode::RemoteV2 => ThreadCompactionStrategy::PreferNative,
        },
        instructions: include_str!("prompts/compact.md").into(),
        requirement: "请根据以上完整上下文生成压缩摘要。".into(),
        summary_prefix: "以下是此前对话的压缩摘要。".into(),
        max_output_tokens: None,
    }
}

use pl_core::thread::{
    context_preparation::{
        ContextPreparation, ContextPreparationHook, ContextPreparationRequest, ContextPreparer,
    },
    extensions::ExtensionMutation,
};
use pl_model::{
    config::ResolvedModelRoute,
    runtime::{ModelRuntime, ThreadModel},
};

/// Binds the policy to the same frozen route as the main model session.
pub(crate) fn preparer(
    route: &ResolvedModelRoute,
    mode: OpenAiCompactionMode,
) -> Result<Option<ContextPreparer>, pl_model::PureError> {
    let Some(limit) = route.model.resolved_auto_compact_limit() else {
        return Ok(None);
    };
    Ok(Some(ContextPreparer::new(StudioCompaction {
        model: ThreadModel::new(ModelRuntime::from_route(route)?, route.reasoning_config()),
        limit,
        reasoning_effort: route
            .effort
            .as_ref()
            .map(|effort| effort.as_str().to_owned()),
        context_window: route.model.resolved_context_window(),
        options: policy(mode),
    })))
}

#[derive(Debug)]
struct StudioCompaction {
    model: ThreadModel,
    limit: u64,
    reasoning_effort: Option<String>,
    context_window: Option<u64>,
    options: ThreadCompactionOptions,
}
impl ContextPreparationHook for StudioCompaction {
    async fn before_step(&self, request: ContextPreparationRequest) -> ContextPreparation {
        let estimate = match self.model.estimate_input(&request.model).await {
            Ok(estimate) => estimate
                .map(|estimate| estimate.tokens)
                .or_else(|| request.previous_usage.and_then(|usage| usage.input_tokens)),
            Err(error) => {
                return ContextPreparation::Failed {
                    error,
                    mutations: vec![],
                };
            }
        };
        if request
            .model
            .context
            .records
            .iter()
            .all(|record| record.source == pl_core::context::ContextSource::Instruction)
            || !estimate.is_some_and(|tokens| tokens >= self.limit)
        {
            return ContextPreparation::Unchanged;
        }
        let turn_id = request.model.turn_id.clone();
        let id = format!(
            "studio.compaction:{}:{}",
            request.model.attempt_id, request.extension_sequence
        );
        let mut model_request = request.model;
        model_request.attempt_id = id.clone();
        match self
            .model
            .compact(model_request, self.options.clone())
            .await
        {
            Ok(result) => {
                let receipt = CompactionReceipt {
                    inference_id: id.clone(),
                    binding: result.binding,
                    reasoning_effort: self.reasoning_effort.clone(),
                    context_window: self.context_window,
                    turn_id: turn_id.clone(),
                    accounting: result.accounting,
                    model_observation: result.model_observation,
                    implementation: Some(
                        match result.implementation {
                            ThreadCompactionStrategy::TextSummary => "textSummary",
                            ThreadCompactionStrategy::PreferNative => "native",
                        }
                        .into(),
                    ),
                    error: None,
                };
                match receipt_mutation(id, receipt) {
                    Ok(mutation) => ContextPreparation::Replaced {
                        replacement: result.replacement,
                        mutations: vec![mutation],
                    },
                    Err(error) => ContextPreparation::Failed {
                        error,
                        mutations: vec![],
                    },
                }
            }
            Err(error) => {
                let receipt = pl_model::runtime::model_failure_receipt(&error)
                    .ok()
                    .flatten()
                    .map(|receipt| CompactionReceipt {
                        inference_id: id.clone(),
                        binding: receipt.binding,
                        reasoning_effort: self.reasoning_effort.clone(),
                        context_window: self.context_window,
                        turn_id: turn_id.clone(),
                        accounting: receipt.accounting,
                        model_observation: receipt.model_observation,
                        implementation: None,
                        error: Some(receipt.message),
                    });
                let mutations = receipt
                    .and_then(|receipt| receipt_mutation(id, receipt).ok())
                    .into_iter()
                    .collect();
                ContextPreparation::Failed { error, mutations }
            }
        }
    }
}

/// Auxiliary model accounting is retained separately from normal turn-response receipts.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompactionReceipt {
    pub inference_id: String,
    pub binding: pl_model::runtime::ModelCallBinding,
    pub reasoning_effort: Option<String>,
    pub context_window: Option<u64>,
    pub turn_id: String,
    pub accounting: pl_model::completion::InferenceAccounting,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_observation: Option<pl_protocol::InferenceModelObservation>,
    pub implementation: Option<String>,
    pub error: Option<String>,
}
fn receipt_mutation(
    id: String,
    receipt: CompactionReceipt,
) -> Result<ExtensionMutation, pl_core::model::ModelError> {
    let payload = serde_json::to_string(&receipt).map_err(receipt_error)?;
    let payload = pl_core::context::OpaquePayload::new("pl.studio.compaction", 1, payload)
        .map_err(receipt_error)?;
    Ok(ExtensionMutation::Put {
        id,
        expected_revision: None,
        payload,
    })
}
fn receipt_error(
    source: impl std::error::Error + Send + Sync + 'static,
) -> pl_core::model::ModelError {
    pl_core::model::ModelError {
        kind: pl_core::model::ModelFailureKind::InvalidResponse,
        details: None,
        usage: Default::default(),
        source: Some(Box::new(source)),
    }
}
